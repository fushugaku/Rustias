//! Complete native oscillator/filter/Comb/amplifier/pan pipeline; original initial states and other controls.
use radias_synth_application::VoiceRenderer;
use radias_synth_domain::{
    Sample,
    comb::{Comb, CombDelay, CombFeedback, CombFeedbackState},
    controller_comb::{CombCutoffControl, CombResonanceControl},
    pan::StereoFrame,
};
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice, wav};
use std::{fs, path::PathBuf};
fn w(row: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let table = MasterTables::from_host_stream(&fs::read(
        root.join("firmware/dsp-master-host-stream.bin"),
    )?)?
    .waveform()?;
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let comb_tables = radias_synth_infrastructure::firmware::comb_control_tables(&system)?;
    let amplifier = radias_synth_infrastructure::firmware::amplifier_tables(&system)?;
    for name in args {
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let exchange = fs::read(out.join(format!("{name}-comb-samples.bin")))?;
        if raw.len() / 704 != exchange.len() / 48 {
            return Err("Original Comb sample alignment differs".into());
        }
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let observations: Vec<serde_json::Value> =
            fs::read_to_string(out.join(format!("{name}-native-comb-events.jsonl")))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        let initial = plan.parameters.routing.ok_or("Comb routing absent")?.second;
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        let mut service_phase = (plan.reference_start_frame & 3) as u8;
        let mut previous = None;
        for (index, row) in raw.chunks_exact(704).enumerate() {
            let currents = (w(row, 99), w(row, 100), w(row, 107), w(row, 108));
            if previous.is_some_and(|old| old != currents) {
                service_phase = (3usize.wrapping_sub(index - 1) & 3) as u8;
                break;
            }
            previous = Some(currents);
        }
        renderer.control_slew(plan.control_slew, service_phase);
        renderer.set_filter2_immediate(initial);
        let mut target = initial;
        let mut deliveries = Vec::new();
        for event in &observations {
            // D534 slot-template reset is initial actor preparation, not a
            // delivered controller packet. E39A is the original long store.
            if event["pc"].as_u64() != Some(0xe39a) {
                continue;
            }
            let input = event["input"]
                .as_array()
                .ok_or("Comb event inputs absent")?;
            if input.len() != 19 {
                return Err("Comb event input shape differs".into());
            }
            let v = |n: usize| input[n].as_u64().ok_or("Comb control absent");
            let cutoff = CombCutoffControl {
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
            };
            let resonance = CombResonanceControl {
                link: v(0)? != 0,
                resonance: v(15)? as u8,
                linked_resonance: v(16)? as u8,
                modulation: v(17)? as i16,
                manual_offset: v(18)? as i8,
            };
            let delay = event["kind"].as_str() == Some("delay");
            let value = if delay {
                comb_tables.delay(cutoff.code(&amplifier))
            } else {
                comb_tables.compile_feedback(
                    event["cutoff_code"]
                        .as_u64()
                        .ok_or("Comb feedback code absent")? as i32,
                    resonance,
                )
            };
            if event["expected_target"].as_u64() != Some(value as u64) {
                return Err(
                    format!("Native Comb target differs:{name}: {event}; computed{value}").into(),
                );
            }
            deliveries.push((
                event["frame"]
                    .as_u64()
                    .ok_or("Comb delivery clock absent")?,
                delay,
                value,
            ));
        }
        let mut delivery_index = 0;
        // Initial coefficient currents are part of the observed actor, while
        // every delivered target is compiled from original control inputs.
        while let Some(&(frame, delay, value)) = deliveries.get(delivery_index) {
            if frame >= plan.reference_start_frame {
                break;
            }
            if delay {
                target.integrator_gain = value as i32;
            } else {
                target.feedback = value as i32;
            }
            delivery_index += 1;
        }
        renderer.set_filter2_target(target);
        let mut target_changes = 0usize;
        let mut coefficient_errors = [0usize; 2];
        let mut delay = CombDelay {
            write_cursor_bytes: w(&exchange, 2) as u16,
            group_phase: w(&exchange, 1) as u8,
            read_samples: [w(&exchange, 5) as i16, w(&exchange, 6) as i16],
            fraction: w(&exchange, 7) as i16,
            ..Default::default()
        };
        let initial = fs::read(out.join(format!("{name}-comb-initial-delay-memory.bin")))?;
        if initial.len() != 8192 {
            return Err("Comb initial buffer shape differs".into());
        }
        for (sample, bytes) in delay.samples.iter_mut().zip(initial.chunks_exact(2)) {
            *sample = i16::from_le_bytes(bytes.try_into().unwrap());
        }
        renderer.comb = Some(Comb {
            delay,
            feedback: CombFeedback {
                state: CombFeedbackState {
                    interpolated: w(&exchange, 8) as i32,
                    previous_drive: w(&exchange, 9) as i32,
                    dc_blocked: w(&exchange, 10) as i32,
                },
            },
        });
        let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
        let mut errors = [0usize; 8];
        for (index, row) in raw.chunks_exact(704).enumerate() {
            while let Some(&(frame, delay, value)) = deliveries.get(delivery_index) {
                if frame > plan.reference_start_frame + index as u64 {
                    break;
                }
                let field = if delay {
                    &mut target.integrator_gain
                } else {
                    &mut target.feedback
                };
                if *field != value as i32 {
                    target_changes += 1;
                }
                *field = value as i32;
                renderer.set_filter2_target(target);
                delivery_index += 1;
            }
            let current = renderer
                .current_filter2()
                .ok_or("Native Comb coefficients absent")?;
            let actual_coefficients = [current.feedback as u32, current.integrator_gain as u32];
            let expected_coefficients = [
                (w(row, 99) << 16) | w(row, 100),
                (w(row, 107) << 16) | w(row, 108),
            ];
            for n in 0..2 {
                if actual_coefficients[n] != expected_coefficients[n] {
                    if coefficient_errors[n] < 2 {
                        eprintln!(
                            "{name} frame{index} coefficient{n}:{} vs{}",
                            actual_coefficients[n], expected_coefficients[n]
                        );
                    }
                    coefficient_errors[n] += 1;
                }
            }
            let sample = renderer.next_on_bus(&table, &plan.events, StereoFrame::default());
            let state = renderer.comb.as_ref().ok_or("Native Comb state absent")?;
            let actual = [
                sample.left.0,
                sample.right.0,
                state.feedback.state.interpolated,
                state.feedback.state.previous_drive,
                state.feedback.state.dc_blocked,
                state.delay.read_samples[0] as u16 as i32,
                state.delay.read_samples[1] as u16 as i32,
                state.delay.fraction as u16 as i32,
            ];
            let mut expected = [
                w(row, 169) as i32,
                w(row, 170) as i32,
                actual[2],
                actual[3],
                actual[4],
                actual[5],
                actual[6],
                actual[7],
            ];
            if let Some(next) = exchange.get((index + 1) * 48..(index + 2) * 48) {
                expected[2..].copy_from_slice(&[
                    w(next, 8) as i32,
                    w(next, 9) as i32,
                    w(next, 10) as i32,
                    w(next, 5) as i32,
                    w(next, 6) as i32,
                    w(next, 7) as i32,
                ]);
            }
            for n in 0..8 {
                if actual[n] != expected[n] {
                    if errors[n] < 2 {
                        eprintln!(
                            "{name} frame{index} field{n}:{} vs{}",
                            actual[n], expected[n]
                        );
                    }
                    errors[n] += 1;
                }
            }
            let scaled = radias_synth_application::scale_bus(sample);
            frames.push([
                scaled.left,
                scaled.right,
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
            ]);
        }
        wav::write_buses(&out.join(format!("{name}-rust-live-comb-mix.wav")), &frames)?;
        let passed = errors.iter().all(|&n| n == 0) && coefficient_errors == [0; 2];
        let report = serde_json::json!({"passed":passed,"name":name,"frames":plan.reference_voice_frames,"errors":errors,"coefficient_errors":coefficient_errors,"native_comb_target_changes":target_changes,"original_four_sample_service_phase_used":true,"original_modulation_envelope_and_key_inputs_used":true,"native_comb_arithmetic_and_memory_used":true,"native_comb_controller_compilation_used":true,"original_comb_controller_coefficients_used_to_render":false,"initial_actor_memory_and_clock_used":true,"other_original_compiled_controls_and_event_times_used":true,"original_comb_incoming_samples_replayed":false,"original_prepared_comb_output_samples_replayed":false,"recorded_audio_used_to_render":false,"complete_native_engine":false});
        fs::write(
            out.join(format!("{name}-comb-live-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native complete Comb voice mismatch".into());
        }
    }
    Ok(())
}
