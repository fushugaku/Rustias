use radias_synth_domain::controller_pan::PanControl;
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[4 * n..4 * n + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/pan-controller.bin"))?;
    let table = radias_synth_infrastructure::firmware::pan_tables(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    if raw.len() != 65536 * 32 {
        return Err("Incomplete original pan corpus".into());
    }
    let mut errors = 0;
    let mut transfer_errors = 0;
    for (n, r) in raw.chunks_exact(32).enumerate() {
        let control = PanControl {
            position: word(r, 0) as u8,
            manual_offset: word(r, 1) as i16,
            modulation: word(r, 2) as i16,
            timbre_offset: word(r, 3) as i8,
            midi_pan: (word(r, 4) != 0).then_some(word(r, 5) as u8),
        };
        let actual = control.target();
        if actual as u32 != word(r, 6) {
            if errors < 3 {
                eprintln!("Pan {n}: {actual} != {}, {control:?}", word(r, 6));
            }
            errors += 1;
        }
        if table.compile(actual) as u32 != word(r, 7) {
            transfer_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0&&transfer_errors==0,"original_sh3_calls":65536,"errors":errors,
        "original_controller_to_hpi_compiler_cases":65536,"transfer_errors":transfer_errors,
        "original_instructions_modified":false,"live_pan_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/pan-controller-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 || transfer_errors != 0 {
        return Err("Original pan compiler differs".into());
    }
    Ok(())
}
