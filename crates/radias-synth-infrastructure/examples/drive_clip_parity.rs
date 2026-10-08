use radias_synth_domain::{
    Sample,
    waveshaper::{Drive, DriveCoefficients, DriveState, hard_clip},
};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/drive-clip.bin"))?;
    if raw.len() != 65536 * 84 {
        return Err("Incomplete original Drive/Hard Clip corpus".into());
    }
    let mut errors = [[0usize; 6]; 2];
    for (index, raw) in raw.chunks_exact(84).enumerate() {
        let w = |n: usize| u32::from_le_bytes(raw[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 {
            return Err("Original shaper corpus order differs".into());
        }
        let input = Sample(w(1) as i32);
        let p = |n: usize| w(2 + n) as i16;
        let mut drive = Drive {
            state: DriveState {
                previous_scaled_input: w(11) as i32,
                previous_output: w(12) as i32,
            },
        };
        let (output, gain) = if mode == 0 {
            let output = drive.sample(
                input,
                DriveCoefficients {
                    depth: p(0),
                    normalization: p(1),
                    feedback_gain: p(3),
                    threshold: p(4),
                    curves: [p(5), p(6)],
                },
            );
            (output.sample, output.gain_target)
        } else {
            (hard_clip(input, p(0)), p(2))
        };
        let actual = [
            output.0,
            drive.state.previous_scaled_input,
            drive.state.previous_output,
            w(13) as i32,
            w(14) as i32,
            gain as i32,
        ];
        let expected = [
            w(15) as i32,
            w(16) as i32,
            w(17) as i32,
            w(18) as i32,
            w(19) as i32,
            w(20) as i16 as i32,
        ];
        for j in 0..actual.len() {
            if actual[j] != expected[j] {
                if errors[mode][j] < 2 {
                    eprintln!(
                        "Mode{mode} case{index} field{j}:{} vs {}",
                        actual[j], expected[j]
                    );
                }
                errors[mode][j] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_transition_cases":65536,
        "errors":errors,"source_ranges":["Master CB90..CBE0","Master CCB4..CCCF"],
        "original_master_modified":false,"production_voice_connected":true,
        "complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/drive-clip-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Drive/Hard Clip mismatch".into());
    }
    Ok(())
}
