//! Direct Rust pitch arithmetic compared with immutable original SH3 execution.
use radias_synth_application::program::TimbreControls;
use radias_synth_domain::note_pitch::{BasePitch, PitchProgram, ScaleContext, normalize_bend};
use radias_synth_infrastructure::{firmware, rdl};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = firmware::note_pitch_tables(&system)?;
    let out = root.join("runs/native-clone");
    let mut report = serde_json::Map::new();
    let mut total_errors = 0usize;
    for (family, width, expected_count) in [
        ("notes", 5, 16384),
        ("bends", 5, 65536),
        ("scales", 7, 0),
        ("tuning", 5, 32768),
        ("bases", 8, 32768),
        ("vibrato", 4, 65536),
    ] {
        let raw = fs::read(out.join(format!("note-pitch-{family}.bin")))?;
        let bytes = width * 4;
        if !raw.len().is_multiple_of(bytes)
            || (expected_count != 0 && raw.len() != expected_count * bytes)
            || raw.is_empty()
        {
            return Err(format!("Original {family} corpus incomplete").into());
        }
        let mut errors = 0;
        for (index, row) in raw.chunks_exact(bytes).enumerate() {
            let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
            let (actual, expected): (Vec<u32>, Vec<u32>) = match family {
                "notes" => {
                    let program = PitchProgram {
                        transpose: w(1) as u8,
                        ..Default::default()
                    };
                    let note = program
                        .initialize(w(0) as u8, ScaleContext::default(), &tables, &mut 0)
                        .ok_or("Invalid note input")?;
                    (
                        vec![
                            note.wrapped as u32,
                            note.clamped_q8 as u32,
                            note.scale_q16 as u32,
                        ],
                        vec![w(2), w(3), w(4)],
                    )
                }
                "bends" => {
                    let bend = normalize_bend(w(0) as u16);
                    let p = PitchProgram {
                        bend_range: w(1) as u8,
                        bend_enabled: w(2) != 0,
                        ..Default::default()
                    };
                    (
                        vec![bend as u16 as u32, p.bend_q16(bend) as u32],
                        vec![w(3), w(4)],
                    )
                }
                "scales" => {
                    let mut seed = w(4) as u16;
                    let context = ScaleContext {
                        selection: w(1) as u8,
                        global_transpose: if w(2) != 0 { Some(w(3) as i8) } else { None },
                        custom_cents: core::array::from_fn(|i| (i as i32 * 19 - 100) as i8),
                    };
                    let value = tables
                        .scale_offset(w(0) as u8, context, &mut seed)
                        .ok_or("Invalid scale input")?;
                    (vec![value as u32, seed as u32], vec![w(5), w(6)])
                }
                "tuning" => {
                    let p = PitchProgram {
                        fine_tune: w(0) as u8,
                        ..Default::default()
                    };
                    (
                        vec![p.tuning_q16(&tables, w(1) as i32, w(2) as i32, w(3) as i16) as u32],
                        vec![w(4)],
                    )
                }
                "bases" => {
                    let pitch = BasePitch {
                        assigned_note_q16: w(0) as i32,
                        scale_q16: w(1) as i32,
                        bend_q16: w(2) as i32,
                        tuning_q16: w(3) as i32,
                        manual_offset: w(4) as i16,
                        drum_transpose: if w(6) != 0 { Some(w(5) as u8) } else { None },
                    };
                    (vec![pitch.q16() as u32], vec![w(7)])
                }
                "vibrato" => {
                    let p = PitchProgram {
                        vibrato_intensity: w(0) as u8,
                        wheel_enabled: w(2) != 0,
                        ..Default::default()
                    };
                    (
                        vec![p.vibrato_depth(w(1) as u8, &tables.vibrato) as u32],
                        vec![w(3)],
                    )
                }
                _ => unreachable!(),
            };
            if actual != expected {
                if errors < 3 {
                    eprintln!("{family} case {index}: {actual:?} vs {expected:?}");
                }
                errors += 1;
            }
        }
        total_errors += errors;
        report.insert(
            family.into(),
            json!({"cases":raw.len()/bytes,"errors":errors}),
        );
    }
    let bank = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut bindings = 0;
    for program in &bank {
        for index in 0..4 {
            let timbre = program.timbre(index).unwrap();
            let controls = TimbreControls::from_timbre(timbre).map_err(|_| "Invalid route")?;
            let p = timbre.synthesis();
            let common = timbre.bytes();
            let expected = PitchProgram {
                transpose: p[0x13],
                fine_tune: p[0x14],
                vibrato_intensity: p[0x15],
                bend_range: common[0xb],
                bend_enabled: common[5] & 0x80 != 0,
                wheel_enabled: common[5] & 0x10 != 0,
            };
            if controls.pitch != expected {
                return Err("Stored pitch binding differs".into());
            }
            bindings += 1;
        }
    }
    report.insert("stored_timbre_bindings".into(), json!(bindings));
    report.insert("passed".into(), json!(total_errors == 0));
    report.insert("errors".into(), json!(total_errors));
    report.insert(
        "independent_midi_delivery_clock_qualified".into(),
        json!(false),
    );
    report.insert("complete_native_engine".into(), json!(false));
    fs::write(
        out.join("note-pitch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if total_errors != 0 {
        return Err("Original pitch arithmetic differs".into());
    }
    println!("All original pitch cases and {bindings} stored timbre bindings match");
    Ok(())
}
