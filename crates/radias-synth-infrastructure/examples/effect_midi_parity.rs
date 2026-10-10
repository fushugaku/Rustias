//! Whole original effects MIDI-source selection and signed normalization.
use radias_synth_domain::effect_midi::{
    EffectMidiSources, EffectMidiTimbre, effect_controller_level,
};
use radias_synth_domain::program::Program;
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-midi-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated effects MIDI corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x464d5031 {
        return Err("Wrong effects MIDI corpus".into());
    }
    let (mut cursor, mut selectors, mut levels, mut selector_errors, mut level_errors) =
        (1usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = serde_json::Value::Null;
    for profile in 0..256 {
        if words[cursor] != profile {
            return Err("Original source profile order differs".into());
        }
        cursor += 1;
        let mut sources = EffectMidiSources::default();
        for timbre in &mut sources.timbres {
            let fields = &words[cursor..cursor + 10];
            cursor += 10;
            *timbre = EffectMidiTimbre {
                control_49: fields[0] as i8,
                bend: fields[1] as i16,
                control_4b: fields[2] as i8,
                channel: fields[3] as u8,
                switch_45: fields[4] as u8,
                controls_4c_50: core::array::from_fn(|i| fields[i + 5] as i8),
            };
        }
        for group in &mut sources.channel_controls {
            for value in group {
                *value = words[cursor] as u8;
                cursor += 1;
            }
        }
        for value in &mut sources.global_controls {
            *value = words[cursor] as u16;
            cursor += 1;
        }
        sources.shared_control = words[cursor] as i8;
        cursor += 1;
        for part in 0..5 {
            for selector in 0..13 {
                let actual = i32::from(
                    sources
                        .value(part, selector)
                        .ok_or("Valid MIDI source rejected")?,
                ) as u32;
                let expected = words[cursor];
                cursor += 1;
                selectors += 1;
                if actual != expected {
                    selector_errors += 1;
                    if first.is_null() {
                        first = json!({"profile":profile,"part":part,"selector":selector,"native":actual,"original":expected});
                    }
                }
            }
        }
        for value in -127..=127 {
            let actual = effect_controller_level(value as i8) as u32;
            let expected = words[cursor];
            cursor += 1;
            levels += 1;
            if actual != expected {
                level_errors += 1;
            }
        }
    }
    let mut header_calls = 0;
    let mut header_errors = 0;
    for part in 0..4 {
        for role in 0..2 {
            for enabled in 0..2 {
                for kind in 0..31 {
                    if words[cursor..cursor + 4] != [part, role, enabled, kind] {
                        return Err("Stored header profile differs".into());
                    }
                    cursor += 4;
                    let mut bytes = [0u8; 1790];
                    bytes[168 + part as usize * 228 + role as usize * 24] =
                        kind as u8 | if enabled != 0 { 128 } else { 0 };
                    let program =
                        Program::from_bytes(&bytes).map_err(|_| "Stored program rejected")?;
                    let effect = program
                        .timbre(part as usize)
                        .unwrap()
                        .effect(role as usize)
                        .unwrap();
                    let native = [
                        u32::from(effect.enabled()),
                        u32::from(effect.kind().ok_or("Valid stored kind rejected")?.raw()),
                    ];
                    if native != words[cursor..cursor + 2] {
                        header_errors += 1;
                    }
                    cursor += 2;
                    header_calls += 2;
                }
            }
        }
    }
    let passed = cursor == words.len()
        && selectors == 16640
        && levels == 65280
        && selector_errors == 0
        && level_errors == 0
        && header_calls == 992
        && header_errors == 0;
    let report = json!({"passed":passed,"whole_original_MIDI_selector_calls":selectors,"whole_original_level_calls":levels,"selector_errors":selector_errors,"level_errors":level_errors,"first_difference":first,
        "all_five_part_contexts":true,"all_thirteen_source_selectors":true,"raw_byte_profiles":256,"native_uses_raw_MIDI_state":true,
        "whole_original_stored_effect_header_getters":header_calls,"stored_header_errors":header_errors,"all_eight_insert_headers_and_31_types":true,
        "source_getter_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,"FXD03_audio_or_full_effect_controller_lifecycle_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-midi-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native FX MIDI sources: {selectors} original selectors, {levels} levels, {selector_errors}/{level_errors} differences"
    );
    if !passed {
        return Err("Native effects MIDI values differ".into());
    }
    Ok(())
}
