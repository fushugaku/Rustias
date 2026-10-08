use radias_synth_application::VoiceRenderer;
use radias_synth_domain::{filter::FilterCoefficients, fixed::saturate, pan::StereoFrame};
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/live-control-edit-voice-va-inputs.bin"))?;
    let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
    let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
    renderer.control_slew(plan.control_slew, (plan.reference_start_frame & 3) as u8);
    let mut previous = None;
    let mut changes = 0;
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(704).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
        let p = |n: usize| word(n + 1) as i16;
        let q = |n: usize| ((word(n + 1) << 16) | word(n + 2)) as i32;
        let target = FilterCoefficients {
            input_gain: p(54),
            feedback: q(56),
            integrator_gain: q(64),
            post_gain: p(68),
            post_feedback: p(70),
            mix: [p(72), p(74), p(76), p(78), p(80)],
        };
        if previous != Some(target) {
            renderer.set_filter(target);
            previous = Some(target);
            changes += 1;
        }
        let mut frame = [StereoFrame::default()];
        renderer.render(&table, &plan.events, &mut frame);
        let expected = [
            saturate((word(169) as i32 as i64) << 5),
            saturate((word(170) as i32 as i64) << 5),
        ];
        if [frame[0].left.0, frame[0].right.0] != expected {
            if errors < 3 {
                eprintln!("Control slew {i}: {:?} != {expected:?}", frame[0]);
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"scope":"Original live Filter1 balance/cutoff controls, derived native 4-frame interpolation from targets","frames":raw.len()/704,"target_changes":changes,"errors":errors,"current_filter_coefficients_taken_from_reference":false,"passed":errors==0});
    println!("{report}");
    fs::write(
        root.join("runs/native-clone/control-slew-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Original control slew audio parity failed".into());
    }
    Ok(())
}
