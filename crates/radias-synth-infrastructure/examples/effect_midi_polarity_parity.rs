use radias_synth_domain::effect_midi::EffectMidiPolarity;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-midi-polarity-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated polarity corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x46504c31 || words.len() != 1 + 256 * 18 {
        return Err("Wrong polarity corpus".into());
    }
    let (mut calls, mut errors) = (0usize, 0usize);
    let mut first = Value::Null;
    for (profile, record) in words[1..].chunks_exact(18).enumerate() {
        let assignments = core::array::from_fn(|i| record[i] as u8);
        let polarity = EffectMidiPolarity { assignments };
        for selector in 0..13 {
            let native = u32::from(polarity.bipolar(selector));
            let original = record[5 + usize::from(selector)];
            if native != original {
                errors += 1;
                if first.is_null() {
                    first = json!({"profile":profile,"assignments":assignments,"selector":selector,"native":native,"original":original});
                }
            }
            calls += 1;
        }
    }
    let passed = calls == 3328 && errors == 0;
    let report = json!({"passed":passed,"whole_original_polarity_selectors":calls,"errors":errors,"first_difference":first,"all_13_selectors_and_256_raw_global_byte_profiles":true,"raw_global_assignment_bytes_are_declared_inputs":true,"configured_assignment_codes_are_not_clamped":true,"original_getter_outputs_used_as_inputs":false,"FXD03_audio_or_full_MIDI_effect_lifecycle_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-midi-polarity-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native FX MIDI polarity: {calls} original selectors, {errors} differences");
    if !passed {
        return Err("Native MIDI polarity differs".into());
    }
    Ok(())
}
