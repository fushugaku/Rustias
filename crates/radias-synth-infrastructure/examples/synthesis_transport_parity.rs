//! Production combined queue against complete source filter receiver writes.
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{amplifier_delivery::AmplifierPacket, filter::FilterCoefficients};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u16 {
    let v = u16::from_le_bytes(raw[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;
    v
}
fn long(words: &[u16], i: usize) -> i32 {
    ((u32::from(words[i]) << 16) | u32::from(words[i + 1])) as i32
}
fn coefficients(words: &[u16], local: usize) -> FilterCoefficients {
    let base = 128 + 160 * local;
    FilterCoefficients {
        input_gain: words[base + 54] as i16,
        feedback: long(words, base + 56),
        integrator_gain: long(words, base + 64),
        post_gain: words[base + 68] as i16,
        post_feedback: words[base + 70] as i16,
        mix: core::array::from_fn(|i| words[base + 72 + 2 * i] as i16),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Expected {
    Amp(usize, i16),
    Filter(usize, FilterCoefficients),
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-filter-receiver.bin"))?;
    let mut cursor = 0;
    let mut original_calls = 0;
    let mut entries = 0;
    let mut errors = 0;
    let mut first_error = None;
    let mut snapshot_case = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor) as usize;
        let opcode = take(&raw, &mut cursor);
        let high = take(&raw, &mut cursor);
        let low = take(&raw, &mut cursor);
        let normalization = ((u32::from(high) << 16) | u32::from(low)) as i32;
        let before = (0..896)
            .map(|_| take(&raw, &mut cursor))
            .collect::<Vec<_>>();
        let _after = (0..896)
            .map(|_| take(&raw, &mut cursor))
            .collect::<Vec<_>>();
        let _hpic = take(&raw, &mut cursor);
        let _hint = take(&raw, &mut cursor);
        let count = take(&raw, &mut cursor);
        let writes = (0..count)
            .map(|_| (take(&raw, &mut cursor), take(&raw, &mut cursor)))
            .collect::<Vec<_>>();
        if opcode == 18 {
            continue;
        }
        let mut state = before.clone();
        let mut filter_outputs = Vec::new();
        for (address, value) in writes {
            if (0x2000..0x2300).contains(&address) {
                state[128 + usize::from(address - 0x2000)] = value;
                if (address - 0x2000) % 160 == 57 {
                    let local = usize::from(address - 0x2000) / 160;
                    filter_outputs.push(Expected::Filter(
                        local + 12 * chip,
                        coefficients(&state, local),
                    ));
                }
            }
        }
        let command_count = usize::from(before[2]) + 1;
        if filter_outputs.len() != command_count {
            return Err("Source filter publication boundaries incomplete".into());
        }
        if opcode == 19 && command_count == 1 && snapshot_case.is_none() {
            snapshot_case = Some((
                chip,
                normalization,
                before.clone(),
                filter_outputs[0].clone(),
            ));
        }
        for chunk in [1, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            for local in 0..5 {
                let start = 128 + 160 * local + 56;
                transport.restore_filter(local + 12 * chip, before[start..start + 16].try_into()?);
                transport.configure_filter(
                    local + 12 * chip,
                    normalization,
                    coefficients(&before, local),
                );
            }
            let mut expected = Vec::new();
            for (i, filter) in filter_outputs.iter().enumerate() {
                let address = before[3 + 3 * i];
                let slot = usize::from(address - 0x2000) / 160 + 12 * chip;
                let amp = (original_calls as i16).wrapping_add(i as i16);
                transport
                    .enqueue(0, slot, AmplifierPacket::Target(amp))
                    .map_err(|e| format!("Queue: {e:?}"))?;
                let value = long(&before, 4 + 3 * i);
                let result = if opcode == 19 {
                    transport.filter_frequency(0, slot, value)
                } else {
                    transport.filter_resonance(0, slot, value)
                };
                result.map_err(|e| format!("Queue: {e:?}"))?;
                expected.push(Expected::Amp(slot, amp));
                expected.push(filter.clone());
            }
            let mut actual = Vec::new();
            let end = command_count as u64 * (106 + 117);
            let mut clock = 0;
            while clock < end {
                clock = (clock + chunk).min(end);
                transport.advance_until(clock, |_, slot, packet| match packet {
                    DeliveredSynthesisParameter::Amplifier(AmplifierPacket::Target(value)) => {
                        actual.push(Expected::Amp(slot, value))
                    }
                    DeliveredSynthesisParameter::Filter1(value) => {
                        actual.push(Expected::Filter(slot, value))
                    }
                    _ => {}
                });
            }
            if actual != expected || transport.pending() != 0 {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":original_calls,"opcode":opcode,"chunk":chunk,"native":format!("{actual:?}"),"original":format!("{expected:?}")}));
            }
        }
        original_calls += 1;
        entries += command_count;
    }
    let (chip, normalization, before, expected) =
        snapshot_case.ok_or("Source snapshot fixture missing")?;
    let local = usize::from(before[3] - 0x2000) / 160;
    let slot = local + 12 * chip;
    let mut transport = SynthesisParameterTransport::default();
    let start = 128 + 160 * local + 56;
    transport.restore_filter(slot, before[start..start + 16].try_into()?);
    let base = coefficients(&before, local);
    transport.configure_filter(slot, normalization, base);
    transport
        .filter_frequency(0, slot, long(&before, 4))
        .map_err(|e| format!("Queue: {e:?}"))?;
    // A subsequent note/parameter change must not retroactively change a
    // completed sender payload's context or reset its earlier parameter bank.
    transport.reset_filter(slot);
    let mut later_base = base;
    later_base.input_gain = base.input_gain.wrapping_add(1);
    transport.configure_filter(slot, normalization.wrapping_add(0x1000_0000), later_base);
    let mut observed = Vec::new();
    transport.advance_until(117, |_, slot, packet| {
        if let DeliveredSynthesisParameter::Filter1(coefficients) = packet {
            observed.push(Expected::Filter(slot, coefficients));
        }
    });
    let snapshot_preserved = observed == vec![expected];
    if !snapshot_preserved {
        errors += 1;
    }
    let report = serde_json::json!({"passed":errors==0&&original_calls==5460,"whole_original_filter_calls":original_calls,"original_filter_entries":entries,
        "errors":errors,"first_error":first_error,"ordered_interleaved_AMP_and_Filter1":true,"both_endpoints":true,"CPU_clock_partitions":[1,31,3000],
        "all_original_filter_parameter_controls_and_prior_memory_are_declared_inputs":true,"source_filter_outputs_used_only_for_comparison":true,
        "production_SynthesisParameterTransport_used":true,"queued_context_and_prior_bank_preserved_across_later_reset":snapshot_preserved,
        "complete_receiver_job_DMA_or_instrument_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("synthesis-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Combined synthesis parameter transport differs".into());
    }
    Ok(())
}
