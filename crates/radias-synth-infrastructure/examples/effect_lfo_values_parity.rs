//! Effect-specific waveform pairs compared to whole original SYS016A0C.
use radias_synth_domain::{
    effect_lfo_program::EffectLfoProgram, effect_lfo_values::EffectLfoValueState, lfo::LfoState,
};
use radias_synth_infrastructure::{effects::EffectLibrary, firmware::lfo_tables};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.lfo_value_tables()?;
    let lfo = lfo_tables(&source)?;
    let raw = fs::read(root.join("runs/native-clone/effect-lfo-values-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated value corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x454c5631 || !(words.len() - 1).is_multiple_of(17) {
        return Err("Wrong value corpus".into());
    }
    let (mut cases, mut errors) = (0usize, 0usize);
    let mut first = Value::Null;
    let mut by_mode = [0usize; 8];
    let mut by_slot = [0usize; 9];
    let mut mode_errors = [0usize; 8];
    let mut slot_errors = [0usize; 9];
    let mut first_by_mode = core::array::from_fn::<_, 8, _>(|_| Value::Null);
    for row in words[1..].chunks_exact(17) {
        let [
            profile,
            mode,
            shape,
            p,
            slot,
            phase,
            previous,
            current,
            alternate,
        ]: [u32; 9] = row[..9].try_into().unwrap();
        if profile >= 16
            || mode >= 8
            || shape >= 128
            || p >= 16
            || slot != (profile + mode + shape + p) % 9
        {
            return Err("Source value profile differs".into());
        }
        let program = EffectLfoProgram {
            bytes: core::array::from_fn(|i| row[9 + i] as u8),
        };
        let state = EffectLfoValueState {
            oscillator: LfoState {
                phase,
                previous_random: previous as i16,
                random: current as i16,
                half_cycle: 0,
            },
            alternate_phase: alternate as u8,
        };
        let actual = tables.values(&lfo, program, state).map(|v| v as u32);
        if actual != row[15..17] {
            errors += 1;
            mode_errors[mode as usize] += 1;
            slot_errors[slot as usize] += 1;
            if first_by_mode[mode as usize].is_null() {
                first_by_mode[mode as usize] = json!({"profile":profile,"mode":mode,"shape":shape,"phase":phase,"slot":slot,"native":actual,"original":&row[15..17]});
            }
            if first.is_null() {
                first = json!({"profile":profile,"mode":mode,"shape":shape,"phase":phase,"slot":slot,"native":actual,"original":&row[15..17]});
            }
        }
        cases += 1;
        by_mode[mode as usize] += 1;
        by_slot[slot as usize] += 1;
    }
    let passed =
        cases == 262144 && errors == 0 && by_mode == [32768; 8] && by_slot.iter().all(|v| *v > 0);
    let report = json!({"passed":passed,"whole_original_pair_getter_calls":cases,"values_compared":cases*2,"errors":errors,"first_difference":first,"mode_counts":by_mode,"slot_counts":by_slot,"mode_errors":mode_errors,"slot_errors":slot_errors,"first_by_mode":first_by_mode,
        "source_waveform_outputs_replayed_as_inputs":false,"phase_and_random_state_are_declared_inputs":true,"all_original_callees_execute_without_stubs":true,"effect_audio_or_complete_live_modulation_service_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-lfo-values-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native effect LFO pairs: {cases} original getters, {errors} differences");
    if !passed {
        return Err("Native effect LFO values differ".into());
    }
    Ok(())
}
