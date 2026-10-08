//! Independent unchanged SH3 damper input/release/pending flag observations.
use radias_synth_domain::{
    sustain::{SustainProgram, SustainState},
    voice_allocation::{AllocationOwner, VoiceClaim},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut errors = [0u32; 4];
    for (group, name, width, count) in [
        (0, "cc", 4, 32768),
        (1, "release", 10, 32768),
        (2, "pending", 97, 8192),
        (3, "mono-pending", 3, 32768),
    ] {
        let raw = fs::read(out.join(format!("sustain-{name}.bin")))?;
        if raw.len() != width * 4 * count {
            return Err(format!("Incomplete source sustain {name}").into());
        }
        for (index, row) in raw.chunks_exact(width * 4).enumerate() {
            let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
            let mismatch = match group {
                0 => {
                    let mut s = SustainState { flags: w(1) as u8 };
                    s.receive(w(0) as u8, w(2));
                    s.flags as u32 != w(3)
                }
                1 => {
                    let mut s = SustainState { flags: w(3) as u8 };
                    let p = SustainProgram { enabled: w(1) != 0 };
                    let mut claim = VoiceClaim {
                        owner: AllocationOwner(1),
                        note_flags: w(4) as u8,
                        release_flags: w(5) as u8,
                    };
                    let release = if w(0) != 0 {
                        s.poly_note_off(p, w(2) as u8, &mut claim)
                    } else {
                        s.mono_note_off(p, w(2) as u8)
                    };
                    [
                        release as u32,
                        s.flags as u32,
                        claim.note_flags as u32,
                        claim.release_flags as u32,
                    ] != [w(6), w(7), w(8), w(9)]
                }
                2 => {
                    let mut mask = 0;
                    let mut actual = [0; 48];
                    for slot in 0..24 {
                        let mut claim = VoiceClaim {
                            owner: AllocationOwner(slot as u32 + 1),
                            note_flags: w(slot * 2) as u8,
                            release_flags: w(slot * 2 + 1) as u8,
                        };
                        if SustainState::release_pending_poly(&mut claim) {
                            mask |= 1u32 << slot;
                        }
                        actual[slot * 2] = claim.note_flags as u32;
                        actual[slot * 2 + 1] = claim.release_flags as u32;
                    }
                    mask != w(48) || actual != core::array::from_fn::<_, 48, _>(|i| w(49 + i))
                }
                3 => {
                    let mut s = SustainState { flags: w(0) as u8 };
                    [s.release_pending_mono() as u32, s.flags as u32] != [w(1), w(2)]
                }
                _ => unreachable!(),
            };
            if mismatch {
                if errors[group] < 3 {
                    eprintln!("Sustain {name} row{index} differs");
                }
                errors[group] += 1;
            }
        }
    }
    let passed = errors == [0; 4];
    let report = serde_json::json!({"passed":passed,"errors":errors,"original_CC_input_cases":32768,"original_note_off_cases":32768,
        "original_poly_physical_scans":8192,"original_mono_pending_cases":32768,
        "original_source_tag_bits_and_value_bit6_preserved":true,"original_global_Poly_physical_scan_preserved":true,
        "source_host_DSP_release_calls_excluded_from_flag_boundary_fixture":true,"complete_native_engine":false});
    fs::write(
        out.join("sustain-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if !passed {
        return Err("Original sustain state differs".into());
    }
    println!("106496 original sustain input/release/pending cases match");
    Ok(())
}
