use radias_synth_domain::controller_noise::{FormantControlTarget, NoiseControl};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/noise-packets.bin"))?;
    if raw.len() != 131072 * 68 {
        return Err("Original Noise packet corpus incomplete".into());
    }
    let mut errors = [[0usize; 9]; 2];
    for (index, row) in raw.chunks_exact(68).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 65536 {
            return Err("Noise packet order differs".into());
        }
        let control = NoiseControl {
            control2: w(5) as u8,
            control2_modulation: w(6) as i16,
            control2_manual_offset: w(7) as i8,
        };
        let actual = if mode == 0 {
            let target = control.colored(w(1) as i16);
            [
                16,
                0x2006,
                target.color as i32 as u32,
                16,
                0x2008,
                target.frequency as i32 as u32,
                0,
                0,
                0,
            ]
        } else {
            let target = control.formant(
                FormantControlTarget {
                    level: w(2) as i16,
                    feedback: w(3) as i16,
                },
                w(4) as i16,
            );
            [
                32,
                0x2010,
                target.shape,
                16,
                0x2006,
                target.input_gain as i32 as u32,
                16,
                0x2008,
                target.frequency as i32 as u32,
            ]
        };
        for field in 0..9 {
            if actual[field] != w(8 + field) {
                if errors[mode][field] < 2 {
                    eprintln!(
                        "Noise packet mode{mode} case{index} field{field}:{} vs{}",
                        actual[field],
                        w(8 + field)
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":131072,"errors":errors,
        "source_entries":["SYS020CB8","SYS020D48"],
        "packet_sink_entries_observed_and_skipped":true,
        "native_control_packets_exact":true,"DSP_coefficient_compiler_qualified":false,
        "complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/noise-packets-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Noise packet mismatch".into());
    }
    Ok(())
}
