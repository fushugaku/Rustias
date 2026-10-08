use radias_synth_infrastructure::{firmware, rdl};
use std::{fs, path::PathBuf};
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let tables = firmware::amplifier_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(root.join("runs/native-clone/amplifier-key.bin"))?;
    if raw.len() != 65536 * 12 {
        return Err("Incomplete amplifier key corpus".into());
    }
    let mut errors = 0;
    for (n, r) in raw.chunks_exact(12).enumerate() {
        let actual = tables.key_modulation(word(r, 0) as u8, word(r, 1) as i16) as u16 as u32;
        if actual != word(r, 2) {
            if errors < 3 {
                eprintln!("AMP key {n}: {actual} != {}", word(r, 2));
            }
            errors += 1;
        }
    }
    let programs = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut bindings = 0;
    for program in programs {
        for index in 0..4 {
            let timbre = program.timbre(index).unwrap();
            let c = radias_synth_application::program::TimbreControls::from_timbre(timbre)
                .map_err(|_| "Invalid controls")?;
            if c.amplifier(0x7f00, None, 0).key_tracking != timbre.synthesis()[0x32] {
                return Err("AMP key binding differs".into());
            }
            bindings += 1;
        }
    }
    let passed = errors == 0;
    let report = serde_json::json!({"passed":passed,"original_complete_key_cases":65536,"errors":errors,
        "stored_key_bindings":bindings,"original_instructions_modified":false,"full_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/amplifier-key-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native amplifier key differs".into());
    }
    Ok(())
}
