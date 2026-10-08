use radias_synth_domain::noise::{
    ColoredNoiseParameters, FormantParameters, FormantState, NoiseFilterState,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/noise-formant.bin"))?;
    if raw.len() != 65536 * 212 {
        return Err("Original Noise/Formant corpus incomplete".into());
    }
    let mut errors = [[0usize; 5]; 2];
    for (index, row) in raw.chunks_exact(212).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 {
            return Err("Noise corpus ordering differs".into());
        }
        let p = |n: usize| w(2 + n) as u16;
        let q = |n: usize| ((p(n) as u32) << 16) | p(n + 1) as u32;
        let phase = w(1).wrapping_sub(q(4));
        let mut filter = NoiseFilterState {
            first: q(if mode == 0 { 14 } else { 12 }) as i32,
            second: q(if mode == 0 { 16 } else { 14 }) as i32,
        };
        let mut counter = p(10) as i16;
        let sample = if mode == 0 {
            filter.colored(
                phase,
                ColoredNoiseParameters {
                    phase_gain: p(7) as i16,
                    seed_bias: p(9) as i16,
                    curve_scale: p(10) as i16,
                    phase_offset: p(11) as i16,
                    curve_limit: q(12) as i32,
                    feedback: p(18) as i16,
                    limits: [q(20) as i32, q(22) as i32],
                },
            )
        } else {
            let mut state = FormantState { counter, filter };
            let sample = state.sample(
                FormantParameters {
                    seed_gain: p(7) as i16,
                    seed_bias: p(9) as i16,
                    frequency: p(11) as i16,
                    input_gain: p(16) as i16,
                    feedback: p(17) as i16,
                    limits: [q(18) as i32, q(20) as i32],
                },
                w(26) as i16,
            );
            filter = state.filter;
            counter = state.counter;
            sample
        };
        let offset = if mode == 0 { 14 } else { 12 };
        let expected_pair = |n: usize| ((w(29 + n) << 16) | w(30 + n)) as i32;
        let actual = [
            sample.0,
            filter.first,
            filter.second,
            if mode == 0 { 0 } else { counter as u16 as u32 } as i32,
            phase as i32,
        ];
        let expected = [
            w(27) as i32,
            expected_pair(offset),
            expected_pair(offset + 2),
            if mode == 0 { 0 } else { w(39) } as i32,
            w(28) as i32,
        ];
        for field in 0..5 {
            if actual[field] != expected[field] {
                if errors[mode][field] < 3 {
                    eprintln!(
                        "Noise mode{mode} case{index} field{field}: {} vs {}",
                        actual[field], expected[field]
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_transitions":65536,"errors":errors,"source_generators":["MasterC0A8","MasterC1A8"],"native_noise_and_formant_arithmetic_used":true,"declared_initial_actor_tables_and_status_used":true,"formant_signed_excitation_bias_exercised":true,"native_controller_compilation_and_full_voice_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/noise-formant-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Noise/Formant transition mismatch".into());
    }
    Ok(())
}
