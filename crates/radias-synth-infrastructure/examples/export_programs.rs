use radias_synth_infrastructure::prepared::PreparedVoice;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let output = root.join("assets/native-va");
    fs::create_dir_all(&output)?;
    for (label, name, extended) in [
        ("saw", "control-sweep", false),
        ("pulse", "live-va-pulse", true),
        ("triangle", "live-va-triangle", true),
        ("sine", "live-va-sine", true),
        ("cross", "live-cross", true),
        ("unison", "live-unison", true),
        ("vpm", "live-vpm", true),
    ] {
        let suffix = if extended {
            "voice-va-inputs"
        } else {
            "voice-inputs"
        };
        let raw = fs::read(root.join(format!("runs/native-clone/{name}-{suffix}.bin")))?;
        let original = if extended {
            PreparedVoice::from_reference_va_parameters(&raw)?
        } else {
            PreparedVoice::from_reference_parameters(&raw)?
        };
        let json = PreparedVoice::export_program(&raw, extended)?;
        let loaded = PreparedVoice::from_program_json(&json)?;
        if original.initial != loaded.initial
            || original.parameters != loaded.parameters
            || original.events != loaded.events
        {
            return Err("Compiled native program changed voice/state/control events".into());
        }
        fs::write(output.join(format!("{label}.json")), &json)?;
        println!(
            "{{\"program\":\"{label}\",\"bytes\":{},\"state_and_controls_exact\":true,\"recorded_audio_present\":false}}",
            json.len()
        );
    }
    fs::copy(
        root.join("runs/native-clone/control-sweep-controls.json"),
        output.join("filter-controls.json"),
    )?;
    Ok(())
}
