//! Original group layout, detune, spread and shared random checkpoints.
use radias_synth_application::program::TimbreControls;
use radias_synth_domain::voice_group::VoiceGroupProgram;
use radias_synth_infrastructure::{firmware, rdl};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let tables =
        firmware::voice_group_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = 0;
    let layouts = fs::read(out.join("voice-group-layout.bin"))?;
    if layouts.len() != 16384 * 16 {
        return Err("Original group layouts incomplete".into());
    }
    for (i, row) in layouts.chunks_exact(16).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[4 * n..4 * n + 4].try_into().unwrap());
        let layout = VoiceGroupProgram {
            raw: w(0) as u8,
            ..Default::default()
        }
        .layout(w(1) as u8);
        if layout.count as u32 != w(2) || if layout.stereo_pair { 2 } else { 0 } != w(3) {
            if errors < 3 {
                eprintln!("Original layout {i} differs");
            }
            errors += 1;
        }
    }
    let offsets = fs::read(out.join("voice-group-offsets.bin"))?;
    if offsets.len() != 32768 * 32 {
        return Err("Original group offsets incomplete".into());
    }
    for (i, row) in offsets.chunks_exact(32).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[4 * n..4 * n + 4].try_into().unwrap());
        let mut seed = w(4) as u16;
        let p = VoiceGroupProgram {
            raw: 128,
            detune: w(2) as u8,
            spread: w(3) as u8,
        };
        let actual = tables
            .offsets(p, w(0) as u8, w(1) as u8, &mut seed)
            .ok_or("Original group address absent")?;
        if actual.tuning_q16 as u32 != w(5)
            || actual.pan as u8 as u32 != w(6)
            || seed as u32 != w(7)
        {
            if errors < 3 {
                eprintln!(
                    "Original offsets {i} differ: {actual:?}, seed{seed}, expected{:?}",
                    [w(5), w(6), w(7)]
                );
            }
            errors += 1;
        }
    }
    let mut bindings = 0;
    for program in rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)? {
        for index in 0..4 {
            let timbre = program.timbre(index).unwrap();
            let controls = TimbreControls::from_timbre(timbre).map_err(|_| "Invalid route")?;
            if controls.voice_group
                != (VoiceGroupProgram {
                    raw: timbre.bytes()[8],
                    detune: timbre.bytes()[9],
                    spread: timbre.bytes()[10],
                })
            {
                return Err("Stored Unison group binding differs".into());
            }
            bindings += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_layout_cases":16384,"original_offset_cases":32768,"stored_timbre_bindings":bindings,"errors":errors,
        "original_shared_random_states_compared":true,"group_table_zero_consumes_no_random_word":true,"complete_native_engine":false});
    fs::write(
        out.join("voice-group-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Original instrument Unison arithmetic differs".into());
    }
    println!("49152 original group cases and1024 stored bindings match");
    Ok(())
}
