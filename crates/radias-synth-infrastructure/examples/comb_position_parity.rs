use radias_synth_domain::comb::read_position;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/comb-position.bin"))?;
    if raw.len() != 65536 * 24 {
        return Err("Original Comb position corpus incomplete".into());
    }
    let mut errors = [0usize; 3];
    for (index, row) in raw.chunks_exact(24).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let p = read_position(w(0), w(1) as u16, w(2) as u8);
        let a = [
            p.current as u32,
            p.previous as u32,
            p.fraction as u16 as u32,
        ];
        for n in 0..3 {
            if a[n] != w(3 + n) {
                if errors[n] < 2 {
                    eprintln!("Comb position{index} field{n}:{} vs{}", a[n], w(3 + n));
                }
                errors[n] += 1;
            }
        }
    }
    let passed = errors.iter().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":65536,"errors":errors,"source_ranges":["B7CC..B804","B843..B856"],"native_ring_address_and_fraction_implemented":true,"complete_comb_implemented":false});
    fs::write(
        root.join("runs/native-clone/comb-position-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Comb position mismatch".into());
    }
    Ok(())
}
