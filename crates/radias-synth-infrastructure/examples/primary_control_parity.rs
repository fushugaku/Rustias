use radias_synth_domain::controller_primary::{PrimaryControl, PrimaryTarget};
use std::{fs, path::PathBuf};
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let data = fs::read(root.join("runs/native-clone/primary-control.bin"))?;
    if data.len() != 65536 * 60 {
        return Err("Incomplete primary control corpus".into());
    }
    let mut composition_errors = 0;
    let mut target_errors = 0;
    for (n, r) in data.chunks_exact(60).enumerate() {
        let p = PrimaryControl {
            control1: word(r, 1) as u8,
            control2: word(r, 2) as u8,
            control1_manual_offset: word(r, 3) as i16,
            control1_modulation: word(r, 4) as i16,
            control2_modulation: word(r, 5) as i16,
            control2_manual_offset: word(r, 6) as i8,
            lfo1: word(r, 7) as i16,
        };
        let state = p.compose();
        if [state.base as u32, state.curved as u32, state.linear as u32]
            != [word(r, 8), word(r, 9), word(r, 10)]
        {
            if composition_errors < 3 {
                eprintln!(
                    "Primary compose {n}: {p:?} -> {state:?} vs {:?}",
                    [word(r, 8), word(r, 9), word(r, 10)]
                );
            }
            composition_errors += 1;
        }
        let (i, value) = match state
            .target(word(r, 0) as u8)
            .ok_or("Unsupported primary selector")?
        {
            PrimaryTarget::Waveform(v) => (0, v),
            PrimaryTarget::Cross(v) => (1, v),
            PrimaryTarget::Unison(v) => (2, v),
            PrimaryTarget::Vpm(v) => (3, v),
        };
        let actual: [u32; 4] =
            core::array::from_fn(|j| if i == j { value as u16 as u32 } else { 0x5a5a });
        let expected: [u32; 4] = core::array::from_fn(|j| word(r, 11 + j));
        if actual != expected {
            if target_errors < 3 {
                eprintln!("Primary target {n}: {actual:?} vs {expected:?}");
            }
            target_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":composition_errors==0&&target_errors==0,"whole_original_calls":65536,
        "selection_modes":16,"composition_errors":composition_errors,"target_errors":target_errors,
        "original_instructions_modified":false,"native_dsp_coefficients_qualified":false,"native_live_primary_controls_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/primary-control-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if composition_errors != 0 || target_errors != 0 {
        return Err("Primary control differs".into());
    }
    Ok(())
}
