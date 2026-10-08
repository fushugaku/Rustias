use radias_synth_domain::controller_secondary::SecondaryPitch;
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[4 * n..4 * n + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let table = radias_synth_infrastructure::firmware::fine_tune_table(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let lookup = fs::read(root.join("runs/native-clone/secondary-pitch-lookup.bin"))?;
    let raw = fs::read(root.join("runs/native-clone/secondary-pitch.bin"))?;
    if lookup.len() != 65536 * 8 || raw.len() != 32768 * 24 {
        return Err("Incomplete secondary pitch corpus".into());
    }
    let mut lookup_errors = 0;
    let mut compose_errors = 0;
    for r in lookup.chunks_exact(8) {
        if table.lookup(word(r, 0) as i32) as u32 != word(r, 1) {
            lookup_errors += 1;
        }
    }
    for (n, r) in raw.chunks_exact(24).enumerate() {
        let control = SecondaryPitch {
            semitone: word(r, 0) as u8,
            fine_tune: word(r, 1) as u8,
            semitone_manual_offset: word(r, 2) as i16,
            fine_manual_offset: word(r, 3) as i16,
            virtual_patch_q16: word(r, 4) as i32,
        };
        if control.relative_code(&table) as u16 as u32 != word(r, 5) {
            if compose_errors < 3 {
                eprintln!("Pitch {n}: {:?} != {}", control, word(r, 5));
            }
            compose_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":lookup_errors==0&&compose_errors==0,"whole_lookup_cases":65536,"whole_secondary_pitch_cases":32768,"lookup_errors":lookup_errors,"compose_errors":compose_errors,"original_instructions_modified":false,"live_secondary_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/secondary-pitch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if lookup_errors != 0 || compose_errors != 0 {
        return Err("Secondary pitch differs".into());
    }
    Ok(())
}
