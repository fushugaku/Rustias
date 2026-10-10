use radias_synth_application::vocoder::VocoderRenderer;
use radias_synth_domain::{
    formant_motion::{FormantPlayback, FormantRecording, MotionRecord, RecordingTick},
    vocoder::{Vocoder, VocoderFrame},
};
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
    fn bytes(&mut self, count: usize) -> Vec<u8> {
        let v = self.bytes[self.at..self.at + count].to_vec();
        self.at += count;
        v
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let mut r = Reader {
        bytes: fs::read(root.join("runs/native-clone/formant-motion-original.bin"))?,
        at: 0,
    };
    if r.word() != 0x464d4f31 {
        return Err("Original Formant header".into());
    }
    let scenes = r.word();
    let mut audio_frames = 0usize;
    let mut playback_ticks = 0usize;
    let mut recording_ticks = 0usize;
    let mut captures = 0usize;
    let mut stop_events = 0usize;
    for scene in 0..scenes {
        let count = r.word() as usize;
        let index = r.word() as u16;
        let data = r.bytes(count * 16);
        let record = MotionRecord::new(&data).map_err(|_| "Motion extent")?;
        let mut playback = FormantPlayback { index };
        let mut renderer = VocoderRenderer {
            processor: Vocoder {
                parameters: core::array::from_fn(|_| r.word() as u16),
                state: core::array::from_fn(|_| r.word() as u16),
            },
            tables: radias_synth_infrastructure::vocoder_tables::interpolation(),
        };
        let ticks = r.word();
        for tick in 0..ticks {
            let expected_index = r.word() as u16;
            let expected = core::array::from_fn(|_| r.word() as u16);
            let mut comparison = playback;
            let levels = comparison.advance(record);
            if comparison.index != expected_index || levels != expected {
                return Err(format!("Original motion conversion differs {scene}/{tick}").into());
            }
            renderer.advance_formant(&mut playback, record);
            if playback != comparison {
                return Err("Playback application clock differs".into());
            }
            let samples = r.word();
            for at in 0..samples {
                let mut frame = VocoderFrame {
                    samples: core::array::from_fn(|_| r.word() as i32),
                };
                renderer
                    .processor
                    .process(&mut frame, true, &renderer.tables)
                    .map_err(|e| format!("Motion audio {e:?}"))?;
                let expected: [i32; 17] = core::array::from_fn(|_| r.word() as i32);
                if frame.samples != expected {
                    return Err(
                        format!("Complete Formant audio differs {scene}/{tick}/{at}").into(),
                    );
                }
                audio_frames += 1;
            }
            playback_ticks += 1;
        }
        for actual in renderer
            .processor
            .parameters
            .iter()
            .chain(&renderer.processor.state)
        {
            if *actual != r.word() as u16 {
                return Err(format!("Final Formant histories differ {scene}").into());
            }
        }
    }
    let recordings = r.word();
    for scene in 0..recordings {
        let mut recording = FormantRecording {
            count: r.word(),
            active: true,
        };
        let ticks = r.word();
        for tick in 0..ticks {
            let levels = core::array::from_fn(|_| r.word() as u16);
            let count = r.word();
            let captured = r.word() != 0;
            let stopped = r.word() != 0;
            let result = recording.capture(
                levels,
                radias_synth_infrastructure::vocoder_tables::formant_quantizer(),
            );
            if recording.count != count {
                return Err("Original recording count differs".into());
            }
            match result {
                RecordingTick::Captured { index, frame } => {
                    if !captured
                        || stopped
                        || index != r.word() as u16
                        || frame.as_slice() != r.bytes(16)
                    {
                        return Err(format!("Recording conversion differs {scene}/{tick}").into());
                    }
                    captures += 1;
                }
                RecordingTick::LimitReached => {
                    if captured || !stopped {
                        return Err("Original recording limit differs".into());
                    }
                    stop_events += 1;
                }
                RecordingTick::Inactive => {
                    if captured || stopped {
                        return Err("Original stopped recording changed".into());
                    }
                }
            }
            recording_ticks += 1;
        }
    }
    if r.at != r.bytes.len() || scenes != 24 || recordings != 6 {
        return Err("Incomplete original motion corpus".into());
    }
    let backup = fs::read(root.join("firmware/Radias-backup.rdl"))?;
    let library = radias_synth_infrastructure::rdl::import_library(&backup)?;
    if library.formants.len() != 16 {
        return Err("User backup Motion bank not retained".into());
    }
    let mut retained_frames = 0;
    for payload in &library.formants {
        retained_frames += radias_synth_infrastructure::rdl::formant_record(payload)?.frame_count();
    }
    let report = serde_json::json!({"passed":true,"playback_scenes":scenes,"playback_services":playback_ticks,"complete_audio_frames":audio_frames,"recording_scenes":recordings,"recording_services":recording_ticks,"recorded_frames":captures,"stop_events":stop_events,"user_backup_records_retained":library.formants.len(),"user_backup_motion_frames":retained_frames,"source_scope":"Original SYS03acdc, explicit envelope-read/publication ports, original C55 receiver37 and complete D05c sample body","callback_clock_is_declared_fixture_schedule":true,"physical_cadence_desktop_recording_or_FXD03_qualified":false});
    fs::write(
        root.join("runs/native-clone/formant-motion-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
