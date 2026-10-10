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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let mut reader = Reader {
        bytes: fs::read(root.join("runs/native-clone/vocoder-stored-controls-original.bin"))?,
        at: 0,
    };
    if reader.word() != 0x56434f31 {
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
        let count = reader.word();
        for at in 0..count {
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
            let buses = renderer
                .render_sources(frame.buses(), inputs, true)
                .map_err(|e| format!("Sample {scenes}/{at}: {e:?}"))?;
            frame = VocoderFrame::from_sources(buses, inputs);
            for (lane, actual) in frame.samples.iter().enumerate() {
                let expected = reader.word() as i32;
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
        "passed": scenes == 512 && target_errors + initial_errors + frame_errors + final_errors == 0,
        "stored_program_profiles": scenes, "complete_audio_frames": frames,
        "target_parameter_errors": target_errors, "initial_parameter_and_history_errors": initial_errors,
        "complete_frame_sample_errors": frame_errors, "final_parameter_and_history_errors": final_errors,
        "first_errors": first_errors,
        "source_scope": "Original SYS03b040 control chain, explicit modulation source getter/HPI data ports, original C55 receiver35 and complete D05c sample body",
        "native_controller_and_audio_interpret_firmware": false,
        "Formant_Motion_scheduler_device_or_FXD03_qualified": false
    });
    fs::write(
        root.join("runs/native-clone/vocoder-stored-controls-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if report["passed"] != true {
        return Err("Stored vocoder control/audio parity failed".into());
    }
    Ok(())
}
