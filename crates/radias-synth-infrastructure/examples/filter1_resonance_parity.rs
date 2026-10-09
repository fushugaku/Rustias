use radias_synth_domain::controller_comb::CombResonanceControl;
use radias_synth_infrastructure::{firmware, prepared::ControlMap};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let table = firmware::filter2_control_tables(&sys)?;
    let map = ControlMap::from_system(&sys)?;
    let raw = fs::read(out.join("filter1-original-resonance.bin"))?;
    if raw.len() != 65536 * 24 {
        return Err("Original Filter1 corpus incomplete".into());
    }
    let mut errors = 0;
    for row in raw.chunks_exact(24) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let c = CombResonanceControl {
            resonance: w(0) as u8,
            manual_offset: w(1) as i8,
            modulation: w(2) as i16,
            ..Default::default()
        };
        let (resonance, gain) = table.resonance_targets(w(3) as u8, c);
        if resonance as u32 != w(4) || gain as u16 as u32 != w(5) {
            errors += 1;
        }
    }
    let captured = ControlMap::from_json(&fs::read(
        root.join("assets/native-va/filter-controls.json"),
    )?)?;
    if map.frequencies != captured.frequencies || map.resonances != captured.resonances {
        return Err("Captured nominal frequency/resonance differed from ROM".into());
    }
    let corrected = (0..128)
        .filter(|&i| map.input_gains[i] != captured.input_gains[i])
        .count();
    if map.input_gains[0] != 32767 || map.input_gains[127] != 8191 || corrected == 0 {
        return Err("Filter1 input gains still use captured smoothing".into());
    }
    let passed = errors == 0;
    let report = serde_json::json!({"passed":passed,"original_complete_Filter1_calls":65536,"errors":errors,
        "static_frequency_and_resonance_ROM_targets":128,"warm_captured_input_gains_corrected":corrected,
        "original_subcalls_excluded":false,"original_instructions_modified":false,
        "production_static_control_map_uses_ROM":true,"complete_native_engine":false});
    fs::write(
        out.join("filter1-resonance-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Filter1 resonance/gain differs".into());
    }
    Ok(())
}
