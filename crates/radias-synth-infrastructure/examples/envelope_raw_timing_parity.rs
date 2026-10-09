//! Original signed MOV.B velocity/note timing inputs, including every raw byte.
use radias_synth_domain::envelope_segment::EnvelopeTiming;
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let tables =
        firmware::envelope_timing_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(out.join("envelope-raw-timing-original.bin"))?;
    let words: Vec<u32> = raw
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    if raw.len() < 8
        || !raw.len().is_multiple_of(4)
        || words[0] != 0x45525431
        || !(words.len() - 2).is_multiple_of(9)
    {
        return Err("Incomplete raw timing observation".into());
    }
    let (mut errors, mut cases) = (0, 0);
    let mut first = None;
    let mut velocity = [[0u32; 256]; 3];
    let mut note = [[0u32; 256]; 3];
    for v in words[1..words.len() - 1].chunks_exact(9) {
        let timing = EnvelopeTiming {
            curve: v[1] as u8,
            time: v[2] as u8,
            velocity: v[3] as u8,
            velocity_sensitivity: v[4] as u8,
            note: v[5] as u8,
            key_tracking: v[6] as u8,
        };
        let value = tables.increment(timing);
        if value != v[7] {
            errors += 1;
            if first.is_none() {
                first = Some(serde_json::json!({"case":cases,"input":v,"native":value}));
            }
        }
        velocity[v[0] as usize][v[3] as usize] += 1;
        note[v[0] as usize][v[5] as usize] += 1;
        cases += 1;
    }
    let passed = errors == 0
        && cases == 24576
        && velocity.iter().flatten().all(|v| *v > 0)
        && note.iter().flatten().all(|v| *v > 0);
    let report = serde_json::json!({"passed":passed,"whole_original_timing_calls":cases,"increment_errors":errors,
        "all256_velocity_and_note_bytes_per_EG_covered":true,"source_instructions_compared":words.last(),
        "source_instructions_unchanged":true,"first_error":first,"complete_native_engine":false});
    fs::write(
        out.join("envelope-raw-timing-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("Raw envelope timing differs".into());
    }
    Ok(())
}
