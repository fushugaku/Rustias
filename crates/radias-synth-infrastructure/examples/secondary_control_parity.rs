use radias_synth_domain::{
    Phase, Sample, pitch::PhaseIncrement, secondary_control::SecondaryModulation,
};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository path required")?);
    let raw = fs::read(root.join("runs/native-clone/original-secondary-control.bin"))?;
    if raw.len() != 32768 * 40 {
        return Err("Incomplete secondary modulation corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(40).enumerate() {
        let actual = SecondaryModulation {
            ring: word(r, 5) != 0,
            sync: word(r, 6) != 0,
        }
        .prepare(
            Phase(word(r, 0)),
            Phase(word(r, 1)),
            Phase(word(r, 2)),
            PhaseIncrement(word(r, 3)),
            Sample(word(r, 4) as i32),
        );
        let a = [
            actual.previous_primary.0,
            actual.phase.0,
            actual.gain as u16 as u32,
        ];
        let e = [word(r, 7), word(r, 8), word(r, 9)];
        if a != e {
            if errors < 3 {
                eprintln!("Secondary {i}: {a:?} != {e:?}");
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"secondary_control_cases":32768,"errors":errors});
    fs::write(
        root.join("runs/native-clone/secondary-control-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Secondary Ring/Sync control parity failed".into());
    }
    Ok(())
}
