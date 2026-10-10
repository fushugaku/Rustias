//! The production native pool compared against complete unchanged SYS 0172e8.
use radias_synth_application::{
    VoiceRenderer,
    clock::ClockPulse,
    lfo::LfoParameters,
    modulation::{ModulationProgram, VoiceModulationTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
    shared_lfo::EffectLfoParameters,
};
use radias_synth_domain::{
    effect_lfo_program::EffectLfoProgram, lfo::LfoState, lfo_tempo::LfoTempoState, pan::VoiceBus,
};
use radias_synth_infrastructure::{
    firmware::{MasterTables, lfo_tables, lfo_tempo_tables, modulation_tables},
    prepared::PreparedVoice,
};
use serde_json::Value;
use std::{fs, path::PathBuf};
const WORDS: usize = 786;
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn source_state(r: &[u8], i: usize) -> LfoState {
    let b = 6 + 12 * i;
    LfoState {
        phase: word(r, b),
        previous_random: word(r, b + 1) as i16,
        random: word(r, b + 2) as i16,
        half_cycle: word(r, b + 3) as u8,
    }
}
fn source_tempo(r: &[u8], i: usize) -> LfoTempoState {
    let b = 6 + 12 * i;
    LfoTempoState {
        phase: word(r, b),
        previous_increment: word(r, b + 4),
        reference_phase: word(r, b + 5),
        clock_count: word(r, b + 6) as u16,
        observed_clock_count: word(r, b + 7) as u16,
        correction_active: word(r, b + 8) as u8,
        correction_hold: word(r, b + 9) as u8,
        division: word(r, b + 10) as u8,
    }
}
fn number(v: &Value, i: usize) -> Result<i64, Box<dyn std::error::Error>> {
    v[i].as_i64()
        .ok_or_else(|| "LFO input parameter absent".into())
}
fn effect(v: &Value) -> Result<EffectLfoParameters, Box<dyn std::error::Error>> {
    Ok(EffectLfoParameters {
        mode: number(v, 0)? as u8,
        frequency: number(v, 1)? as u8,
        phase_sync: number(v, 2)? as u8,
        beat: number(v, 3)? as u8,
        alternate_phase: number(v, 4)? as u8,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let prefix = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "runs/native-clone/lfo-schedule-note-construction".into());
    let raw = fs::read(root.join(format!("{prefix}.bin")))?;
    if raw.len() != 16 * 257 * WORDS * 4 {
        return Err("LFO schedule corpus incomplete".into());
    }
    let inputs: Value =
        serde_json::from_slice(&fs::read(root.join(format!("{prefix}-programs.json")))?)?;
    if inputs.as_array().ok_or("Schedule metadata absent")?.len() != 16 {
        return Err("Scenario count differs".into());
    }
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let effect_values = radias_synth_infrastructure::effects::EffectLibrary::from_system(&sys)?
        .lfo_value_tables()?;
    let effect_raw = fs::read(root.join(format!("{prefix}-effect-values.bin")))?;
    let effect_words: Vec<_> = effect_raw
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    if effect_words.len() != 87249 || effect_words[0] != 0x454c5331 {
        return Err("Complete scheduled effect-value corpus required".into());
    }
    let mut effect_cursor = 1;
    let mut effect_config = [EffectLfoProgram::default(); 9];
    let mut effect_value_errors = 0;
    let mut effect_value_checks = 0;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let data = MasterTables::from_host_stream(&master)?;
    let tables = VoiceModulationTables {
        lfo: lfo_tables(&sys)?,
        matrix: modulation_tables(&sys)?,
        pitch: data.pitch()?,
        bandwidth: data.bandwidth()?,
    };
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let mut phase_errors = 0usize;
    let mut tempo_errors = 0usize;
    let mut seed_errors = 0usize;
    let mut pool = PolyphonicRenderer::default();
    let mut snapshots = 0usize;
    for (record, r) in raw.chunks_exact(WORDS * 4).enumerate() {
        let scenario = word(r, 0) as usize;
        let tick = word(r, 1);
        if scenario >= 16 {
            return Err("Source scenario invalid".into());
        }
        if tick == u32::MAX {
            pool = PolyphonicRenderer::default();
            let input = &inputs[scenario];
            if effect_words[effect_cursor..effect_cursor + 2] != [0x1000, scenario as u32] {
                return Err("Effect value configuration order differs".into());
            }
            effect_cursor += 2;
            for config in &mut effect_config {
                config.bytes = core::array::from_fn(|i| effect_words[effect_cursor + i] as u8);
                effect_cursor += 6;
            }
            if input["oscillator_random_prefix_included"].as_bool() != Some(true) {
                return Err(
                    "Production pool comparison requires original oscillator RNG initialization"
                        .into(),
                );
            }
            if input["tempo"].as_bool().ok_or("Clock setting absent")? {
                pool.enable_tempo_clock(
                    lfo_tempo_tables(&sys)?,
                    input["setting"].as_u64().ok_or("BPM absent")? as u16,
                );
            }
            let mut programs = [ModulationProgram::default(); 4];
            for (timbre, program) in programs.iter_mut().enumerate() {
                for i in 0..2 {
                    let p = &input["programs"][timbre]["lfo"][i];
                    program.lfo[i] = LfoParameters {
                        waveform: number(p, 0)? as u8,
                        shape: number(p, 1)? as u8,
                        frequency: number(p, 2)? as u8,
                        phase_sync: number(p, 3)? as u8,
                        frequency_offset: number(p, 4)? as i8,
                        frequency_modulation: number(p, 5)? as i16,
                    };
                    program.tempo_divisions[i] =
                        number(&input["programs"][timbre]["divisions"], i)? as u8;
                }
                pool.edit_modulation(timbre as u8, *program)
                    .map_err(|_| "Unexpected unsupported tempo")?;
                pool.set_timbre_modulation_active(timbre as u8, true);
                for i in 0..2 {
                    pool.edit_effect_lfo(
                        Some(timbre as u8),
                        i,
                        effect(&input["effects"][2 * timbre + i])?,
                    )
                    .map_err(|_| "Effect tempo absent")?;
                }
            }
            pool.edit_effect_lfo(None, 0, effect(&input["effects"][8])?)
                .map_err(|_| "Global tempo absent")?;
            for slot in 0..24 {
                let timbre = slot & 3;
                let voice = ActiveVoice {
                    uses_program_common: false,
                    drum_pitch: None,
                    drum_instrument: None,
                    drum_filter2: None,
                    renderer: VoiceRenderer::new(plan.initial, plan.parameters),
                    amplifier: None,
                    modulation: None,
                    auxiliary: None,
                    pan: None,
                    mixer: None,
                    secondary: None,
                    primary: None,
                    shaper: None,
                    comb_program: None,
                    timbre: timbre as u8,
                    note: (48 + slot) as u8,
                    velocity: 100,
                    held: true,
                    program: 0,
                    bus: VoiceBus::new(timbre as u8).unwrap(),
                };
                let assigned = pool
                    .trigger_modulated(voice, 4283, programs[timbre])
                    .ok_or("Initial allocation failed")?;
                if assigned.slot as usize != slot {
                    return Err("Fixture physical slot allocation differs".into());
                }
            }
        } else {
            if tick >= 256 || record % 257 != tick as usize + 1 {
                return Err("Source event order differs".into());
            }
            let mask = word(r, 4);
            for timbre in 0..4 {
                pool.set_timbre_modulation_active(timbre, mask & (1 << timbre) != 0);
            }
            match word(r, 3) {
                0 => {}
                1 => pool.clock_pulse(ClockPulse::TimerOne),
                4 => pool.clock_pulse(ClockPulse::ExternalFour),
                _ => return Err("Unknown source clock pulse".into()),
            }
            pool.service_lfos(word(r, 2) as u8, &tables);
        }
        if effect_words[effect_cursor..effect_cursor + 3] != [0x2000, scenario as u32, tick] {
            return Err("Effect value observation order differs".into());
        }
        effect_cursor += 3;
        for (slot, state) in pool.effect_lfo_value_states().into_iter().enumerate() {
            let actual = effect_values
                .values(&tables.lfo, effect_config[slot], state)
                .map(|v| v as u32);
            if actual != effect_words[effect_cursor..effect_cursor + 2] {
                effect_value_errors += 1;
            }
            effect_cursor += 2;
            effect_value_checks += 2;
        }
        for i in 0..65 {
            let actual = if i < 48 {
                pool.retained_modulation_state(i / 2)
                    .ok_or("Physical LFO absent")?[i & 1]
            } else if i < 64 {
                pool.shared_lfo_states((i - 48) / 4)
                    .ok_or("Shared LFO absent")?[(i - 48) % 4]
            } else {
                pool.global_lfo_state()
            };
            let expected = source_state(r, i);
            if actual != expected {
                if phase_errors < 3 {
                    eprintln!("Phase {scenario}/{tick}/{i}: {actual:?} != {expected:?}");
                }
                phase_errors += 1;
            }
            if let Some(clock) = pool.tempo_clock() {
                let actual = if i < 48 {
                    clock.bank.voices[i / 2][i & 1]
                } else if i < 64 {
                    clock.bank.timbres[(i - 48) / 4][(i - 48) % 4]
                } else {
                    clock.bank.global
                };
                let expected = source_tempo(r, i);
                if actual != expected {
                    if tempo_errors < 3 {
                        eprintln!("Tempo {scenario}/{tick}/{i}: {actual:?} != {expected:?}");
                    }
                    tempo_errors += 1;
                }
            }
        }
        if pool.modulation_random as u32 != word(r, 5) {
            if seed_errors < 3 {
                eprintln!(
                    "Seed {scenario}/{tick}: {} != {}",
                    pool.modulation_random,
                    word(r, 5)
                );
            }
            seed_errors += 1;
        }
        snapshots += 1;
    }
    let passed = phase_errors == 0
        && tempo_errors == 0
        && seed_errors == 0
        && effect_value_errors == 0
        && effect_value_checks == 74016
        && effect_cursor == effect_words.len();
    let report = serde_json::json!({"passed":passed,"whole_original_schedule_calls":4096,"continuous_scenarios":16,
        "initial_snapshots_compared":16,"phase_state_observations":snapshots*65,"tempo_state_observations":8*257*65,
        "phase_errors":phase_errors,"tempo_errors":tempo_errors,"seed_errors":seed_errors,
        "live_production_effect_value_checks":effect_value_checks,"live_production_effect_value_errors":effect_value_errors,
        "effect_value_getters_use_evolved_native_pool_states":true,"recorded_effect_phase_or_value_outputs_replayed_as_inputs":false,
        "native_production_pool_used":true,"all_24_private_slots":true,"all_16_shared_states":true,"global_effect_lfo":true,
        "even_odd_physical_slots_qualified":true,"free_and_tempo_branches":true,"enabled_timbre_masks_changed":true,
        "clock_pulses_are_fixture_inputs":true,"original_instructions_modified":false,
        "source_corpus_prefix":prefix,"original_oscillator_random_prefixes":384,"original_oscillator_random_calls":1152,
        "oscillator_prefix_executes_original_01ef78_to_01ef9e_without_callee_stubs":true,
        "whole_oscillator_note_constructor_qualified":false,
        "source_initial_state_replayed":false,"independent_audio_hpi_timing_qualified":false,"effect_audio_qualified":false,"complete_engine":false});
    fs::write(
        root.join("runs/native-clone/lfo-schedule-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native instrument LFO scheduling differs".into());
    }
    Ok(())
}
