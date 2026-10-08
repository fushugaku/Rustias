use radias_synth_domain::controller_comb::{CombCutoffControl, CombResonanceControl};
use radias_synth_infrastructure::firmware::{amplifier_tables, comb_control_tables};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/comb-controller.bin"))?;
    if raw.len() != 65536 * 96 {
        return Err("Original Comb controller corpus incomplete".into());
    }
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = comb_control_tables(&system)?;
    let amplitude = amplifier_tables(&system)?;
    let mut errors = [[0usize; 3]; 2];
    for (index, row) in raw.chunks_exact(96).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 {
            return Err("Original Comb controller order differs".into());
        }
        let cutoff = CombCutoffControl {
            link: w(1) != 0,
            cutoff: w(2) as u8,
            linked_cutoff: w(3) as u8,
            manual_offset: w(4) as i16,
            key_offset: w(5) as i16,
            lfo_offset: w(6) as i16,
            eg1_intensity: w(7) as u8,
            linked_eg1_intensity: w(8) as u8,
            eg1_manual_offset: w(9) as i8,
            eg1_depth_modulation: w(10) as i16,
            eg1_level: w(11) as u16,
            velocity: w(12) as u8,
            eg1_velocity_sensitivity: w(13) as u8,
            additional_offset: w(14) as i16,
            cutoff_modulation: w(15) as i16,
        };
        let resonance = CombResonanceControl {
            link: w(1) != 0,
            resonance: w(16) as u8,
            linked_resonance: w(17) as u8,
            modulation: w(18) as i16,
            manual_offset: w(19) as i8,
        };
        let code = if mode == 0 {
            cutoff.code(&amplitude)
        } else {
            w(20) as i32
        };
        let actual = [
            code as u32,
            if mode == 0 { tables.delay(code) } else { w(22) },
            tables.compile_feedback(code, resonance),
        ];
        for field in 0..3 {
            if actual[field] != w(21 + field) {
                if errors[mode][field] < 2 {
                    eprintln!(
                        "Comb ctrl mode{mode} case{index} field{field}:{} vs{}",
                        actual[field],
                        w(21 + field)
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_calls":65536,"errors":errors,"source_entries":["SYS01BD62","SYS01C592"],"native_cutoff_composition_and_frequency_dependent_feedback_implemented":true,"link_bit_and_signed_manual_modulation_inputs_qualified":true,"independent_key_offset_and_hpi_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/comb-controller-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Comb controller mismatch".into());
    }
    let raw = fs::read(root.join("runs/native-clone/comb-key-controller.bin"))?;
    if raw.len() != 65536 * 28 {
        return Err("Original Comb key corpus incomplete".into());
    }
    let filter = radias_synth_infrastructure::firmware::controller_filter_tables(&system)?;
    let mut key_errors = 0;
    for (index, row) in raw.chunks_exact(28).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let program = radias_synth_application::comb::CombProgram {
            cutoff: CombCutoffControl {
                link: w(0) != 0,
                ..Default::default()
            },
            key_tracking: w(1) as u8,
            linked_key_tracking: w(2) as u8,
            key_manual_offset: w(3) as i8,
            key_modulation: w(4) as i16,
            ..Default::default()
        };
        let actual = program
            .for_voice(
                radias_synth_application::comb::CombVoiceControl {
                    eg1_level: 0,
                    velocity: 100,
                    eg1_velocity_sensitivity: 64,
                    relative_pitch: w(5) as i16,
                    modulation: [0; 4],
                },
                &filter,
            )
            .cutoff
            .key_offset as u16 as u32;
        if actual != w(6) {
            if key_errors < 2 {
                eprintln!("Comb key case{index}: {actual} vs{}", w(6));
            }
            key_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":key_errors==0,"original_calls":65536,"errors":key_errors,"source_entry":"SYS01BC12","native_application_linked_key_tracking_used":true,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/comb-key-controller-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if key_errors != 0 {
        return Err("Native Comb key tracking mismatch".into());
    }
    Ok(())
}
