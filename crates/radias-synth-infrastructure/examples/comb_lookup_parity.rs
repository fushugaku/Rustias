use radias_synth_infrastructure::firmware::comb_control_tables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/comb-lookup.bin"))?;
    if raw.len() != 131072 * 12 {
        return Err("Original Comb lookup corpus incomplete".into());
    }
    let tables = comb_control_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = [0usize; 2];
    for (index, row) in raw.chunks_exact(12).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 65536 {
            return Err("Original Comb lookup order differs".into());
        }
        let actual = if mode == 0 {
            tables.delay(w(1) as i32)
        } else {
            tables.feedback(w(1) as i32)
        };
        if actual != w(2) {
            if errors[mode] < 2 {
                eprintln!("Comb lookup{index}:{actual} vs{}", w(2));
            }
            errors[mode] += 1;
        }
    }
    let passed = errors == [0, 0];
    let report = serde_json::json!({"passed":passed,"original_calls":131072,"errors":errors,"source_entries":["SYS01BFFC","SYS01C87C"],"original_tables_used":true,"complete_comb_controller_compilation":false});
    fs::write(
        root.join("runs/native-clone/comb-lookup-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Comb lookup mismatch".into());
    }
    Ok(())
}
