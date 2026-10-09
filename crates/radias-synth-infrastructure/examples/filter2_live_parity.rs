//! Native ordinary Filter2 controller targets, coefficient slew and complete dry voice.
use radias_synth_application::VoiceRenderer;
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{
    Sample,
    controller_comb::{CombCutoffControl, CombResonanceControl},
    filter_routing::Filter2Coefficients,
    pan::StereoFrame,
};
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::PreparedVoice,
    wav,
};
use std::{fs, path::PathBuf};
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn cutoff(p: &[serde_json::Value]) -> Result<CombCutoffControl, Box<dyn std::error::Error>> {
    let v = |i: usize| p[i].as_u64().ok_or("Filter2 control input absent");
    Ok(CombCutoffControl {
        link: v(0)? != 0,
        cutoff: v(1)? as u8,
        linked_cutoff: v(2)? as u8,
        manual_offset: v(3)? as i16,
        key_offset: v(4)? as i16,
        lfo_offset: v(5)? as i16,
        eg1_intensity: v(6)? as u8,
        linked_eg1_intensity: v(7)? as u8,
        eg1_manual_offset: v(8)? as i8,
        eg1_depth_modulation: v(9)? as i16,
        eg1_level: v(10)? as u16,
        velocity: v(11)? as u8,
        eg1_velocity_sensitivity: v(12)? as u8,
        additional_offset: v(13)? as i16,
        cutoff_modulation: v(14)? as i16,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    let queued_controls = std::env::args().any(|arg| arg == "--queued-controls");
    let out = root.join("runs/native-clone");
    let table = MasterTables::from_host_stream(&fs::read(
        root.join("firmware/dsp-master-host-stream.bin"),
    )?)?
    .waveform()?;
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let frequencies = firmware::controller_filter_tables(&sys)?;
    let amp = firmware::amplifier_tables(&sys)?;
    let tables = firmware::filter2_control_tables(&sys)?;
    for name in args.filter(|arg| arg != "--queued-controls") {
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let initial = plan
            .parameters
            .routing
            .ok_or("Regular Filter2 routing absent")?
            .second;
        let events = fs::read_to_string(out.join(format!("{name}-native-filter2-events.jsonl")))?
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let mut frequency = None;
        let mut resonance = None;
        let mut target = initial;
        let mut deliveries: Vec<(u64, Filter2Coefficients)> = Vec::new();
        let mut controller_inputs = 0;
        let mut initial_context_inputs = 0;
        let first_context = events
            .iter()
            .find(|e| e["kind"].as_str() == Some("coefficients"))
            .ok_or("Initial Filter2 coefficient context absent")?;
        let normalization = first_context["normalization"]
            .as_u64()
            .ok_or("Initial normalization absent")? as i32;
        let mut transport = SynthesisParameterTransport::default();
        let mut initial_words = [0u16; 160];
        initial_words[94] = initial.input_gain as u16;
        // The first unfinished compiler context supplies the not-yet-delivered
        // resonance input. Source output coefficients never populate this bank.
        let r = first_context["resonance"]
            .as_u64()
            .ok_or("Initial resonance context absent")? as u32;
        initial_words[102] = (r >> 16) as u16;
        initial_words[103] = r as u16;
        transport.restore_parameters(0, initial_words);
        transport.configure_filter2(0, normalization, initial);
        let mut sender_origin = 0u64;
        let mut received = None;
        let mut queued_packets = 0usize;
        for event in events {
            let frame = event["frame"]
                .as_u64()
                .ok_or("Filter2 delivery time absent")?;
            match event["kind"].as_str() {
                Some("input") => {
                    let p = event["input"]
                        .as_array()
                        .ok_or("Filter2 original inputs absent")?;
                    if p.len() != 19 {
                        return Err("Filter2 input shape differs".into());
                    }
                    let v = |i: usize| p[i].as_u64().ok_or("Filter2 input absent");
                    let address = event["address"]
                        .as_u64()
                        .ok_or("Filter2 packet address absent")?;
                    let value = if address == 0x2065 {
                        let value = frequencies.frequency(cutoff(p)?.code(&amp));
                        frequency = Some(value);
                        value
                    } else {
                        let r = CombResonanceControl {
                            link: v(0)? != 0,
                            resonance: v(15)? as u8,
                            linked_resonance: v(16)? as u8,
                            modulation: v(17)? as i16,
                            manual_offset: v(18)? as i8,
                        };
                        let (r, g) = tables.resonance_targets(
                            event["route"].as_u64().ok_or("Filter2 route absent")? as u8,
                            r,
                        );
                        if address == 0x2067 {
                            resonance = Some(r as u32);
                            r as u32
                        } else if address == 0x205e {
                            target.input_gain = g;
                            if !queued_controls {
                                deliveries.push((frame, target));
                            }
                            g as u16 as u32
                        } else {
                            return Err("Unknown Filter2 packet".into());
                        }
                    };
                    if event["expected_target"].as_u64() != Some(value as u64) {
                        return Err(format!(
                            "Native Filter2 controller target differs:{name}: {event}; {value}"
                        )
                        .into());
                    }
                    if queued_controls {
                        let sent = match address {
                            0x2065 => transport.filter2_frequency(sender_origin, 0, value as i32),
                            0x2067 => transport.filter2_resonance(sender_origin, 0, value as i32),
                            0x205e => transport.filter2_input_gain(sender_origin, 0, value as i16),
                            _ => unreachable!(),
                        };
                        sent.map_err(|e| format!("Filter2 sender failed: {e:?}"))?;
                        let elapsed = if address == 0x205e { 106 } else { 117 };
                        transport.advance_until(sender_origin + elapsed, |_, slot, event| {
                            assert_eq!(slot, 0);
                            if let DeliveredSynthesisParameter::Filter2(c) = event {
                                received = Some(c);
                                queued_packets += 1;
                            } else {
                                panic!("Unexpected Filter2 receiver publication");
                            }
                        });
                        sender_origin += elapsed;
                        if transport.pending() != 0 || received.is_none() {
                            return Err("Filter2 packet was not delivered".into());
                        }
                        if address == 0x205e {
                            let c = received.take().unwrap();
                            if c != target {
                                return Err(
                                    "Filter2 gain delivery rewrote earlier coefficient targets"
                                        .into(),
                                );
                            }
                            deliveries.push((frame, c));
                        }
                    }
                    controller_inputs += 1;
                }
                Some("coefficients") => {
                    let f = event["frequency"]
                        .as_u64()
                        .ok_or("Filter2 frequency context absent")?
                        as u32;
                    let r = event["resonance"]
                        .as_u64()
                        .ok_or("Filter2 resonance context absent")?
                        as u32;
                    if frequency.is_none() || resonance.is_none() {
                        if frequency.is_some_and(|native| native != f)
                            || resonance.is_some_and(|native| native != r)
                        {
                            return Err("Native Filter2 initial packet context differs".into());
                        }
                        frequency.get_or_insert(f);
                        resonance.get_or_insert(r);
                        initial_context_inputs += 1;
                    } else if frequency != Some(f) || resonance != Some(r) {
                        return Err("Original Filter2 coefficient context differs from native controller targets".into());
                    }
                    let c = radias_synth_domain::filter_control::compile(
                        f as i32,
                        r as i32,
                        event["normalization"]
                            .as_u64()
                            .ok_or("Filter2 normalization absent")? as i32,
                    );
                    if event["expected_coefficients"][0].as_u64() != Some(c.feedback as u32 as u64)
                        || event["expected_coefficients"][1].as_u64()
                            != Some(c.integrator_gain as u32 as u64)
                    {
                        return Err("Native Filter2 coefficient compiler differs".into());
                    }
                    target.feedback = c.feedback;
                    target.integrator_gain = c.integrator_gain;
                    if queued_controls {
                        if event["normalization"].as_u64() != Some(normalization as u32 as u64) {
                            return Err(
                                "Controlled Filter2 normalization changed outside declared context"
                                    .into(),
                            );
                        }
                        let c = received.take().ok_or(
                            "Filter2 source coefficient boundary has no native publication",
                        )?;
                        if c != target {
                            return Err(format!(
                                "Queued Filter2 coefficients differ: {name}: {c:?} vs {target:?}"
                            )
                            .into());
                        }
                        deliveries.push((frame, c));
                    } else {
                        deliveries.push((frame, target));
                    }
                }
                _ => return Err("Unknown Filter2 event".into()),
            }
        }
        if controller_inputs == 0 || deliveries.is_empty() {
            return Err("No native Filter2 inputs exercised".into());
        }
        let mut phase = (plan.reference_start_frame & 3) as u8;
        let mut previous = None;
        for (index, row) in raw.chunks_exact(704).enumerate() {
            let current = (
                word(row, 96),
                word(row, 99),
                word(row, 100),
                word(row, 107),
                word(row, 108),
            );
            if previous.is_some_and(|old| old != current) {
                phase = (3usize.wrapping_sub(index - 1) & 3) as u8;
                break;
            }
            previous = Some(current);
        }
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        renderer.control_slew(plan.control_slew, phase);
        renderer.set_filter2_immediate(initial);
        let mut target = initial;
        let mut next = 0;
        while let Some(&(frame, value)) = deliveries.get(next) {
            if frame >= plan.reference_start_frame {
                break;
            }
            target = value;
            next += 1;
        }
        renderer.set_filter2_target(target);
        let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
        let mut errors = [0; 2];
        let mut coefficient_errors = [0; 3];
        let mut changes = 0;
        for (index, row) in raw.chunks_exact(704).enumerate() {
            while let Some(&(frame, value)) = deliveries.get(next) {
                if frame > plan.reference_start_frame + index as u64 {
                    break;
                }
                if value != target {
                    changes += 1;
                }
                target = value;
                renderer.set_filter2_target(target);
                next += 1;
            }
            let c = renderer.current_filter2().ok_or("Native Filter2 absent")?;
            let actual = [
                c.input_gain as u16 as u32,
                c.feedback as u32,
                c.integrator_gain as u32,
            ];
            let expected = [
                word(row, 96),
                (word(row, 99) << 16) | word(row, 100),
                (word(row, 107) << 16) | word(row, 108),
            ];
            for i in 0..3 {
                if actual[i] != expected[i] {
                    if coefficient_errors[i] < 2 {
                        eprintln!(
                            "{name} frame{index} coefficient{i}:{} vs{}",
                            actual[i], expected[i]
                        );
                    }
                    coefficient_errors[i] += 1;
                }
            }
            let sample = renderer.next_on_bus(&table, &plan.events, StereoFrame::default());
            let actual = [sample.left.0, sample.right.0];
            let expected = [word(row, 169) as i32, word(row, 170) as i32];
            for i in 0..2 {
                if actual[i] != expected[i] {
                    if errors[i] < 2 {
                        eprintln!(
                            "{name} frame{index} sample{i}:{} vs{}",
                            actual[i], expected[i]
                        );
                    }
                    errors[i] += 1;
                }
            }
            let s = radias_synth_application::scale_bus(sample);
            frames.push([
                s.left,
                s.right,
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
            ]);
        }
        let audio_suffix = if queued_controls {
            "rust-filter2-queued-mix"
        } else {
            "rust-filter2-controller-mix"
        };
        wav::write_buses(&out.join(format!("{name}-{audio_suffix}.wav")), &frames)?;
        let passed = errors == [0; 2] && coefficient_errors == [0; 3];
        let report = serde_json::json!({"passed":passed,"name":name,"frames":plan.reference_voice_frames,
            "sample_errors":errors,"coefficient_errors":coefficient_errors,"native_target_changes":changes,
            "original_controller_inputs":controller_inputs,"initial_coefficient_context_inputs":initial_context_inputs,
            "original_initial_actor_and_controller_delivery_times_used":true,"other_original_compiled_controls_used":true,
            "native_Filter2_controller_compilation_and_slew_used":true,"recorded_audio_used_to_render":false,
            "production_sender_and_receiver_used":queued_controls,"queued_packets":queued_packets,
            "source_delivery_frames_and_sequential_sender_origins_declared":queued_controls,
            "original_Filter2_targets_used_to_render":false,"independent_HPI_timing_qualified":false,"complete_native_engine":false});
        let report_suffix = if queued_controls {
            "filter2-delivery-parity"
        } else {
            "filter2-live-parity"
        };
        fs::write(
            out.join(format!("{name}-{report_suffix}.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native regular Filter2 full voice mismatch".into());
        }
    }
    Ok(())
}
