use radias_synth_domain::{noise_control::formant_frequency, pitch::PitchCode};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&image)?;
    let pitch_table = tables.pitch()?;
    let noise = tables.noise_pitch()?;
    let raw = fs::read(root.join("runs/native-clone/noise-pitch.bin"))?;
    if raw.len() != 65536 * 20 {
        return Err("Original Noise pitch corpus incomplete".into());
    }
    let mut errors = [[0usize; 2]; 2];
    for (index, row) in raw.chunks_exact(20).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 {
            return Err("Noise pitch order differs".into());
        }
        let code = PitchCode::new(w(1) as u16).ok_or("Invalid original pitch")?;
        let increment = pitch_table.increment(code);
        let actual = [
            increment.0,
            if mode == 0 {
                noise.curve_scale(code) as u16 as u32
            } else {
                formant_frequency(increment) as u16 as u32
            },
        ];
        let expected = [w(2), w(if mode == 0 { 3 } else { 4 })];
        for field in 0..2 {
            if actual[field] != expected[field] {
                if errors[mode][field] < 3 {
                    eprintln!(
                        "Noise pitch mode{mode} code{} field{field}:{} vs{}",
                        code.raw(),
                        actual[field],
                        expected[field]
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&count| count == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":65536,"errors":errors,
        "source_entries":["MasterDBDF","MasterDCAC"],"noise_pitch_scratch_input_is_declared_code":true,
        "native_primary_phase_increment_and_dependent_coefficients_exact":true,
        "full_controller_lifecycle_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/noise-pitch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Noise pitch mismatch".into());
    }
    Ok(())
}
