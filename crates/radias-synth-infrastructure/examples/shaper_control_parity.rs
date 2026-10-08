use radias_synth_domain::controller_shaper::{ShaperControl, WaveshaperType};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let all_ws = std::env::args().nth(2).as_deref() == Some("--all-ws");
    let filename = if all_ws {
        "all-ws-control"
    } else {
        "shaper-control"
    };
    let modes = if all_ws { 12 } else { 2 };
    let raw = fs::read(root.join(format!("runs/native-clone/{filename}.bin")))?;
    if raw.len() != modes * 32768 * 24 {
        return Err("Incomplete original shaper control corpus".into());
    }
    let mut errors = vec![0usize; modes];
    for (index, row) in raw.chunks_exact(24).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 || w(5) != 84 {
            return Err("Original shaper mode/target differs".into());
        }
        let control = ShaperControl {
            depth: w(1) as u8,
            manual_offset: w(2) as i16,
            modulation: w(3) as i16,
        };
        let actual = if mode == 0 {
            control.drive_depth()
        } else if !all_ws {
            control.hard_clip_depth()
        } else {
            control.waveshaper_depth(
                WaveshaperType::from_raw(mode as u8 - 1).ok_or("WS type invalid")?,
            )
        };
        if actual as u16 as u32 != w(4) {
            if errors[mode] < 3 {
                eprintln!("Shaper mode{mode} case{index}:{actual} vs{}", w(4));
            }
            errors[mode] += 1;
        }
    }
    let passed = errors.iter().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"errors":errors,"original_calls":modes*32768,
        "source_ranges":["SYS002DE4..002E3E","SYS002E70..002FD8"],
        "delivery_stop":"before original HPI00F2A0","original_instructions_modified":false,
        "complete_native_engine":false});
    fs::write(
        root.join(format!("runs/native-clone/{filename}-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native shaper control mismatch".into());
    }
    Ok(())
}
