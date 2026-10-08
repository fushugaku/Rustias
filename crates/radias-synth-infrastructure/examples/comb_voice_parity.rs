//! Complete native oscillator/filter/Comb/amplifier/pan pipeline; original initial states and other controls.
use radias_synth_application::{VoiceRenderer, comb::CombProgram};
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
        let input_program = fs::read(out.join(format!(
            "{}.program.bin",
            name.replacen("live-filter2-", "filter2-", 1)
        )))?;
        let program = CombProgram {
            cutoff: CombCutoffControl {
                link: input_program[97] & 128 != 0,
                cutoff: input_program[104],
                linked_cutoff: input_program[99],
                eg1_intensity: input_program[106],
                linked_eg1_intensity: input_program[101],
                eg1_velocity_sensitivity: input_program[121],
                velocity: 100,
                ..Default::default()
            },
            resonance: CombResonanceControl {
                link: input_program[97] & 128 != 0,
                resonance: input_program[105],
                linked_resonance: input_program[100],
                ..Default::default()
            },
            key_tracking: input_program[107],
            linked_key_tracking: input_program[102],
            ..Default::default()
        };
        let coefficients = program.coefficients(&comb_tables, &amplifier);
        if coefficients != plan.parameters.routing.ok_or("Comb route missing")?.second {
            return Err(format!(
                "Native Comb initial compilation differs:{name}; {coefficients:?} vs{:?}",
                plan.parameters.routing.unwrap().second
            )
            .into());
        }
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        renderer.set_filter2_immediate(coefficients);
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
        wav::write_buses(
            &out.join(format!("{name}-rust-native-comb-mix.wav")),
            &frames,
        )?;
        let passed = errors.iter().all(|&n| n == 0);
        let report = serde_json::json!({"passed":passed,"name":name,"frames":plan.reference_voice_frames,"errors":errors,"native_comb_arithmetic_and_memory_used":true,"native_comb_controller_compilation_used":true,"original_comb_controller_coefficients_used_to_render":false,"initial_actor_memory_and_clock_used":true,"other_original_compiled_controls_and_event_times_used":true,"original_comb_incoming_samples_replayed":false,"original_prepared_comb_output_samples_replayed":false,"recorded_audio_used_to_render":false,"complete_native_engine":false});
        fs::write(
            out.join(format!("{name}-comb-voice-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native complete Comb voice mismatch".into());
        }
    }
    Ok(())
}
