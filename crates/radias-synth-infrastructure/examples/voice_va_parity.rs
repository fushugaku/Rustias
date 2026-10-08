use radias_synth_application::VoiceRenderer;
use radias_synth_domain::{Sample, fixed::saturate, pan::StereoFrame};
use radias_synth_infrastructure::{
    firmware::MasterTables,
    prepared::{PreparedVoice, parameters},
    wav,
};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    for name in args.iter().skip(1) {
        let output = root.join("runs/native-clone");
        let raw = fs::read(output.join(format!("{name}-voice-va-inputs.bin")))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let mut voice = plan.initial;
        let mut errors = [0usize; 6];
        let mut expected_frames = Vec::new();
        for (i, r) in raw.chunks_exact(704).enumerate() {
            let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
            let actual = voice.next_sample(&table, parameters(r)?);
            let a = [
                actual.mixed.0,
                actual.filtered.0,
                actual.level as i32,
                actual.amplified.0,
                actual.stereo.left.0,
                actual.stereo.right.0,
            ];
            let e = [
                word(165) as i32,
                word(166) as i32,
                word(167) as i16 as i32,
                word(168) as i32,
                word(169) as i32,
                word(170) as i32,
            ];
            for stage in 0..6 {
                if a[stage] != e[stage] {
                    if errors[stage] < 2 {
                        eprintln!(
                            "{name} frame {i}, stage {stage}: {} != {}",
                            a[stage], e[stage]
                        );
                    }
                    errors[stage] += 1;
                }
            }
            expected_frames.push(StereoFrame {
                left: Sample(saturate((e[4] as i64) << 5)),
                right: Sample(saturate((e[5] as i64) << 5)),
            });
        }
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        let mut samples = vec![StereoFrame::default(); plan.reference_voice_frames];
        for block in samples.chunks_mut(127) {
            renderer.render(&table, &plan.events, block);
        }
        let independent_exact = samples == expected_frames;
        let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
        frames.extend(samples.iter().map(|s| {
            [
                s.left,
                s.right,
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
                Sample(0),
            ]
        }));
        wav::write_buses(
            &output.join(format!("{name}-rust-autonomous-mix.wav")),
            &frames,
        )?;
        let passed = independent_exact && errors.iter().all(|&e| e == 0);
        let report = serde_json::json!({"name":name,"scope":"Original primary VA voice before FXD03; single/dual filters, Drive and eleven WS types; observed controls and initial actors","frames":samples.len(),"controls":plan.events.len(),"stage_errors":errors,"autonomous_audio_exact":independent_exact,"passed":passed});
        fs::write(
            output.join(format!("{name}-voice-va-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Primary VA composed voice failed".into());
        }
    }
    Ok(())
}
