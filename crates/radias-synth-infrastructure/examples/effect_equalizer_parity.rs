//! Complete original peaking/shelf coefficient calls, without recorded targets.
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&source)?.equalizer_tables()?;
    let raw = fs::read(root.join("runs/native-clone/effect-equalizer-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated EQ corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x45514231 || !(words.len() - 1).is_multiple_of(12) {
        return Err("Wrong EQ corpus".into());
    }
    let (mut cases, mut errors) = (0usize, 0usize);
    let mut counts = [0usize; 3];
    let mut mode_errors = [0usize; 3];
    let mut first = Value::Null;
    for record in words[1..].chunks_exact(12) {
        let mode = record[0] as usize;
        let frequency = record[1] as u8;
        let q = record[2] as u8;
        let gain = record[3] as i8;
        let mut native = [0u32; 8];
        if mode == 0 {
            native = tables
                .peaking(frequency, q, gain)
                .ok_or("Native peaking rejected source inputs")?;
        } else {
            let result = if mode == 1 {
                tables.low_shelf(frequency, gain)
            } else {
                tables.high_shelf(frequency, gain)
            }
            .ok_or("Native shelf rejected source inputs")?;
            native[..3].copy_from_slice(&result);
        }
        if native != record[4..] {
            errors += 1;
            mode_errors[mode] += 1;
            if first.is_null() {
                first = json!({"mode":mode,"frequency":frequency,"q":q,"gain":gain,"native":native,"original":&record[4..]});
            }
        }
        cases += 1;
        counts[mode] += 1;
    }
    let passed = cases == 422086 && counts == [413472, 4307, 4307] && errors == 0;
    let report = json!({"passed":passed,"whole_original_coefficient_calls":cases,"mode_counts":counts,"mode_errors":mode_errors,"errors":errors,"first_difference":first,"all_59_frequencies_96_Q_values_and_73_gain_values":true,"coefficient_tables_are_declared_inputs":true,"original_instruction_bodies_or_callees_modified":false,"coefficient_outputs_used_as_native_inputs":false,"native_executes_firmware_instructions":false,"complete_parameter_dispatch_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-equalizer-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native EQ core: {cases} whole original calls, {errors} differences");
    if !passed {
        return Err("Native EQ core differs".into());
    }
    Ok(())
}
