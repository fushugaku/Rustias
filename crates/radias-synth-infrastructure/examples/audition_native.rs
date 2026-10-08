use radias_synth_application::VoiceRenderer;
use radias_synth_domain::{Sample, pan::StereoFrame};
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice, wav};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    for label in ["pulse", "triangle", "sine"] {
        let raw = fs::read(root.join(format!(
            "runs/native-clone/live-va-{label}-voice-va-inputs.bin"
        )))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        let mut samples = vec![StereoFrame::default(); 48000 * 4];
        for (index, block) in samples.chunks_mut(128).enumerate() {
            if index == 1125 {
                renderer.release();
            }
            renderer.render(&table, &plan.events, block);
        }
        let frames: Vec<_> = samples
            .into_iter()
            .map(|s| {
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
            })
            .collect();
        wav::write_buses(
            &root.join(format!("runs/native-clone/audition-{label}-q31.wav")),
            &frames,
        )?;
    }
    Ok(())
}
