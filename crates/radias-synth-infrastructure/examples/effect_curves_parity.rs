//! Original complete scalar curve calls compared to native integer compilation.
use radias_synth_domain::effect_curves::EffectCurve;
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::json;
use std::{collections::BTreeSet, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let raw = fs::read(root.join("runs/native-clone/effect-curves-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated original scalar records".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x45464331 || !(words.len() - 1).is_multiple_of(7) {
        return Err("Invalid original scalar records".into());
    }
    let curves = [
        EffectCurve::Linear,
        EffectCurve::Quadratic,
        EffectCurve::EaseOut,
        EffectCurve::InverseScale,
        EffectCurve::Select,
        EffectCurve::OffsetScale,
        EffectCurve::OffsetQuadratic,
    ];
    let (mut records, mut errors) = (0u32, 0u32);
    let mut first = serde_json::Value::Null;
    let mut seen = BTreeSet::new();
    for row in words[1..].chunks_exact(7) {
        let candidate = library
            .parameter_range(row[1] as u8)?
            .compile(
                curves[row[0] as usize],
                row[3] as i32,
                row[4] as i32,
                row[5] as i32,
            )
            .ok_or("Curve rejected original input")? as u32;
        if candidate != row[6] {
            errors += 1;
            if first.is_null() {
                first = json!({"curve":row[0],"range":row[1],"profile":row[2],"value":row[3] as i32,"native":candidate,"original":row[6]});
            }
        }
        seen.insert((row[0], row[1], row[2], row[3] as i32));
        records += 1;
    }
    let mut expected = BTreeSet::new();
    for range in [17u8, 31, 63, 76, 81, 82] {
        let domain = library.parameter_range(range)?;
        for curve in 0..7u32 {
            if curve >= 5 && domain.maximum as u8 == 0 {
                continue;
            }
            for profile in 0..8u32 {
                for value in i32::from(domain.minimum)..=i32::from(domain.maximum) {
                    expected.insert((curve, u32::from(range), profile, value));
                }
            }
        }
    }
    let passed = errors == 0 && seen == expected && records as usize == expected.len();
    let report = json!({"passed":passed,"original_scalar_calls":records,"errors":errors,"first_difference":first,
        "whole_parameter_domains":6,"integer_curve_functions":7,"endpoint_profiles":8,
        "original_range_bytes_are_declared_inputs":true,"original_curve_outputs_used_as_native_inputs":false,
        "native_controller_executes_firmware_instructions":false,"FXD03_audio_arithmetic_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-curves-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native effect parameter curves: {records} original calls, {errors} differences");
    if !passed {
        return Err("Native effect parameter curves differ".into());
    }
    Ok(())
}
