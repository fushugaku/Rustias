use radias_synth_domain::controller_noise::{formant_control1, noise_control1};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/noise-control.bin"))?;
    if raw.len() != 131072 * 16 {
        return Err("Original Noise CTRL1 corpus incomplete".into());
    }
    let mut errors = [[0usize; 2]; 2];
    for (index, row) in raw.chunks_exact(16).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 65536 {
            return Err("Noise CTRL1 order differs".into());
        }
        let actual = if mode == 0 {
            [noise_control1(w(1) as i32) as u16 as u32, 0]
        } else {
            let target = formant_control1(w(1) as i32);
            [target.level as u16 as u32, target.feedback as u16 as u32]
        };
        for field in 0..2 {
            if actual[field] != w(2 + field) {
                if errors[mode][field] < 2 {
                    eprintln!(
                        "Noise CTRL1 mode{mode} case{index} field{field}:{} vs{}",
                        actual[field],
                        w(2 + field)
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":131072,"errors":errors,"source_entries":["SYS0207B2","SYS020880"],"native_control1_shadow_targets_exact":true,"control2_and_DSP_coefficient_compiler_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/noise-control-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Noise CTRL1 mismatch".into());
    }
    Ok(())
}
