//! SH3 low-frequency waveforms 0164f4..0165ba and phase service 016b4a.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoWave {
    Saw,
    Pulse,
    BipolarPulse,
    Triangle,
    SampleHold,
    Sine,
    Zero,
}

pub struct LfoTables {
    pub warp: [u16; 65],
    /// Quarter-wave table, including the original preceding word.
    pub sine: [i16; 514],
    pub frequency: [u32; 128],
    pub initial_phase: [u16; 32],
    pub frequency_scale: [u32; 129],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LfoState {
    pub phase: u32,
    pub previous_random: i16,
    pub random: i16,
    pub half_cycle: u8,
}

impl LfoTables {
    /// Original 016eb8/016f54: signed modulation, interpolated rate multiplier,
    /// then unsigned bounds of the original frequency table.
    pub fn modulated_frequency(&self, base: u32, modulation: i32) -> u32 {
        if modulation == 0 {
            return base;
        }
        let phase = ((modulation.clamp(-32767, 32767) >> 1) + 16384) as u32;
        let index = ((phase >> 8) & 127) as usize;
        let start = self.frequency_scale[index];
        let difference = self.frequency_scale[index + 1].wrapping_sub(start) as i32;
        let interpolation = ((difference as i64 * (phase & 255) as i64) as u64 >> 8) as u32;
        let multiplier = start.wrapping_add(interpolation);
        let product = (multiplier as i32 as i64 * base as i32 as i64) as u64 >> 11;
        if product >> 32 != 0 {
            self.frequency[127]
        } else {
            (product as u32).clamp(self.frequency[0], self.frequency[127])
        }
    }
    pub fn phase_offset(&self, raw: u8) -> u16 {
        if raw & 0x60 == 0 {
            0
        } else {
            self.initial_phase[(raw & 31) as usize]
        }
    }
    fn warp(&self, phase: u16, shape: i8) -> u16 {
        let p = phase as u32;
        let shape = shape as i32;
        if shape == 0 {
            return phase;
        }
        let (index, argument, reverse) = if shape > 0 {
            if p < (65536 - (shape << 10)) as u32 {
                (shape as usize, p, false)
            } else {
                ((64 - shape) as usize, !p & 65535, false)
            }
        } else if p < ((-shape) << 10) as u32 {
            ((64 + shape) as usize, p, true)
        } else {
            ((-shape) as usize, !p & 65535, true)
        };
        let value = (self.warp[index] as u32).wrapping_mul(argument) >> 8;
        let value = if reverse { !value & 65535 } else { value };
        value.min(65535) as u16
    }
    fn quarter_sine(&self, phase: u16) -> i32 {
        if phase < 16384 {
            let index = (phase >> 5) as usize + 1;
            let base = self.sine[index] as i32;
            let fraction = phase as u32 & 31;
            let difference = (self.sine[index + 1] as i32 - base) as u16 as u32;
            base + (difference.wrapping_mul(fraction) >> 5) as i32
        } else {
            let mirror = 32768 - phase as u32;
            let index = (mirror >> 5) as usize + 1;
            let base = self.sine[index] as i32;
            let fraction = 32 - (mirror & 31);
            let difference = self.sine[index - 1] as i32 - base;
            base + ((difference * fraction as i32) >> 5)
        }
    }
    fn sine_bend(value: i32, shape: i8) -> i32 {
        if shape == 0 {
            return value;
        }
        let value = value as i16 as i32;
        let factor = 32767 + (((32767 - value.abs()) as i16 as i32 * shape as i32) >> 6);
        ((factor as i64 * value as i64) >> 15) as i32
    }
    pub fn value(&self, wave: LfoWave, phase: u16, shape: i8, state: LfoState) -> i16 {
        self.value_raw(wave, phase, shape, state) as i16
    }
    /// Original leaf's complete register result, before a caller narrows to a
    /// signed word. Saw at phase zero deliberately returns positive 32768.
    pub fn value_raw(&self, wave: LfoWave, phase: u16, shape: i8, state: LfoState) -> i32 {
        debug_assert!((-64..=63).contains(&shape));
        let p = phase as i32;
        match wave {
            LfoWave::Saw => 32768 - self.warp(phase, shape) as i32,
            LfoWave::Pulse => {
                if p <= 32768 + (shape as i32) * 512 {
                    32767
                } else {
                    0
                }
            }
            LfoWave::BipolarPulse => {
                if p <= 32768 + (shape as i32) * 512 {
                    32767
                } else {
                    -32768
                }
            }
            LfoWave::Triangle => {
                let p = p + 16384;
                let p = if (p as i16) < 0 { !p } else { p };
                let p = p.wrapping_mul(2);
                let bend = if shape == 0 {
                    0
                } else {
                    let product =
                        (((!p as u16 as u32).wrapping_mul(p as u16 as u32)) >> 17) as i16 as i32;
                    (product * shape as i32) >> 5
                };
                p.wrapping_add(bend).wrapping_sub(32768)
            }
            LfoWave::SampleHold => {
                let previous = state.previous_random as i32;
                let current = state.random as i32;
                if shape == 0 {
                    current
                } else if shape > 0 {
                    let amount = (((shape as u32) * 4 * (phase as u32)) >> 8) & 65535;
                    let difference = current - previous;
                    previous + (((difference as u32).wrapping_mul(amount) >> 16) as i16 as i32)
                } else {
                    let argument = !phase as u32 & 65535;
                    if argument >= ((-shape as i32) as u32) * 1024 {
                        previous
                    } else {
                        let amount = (self.warp[(64 + shape as i32) as usize] as u32)
                            .wrapping_mul(argument)
                            >> 8;
                        current
                            - (((current - previous) as u32).wrapping_mul(amount & 65535) >> 16)
                                as i16 as i32
                    }
                }
            }
            LfoWave::Sine => {
                let value = if (phase as i16) >= 0 {
                    self.quarter_sine(phase)
                } else {
                    -self.quarter_sine((phase as i16 as i32 + 32768) as u16)
                };
                let value = Self::sine_bend(value, shape);
                Self::sine_bend(value, shape)
            }
            LfoWave::Zero => 0,
        }
    }
}

impl LfoState {
    /// Original 015b42/015bce phase/random substate at note initialization.
    pub fn initialize_note(&mut self, raw_sync: u8, shared: Self, seed: &mut u16) {
        if raw_sync & 0x60 == 0x40 {
            self.phase = 0;
            self.previous_random = self.random;
            self.random = Self::next_random(seed);
        } else {
            self.phase = shared.phase;
            self.previous_random = shared.previous_random;
            self.random = shared.random;
        }
    }
    pub fn value(&self, tables: &LfoTables, wave: LfoWave, raw_phase: u8, shape: i8) -> i16 {
        let phase = ((self.phase >> 16) as u16).wrapping_add(tables.phase_offset(raw_phase));
        tables.value(wave, phase, shape, *self)
    }
    pub fn next_random(seed: &mut u16) -> i16 {
        let feedback = (*seed & 0x8805).count_ones() as u16 & 1;
        *seed = seed.wrapping_shl(1) | feedback;
        *seed as i16
    }
    /// SH3 01ef78 initializes three per-note oscillator random words before
    /// compiling note pitch. This uses the same instrument LFSR as the LFOs,
    /// even when the selected waveform does not use noise.
    pub fn initialize_oscillator_random(seed: &mut u16) -> [u16; 3] {
        core::array::from_fn(|_| Self::next_random(seed) as u16)
    }
    pub fn advance(
        &mut self,
        tables: &LfoTables,
        raw_phase: u8,
        increment: u32,
        phase_offset: i32,
        seed: &mut u16,
    ) {
        let increment = increment.wrapping_mul(2);
        let adjusted = self
            .phase
            .wrapping_add((phase_offset as u32) << 16)
            .wrapping_add((tables.phase_offset(raw_phase) as u32) << 16);
        self.phase = self.phase.wrapping_add(increment);
        let (adjusted, wrapped) = adjusted.overflowing_add(increment);
        if wrapped {
            self.previous_random = self.random;
            self.random = Self::next_random(seed);
        }
        self.half_cycle = (adjusted >> 31) as u8;
    }
}
