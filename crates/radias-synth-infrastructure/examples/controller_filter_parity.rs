use radias_synth_domain::controller_filter::ControllerFilter;
use radias_synth_infrastructure::firmware::{amplifier_tables, controller_filter_tables};
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[4 * n..4 * n + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = controller_filter_tables(&sys)?;
    let amp = amplifier_tables(&sys)?;
    let lookup = fs::read(root.join("runs/native-clone/controller-filter-lookup.bin"))?;
    let composed = fs::read(root.join("runs/native-clone/controller-filter.bin"))?;
    if lookup.len() != 65536 * 8 || composed.len() != 32768 * 68 {
        return Err("Incomplete controller filter corpus".into());
    }
    let mut lookup_errors = 0;
    let mut key_errors = 0;
    let mut cutoff_errors = 0;
    for r in lookup.chunks_exact(8) {
        if tables.frequency(word(r, 0) as i32) != word(r, 1) {
            lookup_errors += 1;
        }
    }
    for (n, r) in composed.chunks_exact(68).enumerate() {
        let p = ControllerFilter {
            cutoff: word(r, 0) as u8,
            cutoff_offset: word(r, 1) as i16,
            lfo_offset: word(r, 2) as i16,
            key_tracking: word(r, 3) as u8,
            key_manual_offset: word(r, 4) as i8,
            key_modulation: word(r, 5) as i16,
            relative_pitch: word(r, 6) as i16,
            eg1_intensity: word(r, 7) as u8,
            eg1_manual_offset: word(r, 8) as i8,
            eg1_depth_modulation: word(r, 9) as i16,
            eg1_level: word(r, 10) as u16,
            velocity: word(r, 11) as u8,
            eg1_velocity_sensitivity: word(r, 12) as u8,
            additional_offset: word(r, 13) as i16,
            cutoff_modulation: word(r, 14) as i16,
        };
        if p.key_offset(&tables) as u16 as u32 != word(r, 15) {
            key_errors += 1;
        }
        let actual = p.frequency(&tables, &amp);
        if actual != word(r, 16) {
            if cutoff_errors < 3 {
                eprintln!("Cutoff {n}: {actual} != {}, {p:?}", word(r, 16));
            }
            cutoff_errors += 1;
        }
    }
    let passed = lookup_errors == 0 && key_errors == 0 && cutoff_errors == 0;
    let report = serde_json::json!({"passed":passed,"lookup_cases":65536,"whole_cutoff_compiler_cases":32768,
        "lookup_errors":lookup_errors,"key_errors":key_errors,"cutoff_errors":cutoff_errors,
        "original_sh3_calls_complete":true,"original_instructions_modified":false,"computed_EG1_velocity_depth":true,
        "live_filter_audio_qualified":false,"complete_engine":false});
    fs::write(
        root.join("runs/native-clone/controller-filter-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native controller filter differs".into());
    }
    Ok(())
}
