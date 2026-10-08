//! Render with native shaper target compilation and independently slewed current.
use radias_synth_application::{
    VoiceRenderer,
    shaper::{ShaperMode, ShaperProgram},
};
use radias_synth_domain::{
    Sample, controller_shaper::ShaperControl, pan::StereoFrame, waveshaper::ShaperPosition,
};
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice, wav};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let table = MasterTables::from_host_stream(&fs::read(
        root.join("firmware/dsp-master-host-stream.bin"),
    )?)?
    .waveform()?;
    for name in args {
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let source_program =
            fs::read(out.join(format!("{}.program.bin", name.trim_start_matches("live-"))))?;
        let mode = ShaperMode::from_allocation(source_program[110] & 3, source_program[111] & 15)
            .ok_or("Unqualified shaper program")?;
        let mut program = ShaperProgram {
            mode,
            position: if source_program[110] & 16 != 0 {
                ShaperPosition::PreAmp
            } else {
                ShaperPosition::PreFilter
            },
            control: ShaperControl {
                depth: source_program[112],
                ..Default::default()
            },
        };
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let path = out.join(format!("{name}-native-shaper-events.jsonl"));
        let observations: Vec<serde_json::Value> = if path.exists() {
            fs::read_to_string(&path)?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        // Note preparation can commit, clear and restore the coefficient before
        // the first observed actor sample. Compile that actor's initial input;
        // do not replay its pre-voice reset writes as controller deliveries.
        let mut pre_voice_inputs = 0;
        for event in &observations {
            if event["frame"]
                .as_u64()
                .ok_or("Original shaper clock absent")?
                < plan.reference_start_frame
            {
                let input = event["input"]
                    .as_array()
                    .ok_or("Initial shaper input absent")?;
                if input.len() != 4 {
                    return Err("Initial shaper input shape differs".into());
                }
                program.control = ShaperControl {
                    depth: input[1].as_u64().ok_or("Initial depth absent")? as u8,
                    manual_offset: input[2].as_u64().ok_or("Initial offset absent")? as i16,
                    modulation: input[3].as_u64().ok_or("Initial modulation absent")? as i16,
                };
                pre_voice_inputs += 1;
            }
        }
        let compiled = program
            .parameters_with_pitch(plan.parameters.primary_pitch_code)
            .ok_or("Missing native shaper parameters")?;
        if plan.parameters.shaper != Some(compiled) {
            return Err(format!(
                "Original initial shaper coefficients differ from native constructor: {:?} vs {:?}",
                plan.parameters.shaper, compiled
            )
            .into());
        }
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        // The complete-bus output clock does not identify the DSP's four-frame
        // service phase. Observe one actor update boundary, then independently
        // generate every subsequent coefficient. No current value is replayed.
        let mut service_phase = (plan.reference_start_frame & 3) as u8;
        let mut previous = None;
        for (index, row) in raw.chunks_exact(704).enumerate() {
            let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
            let currents = (
                word(86),
                compiled.coefficients.gain_current().map(|_| word(89)),
            );
            if previous.is_some_and(|old| old != currents) {
                service_phase = (3usize.wrapping_sub(index - 1) & 3) as u8;
                break;
            }
            previous = Some(currents);
        }
        renderer.control_slew(plan.control_slew, service_phase);
        renderer.set_shaper_immediate(Some(compiled));
        let mut target_depth = compiled.coefficients.depth();
        let mut delivered = Vec::new();
        for event in &observations {
            if event["frame"]
                .as_u64()
                .ok_or("Original shaper clock absent")?
                < plan.reference_start_frame
            {
                continue;
            }
            let input = event["input"]
                .as_array()
                .ok_or("Original shaper controller input absent")?;
            if input.len() != 4 {
                return Err("Original shaper controller input shape differs".into());
            }
            let value = |n: usize| {
                input[n]
                    .as_u64()
                    .ok_or("Original shaper controller value absent")
            };
            if value(0)? as u8 & 3 != source_program[110] & 3 {
                return Err("Original delivered shaper mode differs".into());
            }
            let mut next = program;
            next.control = ShaperControl {
                depth: value(1)? as u8,
                manual_offset: value(2)? as i16,
                modulation: value(3)? as i16,
            };
            let compiled = next
                .parameters_with_pitch(plan.parameters.primary_pitch_code)
                .ok_or("Original shaper unexpectedly off")?;
            let depth = compiled.coefficients.depth();
            if event["expected_target"].as_u64() != Some(depth as u16 as u64) {
                return Err("Original live shaper target compiler differs".into());
            }
            delivered.push((
                event["frame"]
                    .as_u64()
                    .ok_or("Original shaper delivery clock absent")?,
                compiled,
            ));
        }
        let mut delivered_index = 0usize;
        let mut target_changes = 0usize;
        let mut errors = [0usize; 10];
        let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
        for (index, row) in raw.chunks_exact(704).enumerate() {
            let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
            while let Some(&(frame, target)) = delivered.get(delivered_index) {
                if frame > plan.reference_start_frame + index as u64 {
                    break;
                }
                let next = target.coefficients.depth();
                if next != target_depth {
                    target_changes += 1;
                }
                target_depth = next;
                renderer.set_shaper(Some(target));
                delivered_index += 1;
            }
            let shaper = renderer
                .current_shaper()
                .ok_or("Native shaper unexpectedly disabled")?;
            let depth = shaper.coefficients.depth();
            let feedback = shaper.coefficients.gain_current().unwrap_or(0);
            let actual = renderer.next_on_bus(&table, &plan.events, StereoFrame::default());
            let a = [
                target_depth as i32,
                depth as i32,
                feedback as i32,
                actual.left.0,
                actual.right.0,
                renderer.voice.waveshaper.state.words[0],
                renderer.voice.waveshaper.state.words[1],
                renderer.voice.waveshaper.state.words[2],
                renderer.voice.waveshaper.state.words[3],
                renderer.voice.waveshaper.state.startup_counter as i32,
            ];
            let mut e = [
                word(85) as i16 as i32,
                word(86) as i16 as i32,
                if shaper.coefficients.gain_current().is_some() {
                    word(89) as i16 as i32
                } else {
                    0
                },
                word(169) as i32,
                word(170) as i32,
                a[5],
                a[6],
                a[7],
                a[8],
                a[9],
            ];
            if let Some(next) = raw.get((index + 1) * 704..(index + 2) * 704) {
                let w = |n: usize| u32::from_le_bytes(next[n * 4..n * 4 + 4].try_into().unwrap());
                for (state, value) in e[5..9].iter_mut().enumerate() {
                    *value = ((w(153 + state * 2) << 16) | w(154 + state * 2)) as i32;
                }
                e[9] = w(93) as u16 as i32;
            }
            for field in 0..a.len() {
                if a[field] != e[field] {
                    if errors[field] < 2 {
                        eprintln!(
                            "{name} frame{index} field{field}:{} vs{}",
                            a[field], e[field]
                        );
                    }
                    errors[field] += 1;
                }
            }
            let scaled = radias_synth_application::scale_bus(actual);
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
            &out.join(format!("{name}-rust-live-shaper-mix.wav")),
            &frames,
        )?;
        let passed = errors.iter().all(|&n| n == 0);
        let report = serde_json::json!({"passed":passed,"name":name,"frames":plan.reference_voice_frames,"errors":errors,
            "native_initial_coefficient_constructor_matches_original":true,"native_target_compiler_used":true,
            "native_depth_and_feedback_slew_used":true,"recorded_shaper_currents_used_to_render":false,
            "original_shaper_delivery_times_used":true,"native_shaper_target_changes":target_changes,
            "original_four_sample_service_phase_used":true,"four_sample_service_phase":service_phase,
            "compiled_original_shaper_input_deliveries":delivered.len(),
            "original_pre_voice_controller_inputs_used_for_initial_constructor":pre_voice_inputs,
            "other_original_compiled_controls_and_event_timestamps_used":true,"initial_actor_states_used":true,
            "independent_controller_hpi_audio_parity":false,"complete_native_engine":false});
        fs::write(
            out.join(format!("{name}-shaper-live-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native shaper controller/current/audio mismatch".into());
        }
    }
    Ok(())
}
