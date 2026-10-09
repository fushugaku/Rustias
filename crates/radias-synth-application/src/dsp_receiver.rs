//! Direct memory-command side of the original E319 receive use case.
//! Bus adapters own the storage and clock. No CPU interpreter is used here.

pub trait ParameterMemory {
    fn read_word(&self, address: u16) -> u16;
    fn write_word(&mut self, address: u16, value: u16);
    /// The Slave image reads4e2/4e3; the Master reads602/603. The memory
    /// adapter identifies its endpoint and supplies the corresponding inputs.
    fn noise_boot_inputs(&self) -> (u16, u16) {
        (self.read_word(0x602), self.read_word(0x603))
    }
    fn noise_boot_ready(&self) -> bool {
        // Slave readiness is its separate4e4 word, even when input4e2 is zero.
        self.read_word(0x602) != 0
    }
    /// Narrow adapters map only16-bit word addresses. A full DSP data adapter
    /// overrides both methods to expose its23-bit upload destination space.
    fn upload_word_mapped(&self, address: u32) -> bool {
        address <= u32::from(u16::MAX)
    }
    fn write_upload_word(&mut self, address: u32, value: u16) {
        assert!(address <= u32::from(u16::MAX));
        self.write_word(address as u16, value);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Ready,
    OpcodeOutOfRange,
    UnsupportedOpcode(u16),
    AwaitingInput,
    UnmappedUploadWord(u32),
}

/// Common direct parameter entry. Unsupported families retain their packet
/// until the owner supplies their handler; they are never silently discarded.
pub fn receive_parameter_command(
    memory: &mut impl ParameterMemory,
    base: u16,
    filter_mix: &radias_synth_domain::filter_control::FilterMixTable,
) -> ReceiveOutcome {
    match memory.read_word(base.wrapping_add(1)) {
        10 | 11 => receive_upload_command(memory, base),
        28 | 32 => receive_phase_command(memory, base),
        33..=40 => receive_shared_command(memory, base),
        16 | 17 | 23..=27 | 29 | 30 => receive_pitch_command(memory, base),
        18..=21 => receive_filter_command(memory, base, filter_mix),
        _ => receive_memory_command(memory, base),
    }
}

/// E3da/E3e8 start/continue a data upload. Payload reads and writes remain
/// interleaved, including overlap with the mailbox. This copies data directly;
/// it neither interprets uploaded code nor initializes the synthesis engine.
pub fn receive_upload_command(memory: &mut impl ParameterMemory, base: u16) -> ReceiveOutcome {
    use radias_synth_domain::parameter_upload::{UploadProgress, transfer_count};
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    if !matches!(opcode, 10 | 11) {
        return ReceiveOutcome::UnsupportedOpcode(opcode);
    }
    let mut cursor = base.wrapping_add(2);
    let mut progress = if opcode == 10 {
        let remaining = read_long(memory, cursor);
        cursor = cursor.wrapping_add(2);
        let destination = read_long(memory, cursor);
        cursor = cursor.wrapping_add(2);
        UploadProgress {
            destination,
            remaining,
        }
    } else {
        UploadProgress {
            destination: read_long(memory, base.wrapping_add(0xa6)),
            remaining: read_long(memory, base.wrapping_add(0xa8)),
        }
    };
    let count = transfer_count(take(memory, &mut cursor));
    let mut checked = progress;
    for _ in 0..count {
        let address = checked.word_address();
        if !memory.upload_word_mapped(address) {
            return ReceiveOutcome::UnmappedUploadWord(address);
        }
        checked.advance();
    }
    for _ in 0..count {
        let value = take(memory, &mut cursor);
        memory.write_upload_word(progress.word_address(), value);
        progress.advance();
    }
    write_long(memory, base.wrapping_add(0xa6), progress.destination);
    write_long(memory, base.wrapping_add(0xa8), progress.remaining);
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

/// E0cc/E144/E154/E220/E278 and global vocoder banks. These commands publish
/// data only; the vocoder's sample processing remains a separate domain.
pub fn receive_shared_command(memory: &mut impl ParameterMemory, base: u16) -> ReceiveOutcome {
    use radias_synth_domain::fixed::{multiply_q15, saturate};
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    if !matches!(opcode, 33..=40) {
        return ReceiveOutcome::UnsupportedOpcode(opcode);
    }
    let mut cursor = base.wrapping_add(2);
    match opcode {
        33 => {
            let (first, second) = memory.noise_boot_inputs();
            if !memory.noise_boot_ready() {
                return ReceiveOutcome::AwaitingInput;
            }
            let seeds = radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(
                first as i16,
                second as i16,
            );
            for (offset, bank) in [
                (0, seeds.primary.map(|v| (v.0 >> 16) as u16)),
                (20, seeds.secondary.map(|v| (v.0 >> 16) as u16)),
                (22, seeds.mixer.map(|v| (v.state >> 16) as u16)),
            ] {
                for (slot, value) in bank.into_iter().enumerate() {
                    memory.write_word(0x3000 + offset + 64 * slot as u16, value);
                }
            }
        }
        34 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let source = take(memory, &mut cursor);
                let target = take(memory, &mut cursor);
                let value =
                    saturate((i64::from(read_long(memory, source) as i32) >> 1) + (5460_i64 << 16));
                write_long(memory, target, value as u32);
            }
        }
        35 => {
            for (source, target) in [(184, 185), (186, 187), (191, 192), (242, 243)] {
                memory.write_word(0x3800 + target, memory.read_word(0x3800 + source));
            }
            let value = read_long(memory, 0x3800 + 244);
            write_long(memory, 0x3800 + 246, value);
            for source in (280..=348).step_by(2) {
                memory.write_word(0x3800 + source + 1, memory.read_word(0x3800 + source));
            }
            for address in (0x3c00..0x3c00 + 148).chain(0x3c00 + 164..0x3c00 + 298) {
                memory.write_word(address, 0);
            }
        }
        36 | 37 => {
            for index in 0..16 {
                let value = take(memory, &mut cursor);
                let address = if opcode == 36 {
                    0x3c00 + 148 + index
                } else {
                    0x3c00 + 26 + 8 * index
                };
                memory.write_word(address, value);
            }
        }
        38 => {
            let source = take(memory, &mut cursor);
            let target = take(memory, &mut cursor);
            memory.write_word(source, 0);
            let copied = read_long(memory, source.wrapping_add(130));
            write_long(memory, target.wrapping_sub(2), copied);
            let input = i64::from(memory.read_word(source.wrapping_add(124)) as i16) << 16;
            write_long(memory, target, saturate(input - (32767_i64 << 16)) as u32);
            let gain = memory.read_word(source.wrapping_add(128)) as i16;
            let scale_address = target.wrapping_add(35);
            let scale = ((u32::from(memory.read_word(scale_address.wrapping_sub(1))) << 16)
                | u32::from(memory.read_word(scale_address))) as i32;
            let product = multiply_q15(scale, gain) >> 4;
            write_long(memory, target.wrapping_add(4), saturate(product) as u32);
            let inverse = saturate((32767_i64 << 16) - (i64::from(gain) << 16));
            memory.write_word(source.wrapping_add(129), (inverse >> 16) as u16);
            let inverse = memory.read_word(source.wrapping_add(129)) as i16;
            let scale = ((u32::from(memory.read_word(scale_address.wrapping_sub(1))) << 16)
                | u32::from(memory.read_word(scale_address))) as i32;
            write_long(
                memory,
                target.wrapping_add(2),
                saturate(multiply_q15(scale, inverse) >> 4) as u32,
            );
            memory.write_word(scale_address.wrapping_sub(1), 0);
            memory.write_word(scale_address, 0);
        }
        39 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let source = take(memory, &mut cursor);
                memory.write_word(source.wrapping_add(85), memory.read_word(source));
                memory.write_word(source.wrapping_add(86), memory.read_word(source));
            }
        }
        40 => {
            for index in 0..16 {
                memory.write_word(0x3c00 + 26 + 8 * index, 0);
            }
        }
        _ => unreachable!(),
    }
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

/// E5a6/E614 initialize the five Unison carrier phases and a second phase
/// pair. Both targets are explicit packet words; aliasing follows store order.
pub fn receive_phase_command(memory: &mut impl ParameterMemory, base: u16) -> ReceiveOutcome {
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    if !matches!(opcode, 28 | 32) {
        return ReceiveOutcome::UnsupportedOpcode(opcode);
    }
    let mut cursor = base.wrapping_add(2);
    let count = u32::from(take(memory, &mut cursor)) + 1;
    for _ in 0..count {
        let address = take(memory, &mut cursor);
        let code = take(memory, &mut cursor) as i16;
        let second = take(memory, &mut cursor);
        let phases = radias_synth_domain::unison_pitch::unison_phases_signed(code, opcode == 32);
        memory.write_word(second, (phases[0].0 >> 16) as u16);
        memory.write_word(second.wrapping_add(1), phases[0].0 as u16);
        memory.write_word(address, (phases[0].0 >> 16) as u16);
        memory.write_word(address.wrapping_add(1), phases[0].0 as u16);
        for (offset, phase) in [2, 4, 8, 10].into_iter().zip(phases.into_iter().skip(1)) {
            write_long(memory, address.wrapping_add(offset), phase.0);
        }
    }
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

/// E481/E49c/E4ba Filter type/frequency/resonance packet families. Coefficient
/// arithmetic belongs to the existing domain kernels; this use case preserves
/// packet order and publishes only the voice parameter writes, not DSP CPU
/// registers or temporary arithmetic scratch words.
pub fn receive_filter_command(
    memory: &mut impl ParameterMemory,
    base: u16,
    mix: &radias_synth_domain::filter_control::FilterMixTable,
) -> ReceiveOutcome {
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    if !matches!(opcode, 18..=21) {
        return ReceiveOutcome::UnsupportedOpcode(opcode);
    }
    let mut cursor = base.wrapping_add(2);
    let count = u32::from(take(memory, &mut cursor)) + 1;
    for _ in 0..count {
        let address = take(memory, &mut cursor);
        let high = take(memory, &mut cursor);
        if opcode == 18 {
            memory.write_word(address, high);
            for (index, value) in mix.weights(high).iter().copied().enumerate() {
                memory.write_word(address.wrapping_add(19 + 2 * index as u16), value as u16);
            }
            continue;
        }
        let low = take(memory, &mut cursor);
        write_long(memory, address, (u32::from(high) << 16) | u32::from(low));
        if opcode == 21 {
            let depth = take(memory, &mut cursor);
            write_long(memory, address.wrapping_add(74), u32::from(depth) << 16);
            let inverse = radias_synth_domain::fixed::saturate(
                0x8000_0000_i64 - (i64::from(depth as i16) << 16),
            );
            memory.write_word(address.wrapping_add(75), (inverse >> 16) as u16);
        }
        let frequency = if opcode != 20 {
            address
        } else {
            address.wrapping_sub(2)
        };
        let coefficients = radias_synth_domain::filter_control::compile(
            read_long(memory, frequency) as i32,
            read_long(memory, frequency.wrapping_add(2)) as i32,
            read_long(memory, 0x4024) as i32,
        );
        write_long(
            memory,
            frequency.wrapping_add(4),
            coefficients.integrator_gain as u32,
        );
        memory.write_word(frequency.wrapping_add(8), coefficients.post_gain as u16);
        memory.write_word(
            frequency.wrapping_add(10),
            coefficients.post_feedback as u16,
        );
        write_long(
            memory,
            frequency.wrapping_sub(4),
            coefficients.feedback as u32,
        );
    }
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

fn read_long(memory: &impl ParameterMemory, address: u16) -> u32 {
    // The second half of a C55 double-word address is its paired word, even
    // when the packet gives an odd address. Sequential word packets differ.
    (u32::from(memory.read_word(address)) << 16) | u32::from(memory.read_word(address ^ 1))
}

fn write_long(memory: &mut impl ParameterMemory, address: u16, value: u32) {
    memory.write_word(address, (value >> 16) as u16);
    memory.write_word(address ^ 1, value as u16);
}

/// E44b/E466/E51d/E53a/E555 oscillator pitch packets. Original ROM data are read through the memory
/// port, including raw codes outside the normal typed PitchCode range. No DSP
/// instruction execution or recorded coefficient values are used.
pub fn receive_pitch_command(memory: &mut impl ParameterMemory, base: u16) -> ReceiveOutcome {
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    if !matches!(opcode, 16 | 17 | 23..=27 | 29 | 30) {
        return ReceiveOutcome::UnsupportedOpcode(opcode);
    }
    let mut cursor = base.wrapping_add(2);
    let count = u32::from(take(memory, &mut cursor)) + 1;
    for _ in 0..count {
        let address = take(memory, &mut cursor);
        let code = take(memory, &mut cursor);
        memory.write_word(address, code);
        if opcode == 27 {
            publish_unison_detune(memory, address);
            continue;
        }
        if opcode == 23 {
            let ratio = take(memory, &mut cursor);
            memory.write_word(address.wrapping_add(1), ratio);
        }
        let primary = if opcode == 17 {
            address.wrapping_sub(35)
        } else {
            address
        };
        if opcode != 17 {
            let increment = radias_synth_domain::fixed::saturate(packet_increment(memory, code));
            write_long(memory, primary.wrapping_add(2), increment as u32);
        }
        if opcode == 25 {
            let value = radias_synth_domain::fixed::saturate(packet_increment(memory, code) << 4)
                .min(0x4000_0000);
            memory.write_word(primary.wrapping_add(8), (value >> 16) as u16);
        }
        let sum = i64::from(memory.read_word(primary) as i16)
            + i64::from(memory.read_word(primary.wrapping_add(35)) as i16);
        let secondary = radias_synth_domain::fixed::saturate(sum << 16).max(0);
        let secondary_code = (secondary >> 16) as u16;
        let increment = packet_increment(memory, secondary_code);
        let increment = if opcode == 17 {
            increment as u32
        } else {
            radias_synth_domain::fixed::saturate(increment) as u32
        };
        write_long(memory, primary.wrapping_add(36), increment);
        let edge = radias_synth_domain::bandlimit::edge_coefficient(
            radias_synth_domain::pitch::PitchCode::new(secondary_code).unwrap(),
            memory.read_word(primary.wrapping_add(41)) != 0,
        );
        memory.write_word(primary.wrapping_add(42), edge as u16);
        if opcode == 16 {
            let increment = read_long(memory, primary.wrapping_add(2));
            let bandwidth = packet_bandwidth(memory, increment);
            memory.write_word(primary.wrapping_add(8), bandwidth as u16);
        }
        let bandwidth = packet_bandwidth(memory, read_long(memory, primary.wrapping_add(36)));
        memory.write_word(primary.wrapping_add(43), bandwidth as u16);
        if opcode == 23 {
            let ratio = memory.read_word(primary.wrapping_add(1));
            let offset = (ratio as i16 >> 8).wrapping_mul(2) as u16;
            let base = read_long(memory, 0x4232_u16.wrapping_add(offset)) as i32;
            let fraction = memory.read_word(0x4332 + ((ratio >> 1) & 127)) as i16;
            let value = radias_synth_domain::fixed::saturate(
                radias_synth_domain::pitch::fractional_increment(base, fraction),
            );
            write_long(memory, primary.wrapping_add(8), value as u32);
        }
        if opcode == 26 {
            publish_unison_detune(memory, primary.wrapping_add(4));
            let increment = read_long(memory, primary.wrapping_add(2));
            let index = ((increment as i32) >> 24) as u16;
            let gain = memory.read_word(0x47cb_u16.wrapping_add(index)) as i16;
            memory.write_word(primary.wrapping_add(18), gain as u16);
            let square = radias_synth_domain::fixed::high_product(gain, gain);
            let shaped = radias_synth_domain::fixed::high_product((square >> 16) as i16, 0x7333)
                + radias_synth_domain::fixed::high_product(gain, 0x0ccc);
            memory.write_word(
                primary.wrapping_add(32),
                (radias_synth_domain::fixed::saturate(shaped) >> 16) as u16,
            );
        }
        if opcode == 29 {
            let index = (code as i16 >> 8) as u16;
            let scale = memory.read_word(0x484b_u16.wrapping_add(index));
            memory.write_word(primary.wrapping_add(8), scale);
        }
        if opcode == 30 {
            let frequency = radias_synth_domain::noise_control::formant_frequency(
                radias_synth_domain::pitch::PhaseIncrement(read_long(
                    memory,
                    primary.wrapping_add(2),
                )),
            );
            memory.write_word(primary.wrapping_add(9), frequency as u16);
        }
    }
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

fn publish_unison_detune(memory: &mut impl ParameterMemory, address: u16) {
    let tables = radias_synth_domain::unison_pitch::UnisonDetuneTable {
        coefficients: core::array::from_fn(|i| memory.read_word(0x4788 + i as u16) as i16),
    };
    let pitch = tables.compile_signed(
        radias_synth_domain::pitch::PhaseIncrement(read_long(memory, address.wrapping_sub(2))),
        memory.read_word(address) as i16,
    );
    for (offset, value) in [
        (4, pitch.increments[1]),
        (6, pitch.increments[2]),
        (8, pitch.averaging_center),
        (10, pitch.increments[3]),
        (12, pitch.increments[4]),
    ] {
        write_long(memory, address.wrapping_add(offset), value.0);
    }
}

fn packet_increment(memory: &impl ParameterMemory, code: u16) -> i64 {
    // D5de masks the byte-scaled note index withFE, discarding the input sign
    // bit before its ROM lookup. Retain the raw code in parameter memory.
    let base = read_long(memory, 0x4032 + 2 * ((code >> 8) & 127)) as i32;
    let fraction = memory.read_word(0x4332 + ((code >> 1) & 127)) as i16;
    radias_synth_domain::pitch::fractional_increment(base, fraction)
}

pub fn packet_bandwidth(memory: &impl ParameterMemory, increment: u32) -> i16 {
    let index = ((increment as i32) >> 24) as u16;
    let address = 0x43b3_u16.wrapping_add(index);
    let fraction = (increment >> 9) & 32767;
    radias_synth_domain::bandlimit::interpolate_words(
        memory.read_word(address) as i16,
        memory.read_word(address.wrapping_add(1)) as i16,
        fraction as u16,
    )
}

/// Run a complete memory handler and its original mailbox cleanup. The hot
/// E319 entry dispatches the opcode without checking the header word. Arithmetic
/// handlers are separate synthesis use cases and must be routed by the adapter.
/// Unsupported commands leave both memory and readiness unchanged.
pub fn receive_memory_command(memory: &mut impl ParameterMemory, base: u16) -> ReceiveOutcome {
    let opcode = memory.read_word(base.wrapping_add(1));
    if opcode >= 42 {
        return ReceiveOutcome::OpcodeOutOfRange;
    }
    let mut cursor = base.wrapping_add(2);
    match opcode {
        0 | 1 | 22 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let address = take(memory, &mut cursor);
                let value = take(memory, &mut cursor);
                memory.write_word(address, value);
                if opcode != 0 {
                    let low = take(memory, &mut cursor);
                    memory.write_word(address.wrapping_add(1), low);
                }
            }
        }
        2 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            let mut target = take(memory, &mut cursor);
            for _ in 0..count {
                let value = take(memory, &mut cursor);
                memory.write_word(target, value);
                target = target.wrapping_add(1);
            }
        }
        3..=8 | 12..=14 => {}
        9 => {
            let mut source = take(memory, &mut cursor);
            let mut target = take(memory, &mut cursor);
            for _ in 0..160 {
                let value = memory.read_word(source);
                memory.write_word(target, value);
                source = source.wrapping_add(1);
                target = target.wrapping_add(1);
            }
        }
        15 => {
            let source = take(memory, &mut cursor);
            let value = take(memory, &mut cursor);
            memory.write_word(source, value);
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let target = take(memory, &mut cursor);
                // The original D530 call reads this same source word each time.
                memory.write_word(target, memory.read_word(source));
            }
        }
        31 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let address = take(memory, &mut cursor);
                let _unused_value = take(memory, &mut cursor);
                // E054 publishes alternate coefficients in their original
                // order. Long copies read both source words before storing.
                for (source, target, long) in [
                    (0, 1, false),
                    (2, 3, false),
                    (41, 42, false),
                    (43, 44, false),
                    (45, 46, false),
                    (48, 49, false),
                    (50, 52, true),
                    (58, 60, true),
                    (62, 63, false),
                    (64, 65, false),
                    (66, 67, false),
                    (68, 69, false),
                    (70, 71, false),
                    (72, 73, false),
                    (74, 75, false),
                    (78, 79, false),
                    (81, 82, false),
                    (88, 89, false),
                    (90, 92, true),
                    (98, 100, true),
                    (121, 122, false),
                ] {
                    let source = address.wrapping_add(source);
                    let target = address.wrapping_add(target);
                    if long {
                        let value = read_long(memory, source);
                        write_long(memory, target, value);
                    } else {
                        memory.write_word(target, memory.read_word(source));
                    }
                }
            }
        }
        41 => {
            let count = u32::from(take(memory, &mut cursor)) + 1;
            for _ in 0..count {
                let address = take(memory, &mut cursor);
                let value = take(memory, &mut cursor);
                memory.write_word(address, value);
                let inverse = (0x7fff_0000_i64 - (i64::from(value as i16) << 16))
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
                memory.write_word(address.wrapping_sub(2), (inverse >> 16) as u16);
            }
        }
        _ => return ReceiveOutcome::UnsupportedOpcode(opcode),
    }
    memory.write_word(base.wrapping_add(1), 0x7fff);
    memory.write_word(base, 0);
    ReceiveOutcome::Ready
}

fn take(memory: &impl ParameterMemory, cursor: &mut u16) -> u16 {
    let value = memory.read_word(*cursor);
    *cursor = cursor.wrapping_add(1);
    value
}
