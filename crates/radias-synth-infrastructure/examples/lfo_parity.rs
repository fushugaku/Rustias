use radias_synth_domain::lfo::{LfoState, LfoWave};
use radias_synth_infrastructure::firmware::lfo_tables;
use std::{fs, path::PathBuf};
fn word(raw: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(raw[n * 4..n * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let output = root.join("runs/native-clone");
    let tables = lfo_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(output.join("lfo-waves.bin"))?;
    if raw.len() != 262144 * 24 {
        return Err("Incomplete original LFO waveform corpus".into());
    }
    let modes = [
        LfoWave::Saw,
        LfoWave::Pulse,
        LfoWave::BipolarPulse,
        LfoWave::Triangle,
        LfoWave::SampleHold,
        LfoWave::Sine,
        LfoWave::Zero,
        LfoWave::Zero,
    ];
    let mut errors = [0usize; 8];
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let mode = word(r, 0) as usize;
        let actual = tables.value(
            modes[mode],
            word(r, 1) as u16,
            word(r, 2) as i8,
            LfoState {
                phase: 0,
                previous_random: word(r, 3) as i16,
                random: word(r, 4) as i16,
                half_cycle: 0,
            },
        );
        let expected = word(r, 5) as i16;
        if actual != expected {
            if errors[mode] < 3 {
                eprintln!(
                    "LFO mode {mode}, case {i}, phase {}, shape {}: {actual} != {expected}",
                    word(r, 1),
                    word(r, 2) as i32
                );
            }
            errors[mode] += 1;
        }
    }
    let raw = fs::read(output.join("lfo-phases.bin"))?;
    if raw.len() != 65536 * 48 {
        return Err("Incomplete LFO phase corpus".into());
    }
    let mut phase_errors = 0;
    for (i, r) in raw.chunks_exact(48).enumerate() {
        let mut state = LfoState {
            phase: word(r, 0),
            previous_random: word(r, 4) as i16,
            random: word(r, 5) as i16,
            half_cycle: 0,
        };
        let mut seed = word(r, 6) as u16;
        state.advance(
            &tables,
            word(r, 3) as u8,
            word(r, 1),
            word(r, 2) as i32,
            &mut seed,
        );
        let actual = [
            state.phase,
            state.previous_random as u16 as u32,
            state.random as u16 as u32,
            state.half_cycle as u32,
            seed as u32,
        ];
        let expected = core::array::from_fn::<_, 5, _>(|n| word(r, n + 7));
        if actual != expected {
            if phase_errors < 3 {
                eprintln!("LFO phase {i}: {actual:?} != {expected:?}");
            }
            phase_errors += 1;
        }
    }
    let raw = fs::read(output.join("lfo-offsets.bin"))?;
    if raw.len() != 256 * 8 {
        return Err("Incomplete original phase-offset corpus".into());
    }
    let offset_errors = raw
        .chunks_exact(8)
        .filter(|r| tables.phase_offset(word(r, 0) as u8) as u32 != word(r, 1))
        .count();
    let raw = fs::read(output.join("lfo-random.bin"))?;
    if raw.len() != 65536 * 12 {
        return Err("Incomplete random-state corpus".into());
    }
    let random_errors = raw
        .chunks_exact(12)
        .filter(|r| {
            let mut seed = word(r, 0) as u16;
            let value = LfoState::next_random(&mut seed) as u16 as u32;
            value != word(r, 1) || seed as u32 != word(r, 2)
        })
        .count();
    let raw = fs::read(output.join("lfo-frequencies.bin"))?;
    if raw.len() != 32768 * 12 {
        return Err("Incomplete LFO frequency corpus".into());
    }
    let mut frequency_errors = 0;
    for (i, r) in raw.chunks_exact(12).enumerate() {
        let actual = tables.modulated_frequency(word(r, 0), word(r, 1) as i32);
        if actual != word(r, 2) {
            if frequency_errors < 3 {
                eprintln!("LFO frequency {i}: {actual} != {}", word(r, 2));
            }
            frequency_errors += 1;
        }
    }
    let raw = fs::read(output.join("lfo-initialize.bin"))?;
    if raw.len() != 65536 * 60 {
        return Err("Incomplete LFO note-initialization corpus".into());
    }
    let mut initialization_errors = 0;
    let mut application_initializations = 0;
    let mut application_initialization_errors = 0;
    let mut clocked_application_initializations = 0;
    let mut clocked_application_errors = 0;
    for (i, r) in raw.chunks_exact(60).enumerate() {
        let mut state = LfoState {
            phase: word(r, 2),
            previous_random: word(r, 3) as i16,
            random: word(r, 4) as i16,
            half_cycle: word(r, 5) as u8,
        };
        let shared = LfoState {
            phase: word(r, 6),
            previous_random: word(r, 7) as i16,
            random: word(r, 8) as i16,
            half_cycle: 0,
        };
        let mut seed = word(r, 9) as u16;
        {
            let family = word(r, 0) as usize;
            let mut program = radias_synth_application::modulation::ModulationProgram::default();
            program.lfo[family].phase_sync = word(r, 1) as u8;
            let mut prior = [LfoState::default(); 2];
            prior[family] = state;
            let mut inherited = [LfoState::default(); 2];
            inherited[family] = shared;
            let mut random = seed;
            let voice =
                radias_synth_application::modulation::VoiceModulation::from_prior_with_clock(
                    program,
                    60 << 16,
                    inherited,
                    prior,
                    &mut random,
                    true,
                )
                .map_err(|_| "Configured clock rejected original note mode")?;
            let v = voice.pair.states[family];
            if [
                v.phase,
                v.previous_random as u16 as u32,
                v.random as u16 as u32,
                v.half_cycle as u32,
                random as u32,
            ] != core::array::from_fn::<_, 5, _>(|n| word(r, n + 10))
            {
                clocked_application_errors += 1;
            }
            clocked_application_initializations += 1;
        }
        if word(r, 1) & 128 == 0 {
            let family = word(r, 0) as usize;
            let mut program = radias_synth_application::modulation::ModulationProgram::default();
            program.lfo[family].phase_sync = word(r, 1) as u8;
            let mut old = [LfoState::default(); 2];
            old[family] = state;
            let mut inherited = [LfoState::default(); 2];
            inherited[family] = shared;
            let mut random = seed;
            let voice = radias_synth_application::modulation::VoiceModulation::from_prior(
                program,
                60 << 16,
                inherited,
                old,
                &mut random,
            )
            .map_err(|_| "Unexpected tempo rejection")?;
            let value = voice.pair.states[family];
            let actual = [
                value.phase,
                value.previous_random as u16 as u32,
                value.random as u16 as u32,
                value.half_cycle as u32,
                random as u32,
            ];
            let expected = core::array::from_fn::<_, 5, _>(|n| word(r, n + 10));
            if actual != expected {
                application_initialization_errors += 1;
            }
            application_initializations += 1;
        }
        state.initialize_note(word(r, 1) as u8, shared, &mut seed);
        let actual = [
            state.phase,
            state.previous_random as u16 as u32,
            state.random as u16 as u32,
            state.half_cycle as u32,
            seed as u32,
        ];
        let expected = core::array::from_fn::<_, 5, _>(|n| word(r, n + 10));
        if actual != expected {
            if initialization_errors < 3 {
                eprintln!("LFO initialization {i}: {actual:?} != {expected:?}");
            }
            initialization_errors += 1;
        }
    }
    let raw = fs::read(output.join("lfo-continuous.bin"))?;
    if raw.len() != 32768 * 26 * 4 {
        return Err("Incomplete continuous LFO pair corpus".into());
    }
    let mut pair = radias_synth_application::lfo::LfoPairController {
        states: [LfoState::default(); 2],
        parameters: [radias_synth_application::lfo::LfoParameters::default(); 2],
    };
    let mut seed = 0;
    let mut continuous_errors = 0;
    for (i, r) in raw.chunks_exact(26 * 4).enumerate() {
        if word(r, 1) == 0 {
            pair.states = [LfoState::default(); 2];
            seed = word(r, 2) as u16;
        }
        for (family, p) in pair.parameters.iter_mut().enumerate() {
            let b = 3 + 6 * family;
            *p = radias_synth_application::lfo::LfoParameters {
                waveform: word(r, b) as u8,
                shape: word(r, b + 1) as u8,
                frequency: word(r, b + 2) as u8,
                phase_sync: word(r, b + 3) as u8,
                frequency_offset: word(r, b + 4) as i8,
                frequency_modulation: word(r, b + 5) as i16,
            };
        }
        let values = pair
            .tick(&tables, &mut seed)
            .map_err(|_| "Unexpected tempo synchronization")?;
        let mut actual = [0u32; 11];
        actual[0] = values[0] as u16 as u32;
        actual[1] = values[1] as u16 as u32;
        for (family, s) in pair.states.iter().enumerate() {
            actual[2 + 4 * family..6 + 4 * family].copy_from_slice(&[
                s.phase,
                s.previous_random as u16 as u32,
                s.random as u16 as u32,
                s.half_cycle as u32,
            ]);
        }
        actual[10] = seed as u32;
        let expected = core::array::from_fn::<_, 11, _>(|n| {
            if n < 2 {
                word(r, 15 + n) as u16 as u32
            } else {
                word(r, 15 + n)
            }
        });
        if actual != expected {
            if continuous_errors < 3 {
                eprintln!("LFO pair tick {i}: {actual:?} != {expected:?}");
            }
            continuous_errors += 1;
        }
    }
    let passed = continuous_errors == 0
        && clocked_application_errors == 0
        && application_initialization_errors == 0
        && initialization_errors == 0
        && frequency_errors == 0
        && errors.iter().all(|&n| n == 0)
        && phase_errors == 0
        && offset_errors == 0
        && random_errors == 0;
    let report = serde_json::json!({"waveform_cases":262144,"waveform_errors":errors,"phase_cases":65536,"phase_errors":phase_errors,
        "offset_cases":256,"offset_errors":offset_errors,"random_cases":65536,"random_errors":random_errors,
        "frequency_cases":32768,"frequency_errors":frequency_errors,
        "note_initialization_cases":65536,"note_initialization_errors":initialization_errors,
        "application_retained_note_initializations":application_initializations,"application_retained_note_errors":application_initialization_errors,
        "continuous_pair_ticks":32768,"continuous_pair_errors":continuous_errors,
        "passed":passed,"original_sh3_bytes_executed":true,"native_lfo_routed_to_audio":false,"complete_lfo_lifecycle":false});
    let mut report = report;
    report["clock_enabled_application_note_cases"] = clocked_application_initializations.into();
    report["clock_enabled_application_note_errors"] = clocked_application_errors.into();
    fs::write(
        output.join("lfo-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Direct native LFO kernels differ".into());
    }
    Ok(())
}
