use radias_synth_domain::controller_mixer::MixerLevel;
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/controller-mixer.bin"))?;
    if raw.len() != 98304 * 28 {
        return Err("Incomplete mixer corpus".into());
    }
    let mut compose_errors = 0;
    let mut gain_errors = 0;
    for (n, r) in raw.chunks_exact(28).enumerate() {
        let control = MixerLevel {
            level: word(r, 1) as u8,
            manual_offset: word(r, 2) as i16,
            modulation: word(r, 3) as i16,
            scale: word(r, 4) as u16,
        };
        if control.composed() as u32 != word(r, 5) {
            compose_errors += 1;
        }
        if control.gain() as u16 as u32 != word(r, 6) {
            if gain_errors < 3 {
                eprintln!("Mixer {n}: {:?} != {}", control, word(r, 6));
            }
            gain_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":compose_errors==0&&gain_errors==0,"whole_level_cases":98304,"whole_controller_to_hpi_cases":98304,
        "compose_errors":compose_errors,"gain_errors":gain_errors,"original_instructions_modified":false,
        "audio_input_primary_branch_qualified":false,"live_mixer_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/controller-mixer-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if compose_errors != 0 || gain_errors != 0 {
        return Err("Mixer controller differs".into());
    }
    Ok(())
}
