//! Original SH3 rates, reverse-phase glide and per-note initialization.
use radias_synth_domain::portamento::{PortamentoProgram, PortamentoState};
use radias_synth_infrastructure::firmware;
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let rates = firmware::portamento_rates(&sys)?;
    let curves = firmware::portamento_curves(&sys)?;
    let out = root.join("runs/native-clone");
    let mut report = serde_json::Map::new();
    let mut total_errors = 0;
    for (family, width, count) in [
        ("rates", 6, 32768),
        ("steps", 9, 65536),
        ("notes", 12, 32768),
    ] {
        let raw = fs::read(out.join(format!("portamento-{family}.bin")))?;
        if raw.len() != width * 4 * count {
            return Err("Original portamento corpus truncated".into());
        }
        let mut errors = 0;
        for (index, row) in raw.chunks_exact(width * 4).enumerate() {
            let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
            let (actual, expected): (Vec<u32>, Vec<u32>) = match family {
                "rates" => {
                    let p = PortamentoProgram {
                        time: w(0) as u8,
                        switch_required: w(1) != 0,
                        ..Default::default()
                    };
                    (
                        vec![p.rate(&rates, w(2) != 0, w(3) as i16, w(4) as i8)],
                        vec![w(5)],
                    )
                }
                "steps" => {
                    let mut s = PortamentoState {
                        phase: w(1),
                        rate: w(2),
                        start_q16: w(3) as i32,
                        current_q16: w(4) as i32,
                    };
                    s.advance(&curves, w(0) as u8);
                    (
                        vec![s.phase, s.rate, s.start_q16 as u32, s.current_q16 as u32],
                        vec![w(5), w(6), w(7), w(8)],
                    )
                }
                "notes" => {
                    let p = PortamentoProgram {
                        time: w(0) as u8,
                        ..Default::default()
                    };
                    let mut s = PortamentoState::default();
                    s.begin(
                        p.rate(&rates, false, 0, 0),
                        w(1) as u8,
                        w(4) as i32,
                        if w(2) != 0 || w(3) != 0 {
                            Some(w(5) as i32)
                        } else {
                            None
                        },
                    );
                    (
                        vec![
                            s.phase,
                            s.rate,
                            s.start_q16 as u32,
                            s.current_q16 as u32,
                            s.assigned_note_q16(w(1) as u8) as u32,
                            s.relative_pitch_word(w(1) as u8) as u32,
                        ],
                        (6..12).map(w).collect(),
                    )
                }
                _ => unreachable!(),
            };
            if actual != expected {
                if errors < 3 {
                    eprintln!("{family} {index}: {actual:?} vs {expected:?}");
                }
                errors += 1;
            }
        }
        total_errors += errors;
        report.insert(family.into(), json!({"cases":count,"errors":errors}));
    }
    report.insert("passed".into(), json!(total_errors == 0));
    report.insert("errors".into(), json!(total_errors));
    report.insert("live_controller_connected".into(), json!(false));
    report.insert("full_audio_qualified".into(), json!(false));
    report.insert("complete_native_engine".into(), json!(false));
    fs::write(
        out.join("portamento-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if total_errors != 0 {
        return Err("Original portamento arithmetic differs".into());
    }
    println!("131072 original portamento rate/step/note cases match");
    Ok(())
}
