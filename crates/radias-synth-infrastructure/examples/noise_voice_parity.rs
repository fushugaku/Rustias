//! Full dry native voice; one actor seed and original controls/input clocks.
use radias_synth_application::VoiceRenderer;
use radias_synth_application::noise::{NoiseTables, NoiseTarget};
use radias_synth_domain::{
    Sample,
    controller_noise::{NoiseControl, formant_control1, noise_control1},
    controller_primary::PrimaryControl,
    pan::StereoFrame,
    pitch::PitchCode,
    voice::VoiceFrameInputs,
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
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&image)?;
    let tables = master.waveform()?;
    let pitch = master.pitch()?;
    let noise_pitch = master.noise_pitch()?;
    let counter_seeds = radias_synth_infrastructure::firmware::formant_counter_seeds(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let live_tables = NoiseTables {
        pitch: master.pitch()?,
        noise: master.noise_pitch()?,
        counters: radias_synth_infrastructure::firmware::formant_counter_seeds(&fs::read(
            root.join("firmware/RADIAS_SYS_0200.bin"),
        )?)?,
    };
    let boot: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join("live-mixer-noise-lifecycle-reference-noise-boot-inputs.json"),
    )?)?;
    let seeds = radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(
        boot["input602"]
            .as_u64()
            .ok_or("Original accepted boot input602 absent")? as i16,
        boot["input603"]
            .as_u64()
            .ok_or("Original accepted boot input603 absent")? as i16,
    );
    for name in args {
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let noise = fs::read(out.join(format!("{name}-noise-frame-inputs.bin")))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let mut initial = plan.initial;
        initial.primary.phase = seeds.primary[0];
        initial.secondary.sync_phase(radias_synth_domain::Phase(
            seeds.secondary[0]
                .0
                .wrapping_sub(initial.secondary.increment().0),
        ));
        initial.mixer_noise = seeds.mixer[0];
        initial.primary.noise = Default::default();
        initial.primary.formant = radias_synth_domain::noise::FormantState {
            counter: counter_seeds.for_slot(0),
            filter: Default::default(),
        };
        let mut renderer = VoiceRenderer::new(initial, plan.parameters);
        let mut native_controls = false;
        let commit_path = out.join(format!("{name}-noise-control-commits.jsonl"));
        let commits: Vec<serde_json::Value> = if commit_path.exists() {
            fs::read_to_string(&commit_path)?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        let mut next_commit = 0usize;
        let mut controller_updates = 0usize;
        let mut changed_targets = 0usize;
        let mut previous_targets = [None; 3];
        let mut previous_pitch = None;
        let mut pitch_updates = 0usize;
        if matches!(
            plan.parameters.primary,
            radias_synth_domain::primary_oscillator::PrimaryParameters::Noise(_)
                | radias_synth_domain::primary_oscillator::PrimaryParameters::Formant(_)
        ) {
            let label = name
                .strip_prefix("live-")
                .and_then(|n| n.strip_suffix("-reference"))
                .ok_or("Controlled noise fixture label missing")?;
            let program = fs::read(out.join(format!("{label}.program.bin")))?;
            if program.len() != 1790 {
                return Err("Controlled noise program incomplete".into());
            }
            let control = PrimaryControl {
                control1: program[87],
                control2: program[88],
                ..Default::default()
            };
            let code =
                PitchCode::new(w(&raw, 3) as u16).ok_or("Original declared note pitch invalid")?;
            let compiled = live_tables
                .compile(
                    radias_synth_application::primary::PrimaryProgram {
                        selection: program[86],
                        control,
                    },
                    code,
                    0,
                    [0; 2],
                )
                .ok_or("Live Noise compiler rejected the original program")?;
            renderer.configure_noise_control(compiled);
            renderer.control_slew(plan.control_slew, 3);
            native_controls = true;
        }
        let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
        let mut errors = [0usize; 3];
        for (index, row) in raw.chunks_exact(704).enumerate() {
            if let Some(compiled) = renderer.noise_control_mut() {
                let code =
                    PitchCode::new(w(row, 3) as u16).ok_or("Declared DSP pitch input invalid")?;
                if previous_pitch != Some(code.raw()) {
                    compiled.pitch(code, &pitch, &noise_pitch);
                    previous_pitch = Some(code.raw());
                    pitch_updates += 1;
                }
            }
            while let Some(commit) = commits.get(next_commit) {
                let frame = commit["frame"]
                    .as_u64()
                    .ok_or("Noise commit frame absent")?;
                if frame > w(row, 0) as u64 {
                    break;
                }
                let input = commit["input"]
                    .as_array()
                    .filter(|v| v.len() == 8)
                    .ok_or("Noise commit inputs absent")?;
                let v = |i: usize| input[i].as_u64().unwrap() as u32;
                let primary = PrimaryControl {
                    control1: v(0) as u8,
                    control2: v(1) as u8,
                    control1_manual_offset: v(2) as i16,
                    control1_modulation: v(3) as i16,
                    control2_modulation: v(4) as i16,
                    control2_manual_offset: v(5) as i8,
                    lfo1: v(6) as i16,
                };
                let control = NoiseControl {
                    control2: primary.control2,
                    control2_modulation: primary.control2_modulation,
                    control2_manual_offset: primary.control2_manual_offset,
                };
                let base = primary.compose().base;
                let address = commit["address"]
                    .as_u64()
                    .ok_or("Noise commit address absent")?;
                let selection = commit["selection"]
                    .as_u64()
                    .ok_or("Noise commit selection absent")?;
                let (command, value) = if selection == 4 {
                    let target = control.colored(noise_control1(base));
                    match address {
                        0x2006 => (
                            NoiseTarget::ExcitationGain(target.color),
                            target.color as u16 as u32,
                        ),
                        0x2008 => (
                            NoiseTarget::ExcitationBias(target.frequency),
                            target.frequency as u16 as u32,
                        ),
                        _ => return Err("Unexpected Noise target".into()),
                    }
                } else if selection == 5 {
                    let target = control.formant(formant_control1(base), v(7) as i16);
                    match address {
                        0x2006 => (
                            NoiseTarget::ExcitationGain(target.input_gain),
                            target.input_gain as u16 as u32,
                        ),
                        0x2008 => (
                            NoiseTarget::ExcitationBias(target.frequency),
                            target.frequency as u16 as u32,
                        ),
                        0x2010 => (
                            NoiseTarget::FormantShape {
                                input_gain: (target.shape >> 16) as i16,
                                feedback: target.shape as i16,
                            },
                            target.shape,
                        ),
                        _ => return Err("Unexpected Formant target".into()),
                    }
                } else {
                    return Err("Unexpected Noise selection".into());
                };
                if commit["expected_target"].as_u64() != Some(value as u64) {
                    return Err(format!("{name} native Noise target differs at frame{frame} address{address:x}: {value} vs {}",commit["expected_target"]).into());
                }
                renderer
                    .noise_control_mut()
                    .ok_or("Native Noise control missing")?
                    .update(command);
                let target_index = match address {
                    0x2006 => 0,
                    0x2008 => 1,
                    _ => 2,
                };
                if previous_targets[target_index].is_some_and(|previous| previous != value) {
                    changed_targets += 1;
                }
                previous_targets[target_index] = Some(value);
                controller_updates += 1;
                next_commit += 1;
            }
            let input = &noise[index * 20..index * 20 + 20];
            let sample = renderer.next_on_bus_with_inputs(
                &tables,
                &plan.events,
                StereoFrame::default(),
                VoiceFrameInputs {
                    excitation_bias: w(input, 4) as i16,
                    mixer_bias: w(input, 2) as i16,
                },
            );
            let actual = [
                sample.left.0,
                sample.right.0,
                renderer.voice.mixer_noise.state,
            ];
            let expected = [
                w(row, 169) as i32,
                w(row, 170) as i32,
                noise
                    .get((index + 1) * 20..(index + 2) * 20)
                    .map_or(actual[2], |n| w(n, 3) as i32),
            ];
            for field in 0..3 {
                if actual[field] != expected[field] {
                    if errors[field] < 2 {
                        eprintln!(
                            "{name} frame{index} field{field}:{} vs{}",
                            actual[field], expected[field]
                        );
                    }
                    errors[field] += 1;
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
            &out.join(format!("{name}-rust-native-noise-mix.wav")),
            &frames,
        )?;
        let passed = errors == [0; 3];
        let report = serde_json::json!({"passed":passed,"name":name,"frames":plan.reference_voice_frames,"errors":errors,"native_noise_formant_and_mixer_noise_used":true,"first_mixer_actor_state_used":false,"accepted_original_boot_input_words_used":true,"primary_secondary_phases_and_mixer_state_generated_from_boot_inputs":true,"other_initial_actor_states_used":true,"original_mixer_noise_values_replayed":false,"original_input_biases_and_other_compiled_controls_and_event_times_used":true,"recorded_audio_used_to_render":false,"native_noise_controller_compilation_used":native_controls,"noise_controls_taken_from_program":native_controls,"computed_controller_target_updates":controller_updates,"changed_controller_targets":changed_targets,"computed_pitch_updates":pitch_updates,"declared_dsp_pitch_code_inputs_used":native_controls,"moving_noise_controls_qualified":changed_targets>0,"native_controller_compilation_qualified":false,"complete_native_engine":false});
        fs::write(
            out.join(format!("{name}-noise-voice-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native complete noise voice mismatch".into());
        }
    }
    Ok(())
}
