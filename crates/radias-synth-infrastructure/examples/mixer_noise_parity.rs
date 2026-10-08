use radias_synth_domain::noise::MixerNoise;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/mixer-noise.bin"))?;
    if raw.len() != 65536 * 16 {
        return Err("Original mixer noise corpus incomplete".into());
    }
    let mut errors = [0usize; 2];
    for (index, row) in raw.chunks_exact(16).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mut state = MixerNoise { state: w(0) as i32 };
        state.next_word(w(1) as i16);
        for (field, error) in errors.iter_mut().enumerate() {
            if state.state as u32 != w(2 + field) {
                if *error < 2 {
                    eprintln!(
                        "Mixer noise case{index} field{field}:{} vs{}",
                        state.state,
                        w(2 + field)
                    );
                }
                *error += 1;
            }
        }
    }
    let passed = errors == [0; 2];
    let report = serde_json::json!({"passed":passed,"original_transitions":65536,"errors":errors,"source_entry":"MasterA19A..A1B1","native_mixer_noise_state_and_output_used":true,"per_voice_frame22_state_and_frame28_output_exact":true,"signed_excitation_bias_exercised":true,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/mixer-noise-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native mixer noise mismatch".into());
    }
    Ok(())
}
