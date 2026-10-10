use radias_synth_domain::program::Program;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-controller-fields-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated controller field corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x46434631 || !(words.len() - 1).is_multiple_of(8) {
        return Err("Wrong controller field corpus".into());
    }
    let (mut calls, mut errors) = (0usize, 0usize);
    let mut first = Value::Null;
    for record in words[1..].chunks_exact(8) {
        let part = record[0] as usize;
        let role = record[1] as usize;
        let mut bytes = [0u8; 1790];
        let offset = 168 + 228 * part + 24 * role;
        for (i, v) in record[2..6].iter().enumerate() {
            bytes[offset + i] = *v as u8;
        }
        let native = Program::from_bytes(&bytes)
            .map_err(|_| "Declared program rejected")?
            .timbre(part)
            .ok_or("Invalid timbre")?
            .effect(role)
            .ok_or("Invalid insert")?
            .controllers()
            .map(u32::from);
        if native != record[6..] {
            errors += 1;
            if first.is_null() {
                first = json!({"part":part,"role":role,"raw":&record[2..6],"native":native,"original":&record[6..]});
            }
        }
        calls += 2;
    }
    let passed = calls == 4096 && errors == 0;
    let report = json!({"passed":passed,"whole_original_stored_controller_getters":calls,"errors":errors,"first_difference":first,"all_eight_insert_slots_and_full_byte_profiles":true,"raw_program_bytes_are_declared_inputs":true,"source_getter_results_replayed_as_inputs":false,"FXD03_audio_or_full_effect_lifecycle_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-controller-fields-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native FX controller fields: {calls} original getters, {errors} differences");
    if !passed {
        return Err("Native controller fields differ".into());
    }
    Ok(())
}
