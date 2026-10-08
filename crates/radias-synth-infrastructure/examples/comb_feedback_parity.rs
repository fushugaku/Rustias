use radias_synth_domain::{
    Sample,
    comb::{CombFeedback, CombFeedbackInput, CombFeedbackState},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/comb-feedback.bin"))?;
    if raw.len() != 65536 * 48 {
        return Err("Original Comb corpus incomplete".into());
    }
    let mut errors = [0usize; 4];
    for (index, row) in raw.chunks_exact(48).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mut filter = CombFeedback {
            state: CombFeedbackState {
                interpolated: w(5) as i32,
                previous_drive: w(6) as i32,
                dc_blocked: w(7) as i32,
            },
        };
        let output = filter.prepare(CombFeedbackInput {
            sample: Sample(w(0) as i32),
            feedback: w(1) as i32,
            delay_samples: [w(2) as i16, w(3) as i16],
            fraction: w(4) as i16,
        });
        let actual = [
            output.0,
            filter.state.interpolated,
            filter.state.previous_drive,
            filter.state.dc_blocked,
        ];
        for n in 0..4 {
            if actual[n] != w(n + 8) as i32 {
                if errors[n] < 2 {
                    eprintln!(
                        "Comb case{index} field{n}:{} vs{}",
                        actual[n],
                        w(n + 8) as i32
                    );
                }
                errors[n] += 1;
            }
        }
    }
    let passed = errors.iter().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":65536,"source_range":"MasterB73C..B7A4","errors":errors,"native_feedback_interpolation_dc_block_implemented":true,"delay_memory_and_complete_comb_implemented":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/comb-feedback-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Comb feedback mismatch".into());
    }
    Ok(())
}
