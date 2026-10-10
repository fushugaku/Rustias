use radias_synth_application::vocoder::VocoderRenderer;
use radias_synth_domain::vocoder::{InterpolationTables, VocoderFrame};
use radias_synth_domain::vocoder_control::{VocoderControlInputs, VocoderProgram};
use std::{fs, path::PathBuf};
struct Reader {
    bytes: Vec<u8>,
    at: usize,
}
impl Reader {
    fn word(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let v = self.bytes[self.at..self.at + N].try_into().unwrap();
        self.at += N;
        v
    }
}
fn decode_sources(ports: [u32; 16]) -> radias_synth_domain::vocoder_sources::VocoderSources {
    radias_synth_domain::vocoder_sources::VocoderSources {
        actor: matches!(ports[15], 1 | 3).then_some(
            radias_synth_domain::vocoder_sources::VocoderActorSources {
                envelope_outputs: core::array::from_fn(|i| ports[i] as i32),
                lfo_outputs: [ports[3] as i16, ports[4] as i16],
                velocity: ports[5] as u8,
                keyboard_tracking: ports[6] as i16,
            },
        ),
        pitch_bend: ports[7] as i16,
        timbre_controller: ports[8] as u8,
        global_controller: ports[9] as i16,
        performance: core::array::from_fn(|i| ports[10 + i] as i32),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let mut reader = Reader {
        bytes: fs::read(root.join("runs/native-clone/vocoder-source-audio-original.bin"))?,
        at: 0,
    };
    if reader.word() != 0x56534f31 {
        return Err("Original controls corpus header".into());
    }
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let origin = u16::from_be_bytes(image[..2].try_into().unwrap()) as usize;
    let source = |address: usize| {
        let at = 2 + 2 * (address - origin);
        u16::from_be_bytes(image[at..at + 2].try_into().unwrap())
    };
    let tables = InterpolationTables {
        scalar_offsets: core::array::from_fn(|i| source(0x494b + i)),
        wide_offset: source(0x4971),
    };
    let controls = radias_synth_infrastructure::vocoder_tables::original();
    let mut scenes = 0usize;
    let mut frames = 0usize;
    let mut target_errors = 0usize;
    let mut source_errors = 0usize;
    let mut live_updates = 0usize;
    let mut negative_control_frames_different = 0usize;
    let mut initial_errors = 0usize;
    let mut frame_errors = 0usize;
    let mut final_errors = 0usize;
    let mut first_errors = Vec::new();
    while reader.at < reader.bytes.len() {
        let bytes = reader.bytes::<78>();
        let program = VocoderProgram { bytes: &bytes };
        let inputs = VocoderControlInputs {
            carrier_flags: reader.word() as u8,
            frequency_source: reader.word() as i16,
        };
        let ports: [u32; 16] = core::array::from_fn(|_| reader.word());
        let sources = decode_sources(ports);
        let source = sources.read(bytes[40]);
        if source != inputs.frequency_source {
            source_errors += 1;
            if first_errors.len() < 16 {
                first_errors.push(format!(
                    "scene {scenes} source: {source} != {}",
                    inputs.frequency_source
                ));
            }
        }
        let inputs = VocoderControlInputs {
            frequency_source: source,
            ..inputs
        };
        let targets = program
            .compile_targets(inputs, &controls)
            .map_err(|e| format!("Targets: {e:?}"))?;
        for (at, actual) in targets.iter().enumerate() {
            let expected = reader.word() as u16;
            if *actual != expected {
                target_errors += 1;
                if first_errors.len() < 16 {
                    first_errors.push(format!(
                        "scene {scenes} target {at:x}: {actual:04x} != {expected:04x}"
                    ));
                }
            }
        }
        let mut renderer =
            VocoderRenderer::from_program(program, inputs, &controls, tables.clone())
                .map_err(|e| format!("Program: {e:?}"))?;
        for (at, actual) in renderer
            .processor
            .parameters
            .iter()
            .chain(&renderer.processor.state)
            .enumerate()
        {
            let expected = reader.word() as u16;
            if *actual != expected {
                initial_errors += 1;
                if first_errors.len() < 16 {
                    first_errors.push(format!(
                        "scene {scenes} initial {at:x}: {actual:04x} != {expected:04x}"
                    ));
                }
            }
        }
        let mut unmodulated = VocoderRenderer {
            processor: renderer.processor.clone(),
            tables: tables.clone(),
        };
        let count = reader.word();
        for at in 0..count {
            if reader.word() != 0 {
                let ports = core::array::from_fn(|_| reader.word());
                renderer.publish_sources(program, decode_sources(ports), &controls);
                live_updates += 1;
            }
            let mut frame = VocoderFrame {
                samples: core::array::from_fn(|_| reader.word() as i32),
            };
            let inputs = core::array::from_fn(|pair| radias_synth_domain::pan::StereoFrame {
                left: radias_synth_domain::Sample(frame.samples[2 * pair]),
                right: radias_synth_domain::Sample(frame.samples[2 * pair + 1]),
            });
            if VocoderFrame::from_sources(frame.buses(), inputs) != frame {
                return Err("Fixture has unmodeled frame lanes".into());
            }
            let bad_buses = unmodulated
                .render_sources(frame.buses(), inputs, true)
                .map_err(|e| format!("Negative control: {e:?}"))?;
            let buses = renderer
                .render_sources(frame.buses(), inputs, true)
                .map_err(|e| format!("Sample {scenes}/{at}: {e:?}"))?;
            frame = VocoderFrame::from_sources(buses, inputs);
            let expected_frame: [i32; 17] = core::array::from_fn(|_| reader.word() as i32);
            let bad_frame = VocoderFrame::from_sources(bad_buses, inputs);
            negative_control_frames_different += usize::from(bad_frame.samples != expected_frame);
            for (lane, actual) in frame.samples.iter().enumerate() {
                let expected = expected_frame[lane];
                if *actual != expected {
                    frame_errors += 1;
                    if first_errors.len() < 16 {
                        first_errors.push(format!(
                            "scene {scenes} sample {at}/{lane}: {actual} != {expected}"
                        ));
                    }
                }
            }
            frames += 1;
        }
        for actual in renderer
            .processor
            .parameters
            .iter()
            .chain(&renderer.processor.state)
        {
            final_errors += usize::from(*actual != reader.word() as u16);
        }
        scenes += 1;
    }
    let report = serde_json::json!({
        "passed": scenes == 512 && live_updates == 4096 && negative_control_frames_different > 0 && target_errors + source_errors + initial_errors + frame_errors + final_errors == 0,
        "stored_program_profiles": scenes, "complete_audio_frames": frames,
        "live_source_publications":live_updates,"negative_control_frames_different":negative_control_frames_different,"source_getter_errors": source_errors, "target_parameter_errors": target_errors, "initial_parameter_and_history_errors": initial_errors,
        "complete_frame_sample_errors": frame_errors, "final_parameter_and_history_errors": final_errors,
        "first_errors": first_errors,
        "source_scope": "Original SYS03b040 control chain, original modulation source getters with explicit envelope/MIDI data ports and HPI services, original C55 receiver35 and complete D05c sample body",
        "native_controller_and_audio_interpret_firmware": false,
        "Formant_Motion_scheduler_device_or_FXD03_qualified": false
    });
    fs::write(
        root.join("runs/native-clone/vocoder-source-audio-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if report["passed"] != true {
        return Err("Stored vocoder control/audio parity failed".into());
    }
    Ok(())
}
