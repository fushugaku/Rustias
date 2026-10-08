use radias_synth_domain::lfo_tempo::LfoTempoState;
use radias_synth_infrastructure::firmware::lfo_tempo_tables;
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap())
}
fn state(r: &[u8], start: usize) -> LfoTempoState {
    LfoTempoState {
        phase: word(r, start),
        previous_increment: word(r, start + 1),
        reference_phase: word(r, start + 2),
        clock_count: word(r, start + 3) as u16,
        observed_clock_count: word(r, start + 4) as u16,
        correction_active: word(r, start + 5) as u8,
        correction_hold: word(r, start + 6) as u8,
        division: word(r, start + 7) as u8,
    }
}
fn history(r: &[u8], start: usize) -> radias_synth_domain::midi_clock::ClockIntervalHistory {
    radias_synth_domain::midi_clock::ClockIntervalHistory {
        changed: word(r, start) as u8,
        cursor: word(r, start + 1) as u8,
        instability: word(r, start + 2) as u8,
        intervals: core::array::from_fn(|i| word(r, start + 3 + i)),
        half_sum: word(r, start + 19),
        value: word(r, start + 20),
        scaled_value: word(r, start + 21),
        accepted_value: word(r, start + 22),
        delay: word(r, start + 23) as u8,
        direction: word(r, start + 24) as u8,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let tables = lfo_tempo_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = [0; 2];
    for (routine, kind) in ["phases", "increments"].iter().enumerate() {
        let data = fs::read(root.join(format!("runs/native-clone/lfo-tempo-{kind}.bin")))?;
        if data.len() != 16384 * 72 {
            return Err("Incomplete tempo corpus".into());
        }
        for (index, r) in data.chunks_exact(72).enumerate() {
            let mut actual = state(r, 1);
            let value = if routine == 0 {
                actual.phase_correction(word(r, 0) as u8, &tables) as u32
            } else {
                actual.increment(word(r, 0) as i32, &tables)
            };
            if value != word(r, 9) || actual != state(r, 10) {
                if errors[routine] < 3 {
                    eprintln!(
                        "Tempo {kind}/{index}: {value}, {actual:?} != {}, {:?}",
                        word(r, 9),
                        state(r, 10)
                    );
                }
                errors[routine] += 1;
            }
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-compiler.bin"))?;
    if raw.len() != 82085 * 20 {
        return Err("Incomplete tempo compiler corpus".into());
    }
    let mut compiler_errors = 0;
    for (i, r) in raw.chunks_exact(20).enumerate() {
        let (index, value) =
            tables.compile_increment(word(r, 0) as i32, word(r, 1) as i32, word(r, 2));
        if value != word(r, 3) || index as u32 != word(r, 4) {
            if compiler_errors < 3 {
                eprintln!(
                    "Tempo compiler {i}: [{index},{value}] != [{},{}]",
                    word(r, 4),
                    word(r, 3)
                );
            }
            compiler_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-settings.bin"))?;
    if raw.len() != 65542 * 12 {
        return Err("Incomplete tempo setting corpus".into());
    }
    let mut setting_errors = 0;
    for r in raw.chunks_exact(12) {
        let tempo = radias_synth_domain::lfo_tempo::TempoSetting::clamped(word(r, 0));
        if tempo.tenths_bpm() as u32 != word(r, 1) || tempo.clock_rate() != word(r, 2) {
            setting_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-initialize.bin"))?;
    if raw.len() != 32768 * 108 {
        return Err("Incomplete tempo note initialization corpus".into());
    }
    let mut initialization_errors = 0;
    for (i, r) in raw.chunks_exact(108).enumerate() {
        let mut actual = state(r, 3);
        let shared = state(r, 11);
        actual.initialize_note(word(r, 1) as u8, word(r, 2) as u8, shared);
        if actual != state(r, 19) {
            if initialization_errors < 3 {
                eprintln!("Tempo note {i}: {actual:?} != {:?}", state(r, 19));
            }
            initialization_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-events.bin"))?;
    if raw.len() != 49152 * 68 {
        return Err("Incomplete clock event corpus".into());
    }
    let mut clock_errors = 0;
    for (i, r) in raw.chunks_exact(68).enumerate() {
        let mut actual = state(r, 1);
        match word(r, 0) {
            0 => actual.clock_pulse_four(),
            1 => actual.clock_pulse_one(),
            2 => actual.reset_clock(),
            _ => return Err("Unknown original clock event".into()),
        }
        if actual != state(r, 9) {
            if clock_errors < 3 {
                eprintln!("Clock event {i}: {actual:?} != {:?}", state(r, 9));
            }
            clock_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-intervals.bin"))?;
    if raw.len() != 98304 * 8 {
        return Err("Incomplete clock interval corpus".into());
    }
    let mut interval_errors = 0;
    for (i, r) in raw.chunks_exact(8).enumerate() {
        let actual = radias_synth_domain::midi_clock::interval_to_tenths_bpm(word(r, 0));
        if actual != word(r, 1) {
            if interval_errors < 3 {
                eprintln!("Clock interval {i}: {actual} != {}", word(r, 1));
            }
            interval_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-timestamps.bin"))?;
    if raw.len() != 32768 * 32 {
        return Err("Incomplete clock timestamp corpus".into());
    }
    let mut timestamp_errors = 0;
    for (i, r) in raw.chunks_exact(32).enumerate() {
        let mut actual = radias_synth_domain::midi_clock::ClockTimestamp {
            countdown: word(r, 0),
            timeout_ticks: word(r, 2) as u8,
            flags: word(r, 3) as u8,
        };
        let elapsed = actual.observe(word(r, 1));
        let expected = radias_synth_domain::midi_clock::ClockTimestamp {
            countdown: word(r, 5),
            timeout_ticks: word(r, 6) as u8,
            flags: word(r, 7) as u8,
        };
        if elapsed != word(r, 4) || actual != expected {
            if timestamp_errors < 3 {
                eprintln!(
                    "Clock timestamp {i}: {elapsed}, {actual:?} != {}, {expected:?}",
                    word(r, 4)
                );
            }
            timestamp_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-dispatch.bin"))?;
    const DISPATCH_BYTES: usize = (1 + 65 * 8 * 2) * 4;
    if raw.len() != 2048 * DISPATCH_BYTES {
        return Err("Incomplete clock dispatcher corpus".into());
    }
    let mut dispatch_errors = 0;
    for (i, r) in raw.chunks_exact(DISPATCH_BYTES).enumerate() {
        let mut bank = radias_synth_application::clock::ControllerClockBank::default();
        for (j, s) in bank
            .voices
            .iter_mut()
            .flatten()
            .chain(bank.timbres.iter_mut().flatten())
            .chain(core::iter::once(&mut bank.global))
            .enumerate()
        {
            *s = state(r, 1 + 8 * j);
        }
        bank.pulse(match word(r, 0) {
            0 => radias_synth_application::clock::ClockPulse::ExternalFour,
            1 => radias_synth_application::clock::ClockPulse::TimerOne,
            _ => return Err("Unknown dispatcher pulse".into()),
        });
        for (j, s) in bank
            .voices
            .iter()
            .flatten()
            .chain(bank.timbres.iter().flatten())
            .chain(core::iter::once(&bank.global))
            .enumerate()
        {
            if *s != state(r, 1 + 65 * 8 + 8 * j) {
                if dispatch_errors < 3 {
                    eprintln!("Clock dispatcher {i}, state {j}: {s:?} differs");
                }
                dispatch_errors += 1;
            }
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-measurement.bin"))?;
    if raw.len() != 32768 * 76 {
        return Err("Incomplete clock measurement corpus".into());
    }
    let port = |value| match value {
        0 => Ok(radias_synth_domain::midi_clock::ClockInputPort::A),
        1 => Ok(radias_synth_domain::midi_clock::ClockInputPort::B),
        2 => Ok(radias_synth_domain::midi_clock::ClockInputPort::Other),
        _ => Err("Unknown original clock input port"),
    };
    let mut measurement_errors = 0;
    for (i, r) in raw.chunks_exact(76).enumerate() {
        let mut actual = radias_synth_domain::midi_clock::ClockIntervalMeasurement {
            interval: word(r, 2),
            flags: word(r, 3) as u8,
            warmup_count: word(r, 4) as u8,
            timestamp: radias_synth_domain::midi_clock::ClockTimestamp {
                countdown: word(r, 5),
                timeout_ticks: word(r, 7) as u8,
                flags: word(r, 8) as u8,
            },
        };
        let accepted =
            radias_synth_domain::midi_clock::accepts_clock(word(r, 1) as u8, port(word(r, 0))?);
        let mut input_timeout = word(r, 9);
        let measured = if accepted {
            input_timeout = 500;
            actual.observe(word(r, 6))
        } else {
            0
        };
        let expected = radias_synth_domain::midi_clock::ClockIntervalMeasurement {
            interval: word(r, 12),
            flags: word(r, 13) as u8,
            warmup_count: word(r, 14) as u8,
            timestamp: radias_synth_domain::midi_clock::ClockTimestamp {
                countdown: word(r, 15),
                timeout_ticks: word(r, 16) as u8,
                flags: word(r, 17) as u8,
            },
        };
        if accepted != (word(r, 10) != 0)
            || measured != word(r, 11)
            || actual != expected
            || input_timeout != word(r, 18)
        {
            if measurement_errors < 3 {
                eprintln!(
                    "Clock measurement {i}: accepted {accepted}, delta {measured}, {actual:?} != accepted {}, delta {}, {expected:?}",
                    word(r, 10),
                    word(r, 11)
                );
            }
            measurement_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-gates.bin"))?;
    if raw.len() != 768 * 12 {
        return Err("Incomplete clock gate corpus".into());
    }
    let mut gate_errors = 0;
    for r in raw.chunks_exact(12) {
        let actual = radias_synth_domain::midi_clock::propagation_suppressed(
            port(word(r, 0))?,
            word(r, 1) as u8,
        );
        if actual != (word(r, 2) != 0) {
            gate_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-smoothing.bin"))?;
    if raw.len() != 32768 * 12 {
        return Err("Incomplete clock smoothing corpus".into());
    }
    let mut smoothing_errors = 0;
    for r in raw.chunks_exact(12) {
        let mut actual = radias_synth_domain::midi_clock::ClockIntervalMeasurement {
            interval: word(r, 0),
            ..Default::default()
        };
        actual.smooth_interval(word(r, 1));
        if actual.interval != word(r, 2) {
            smoothing_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-tempo-propagation.bin"))?;
    const PROPAGATION_BYTES: usize = (1 + 65 + 65 * 8 * 2 + 2) * 4;
    if raw.len() != 2048 * PROPAGATION_BYTES {
        return Err("Incomplete global tempo propagation corpus".into());
    }
    let mut propagation_errors = 0;
    for (i, r) in raw.chunks_exact(PROPAGATION_BYTES).enumerate() {
        let mut bank = radias_synth_application::clock::ControllerClockBank::default();
        let mut divisions = radias_synth_application::clock::TempoDivisions::default();
        for (j, division) in divisions
            .voices
            .iter_mut()
            .flatten()
            .chain(divisions.timbres.iter_mut().flatten())
            .chain(core::iter::once(&mut divisions.global))
            .enumerate()
        {
            *division = word(r, 1 + j) as u8;
        }
        for (j, s) in bank
            .voices
            .iter_mut()
            .flatten()
            .chain(bank.timbres.iter_mut().flatten())
            .chain(core::iter::once(&mut bank.global))
            .enumerate()
        {
            *s = state(r, 66 + 8 * j);
        }
        let setting = radias_synth_domain::lfo_tempo::TempoSetting::clamped(word(r, 0));
        bank.compile_rates(&divisions, setting, &tables);
        let mut different = false;
        for (j, s) in bank
            .voices
            .iter()
            .flatten()
            .chain(bank.timbres.iter().flatten())
            .chain(core::iter::once(&bank.global))
            .enumerate()
        {
            if *s != state(r, 66 + 65 * 8 + 8 * j) {
                different = true;
                if propagation_errors < 3 {
                    eprintln!(
                        "Tempo propagation {i}, state {j}: {s:?} != {:?}",
                        state(r, 66 + 65 * 8 + 8 * j)
                    );
                }
            }
        }
        if different
            || setting.tenths_bpm() as u32 != word(r, 66 + 65 * 8 * 2)
            || setting.clock_rate() != word(r, 67 + 65 * 8 * 2)
        {
            propagation_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-timer-tempo.bin"))?;
    if raw.len() != 65536 * 24 {
        return Err("Incomplete tempo timer corpus".into());
    }
    let mut timer_tempo_errors = 0;
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let mut timer = radias_synth_domain::midi_clock::ClockTimerReload {
            constant: word(r, 2),
            counter: word(r, 3),
            last_external_interval: 0,
        };
        timer.set_tempo(word(r, 0), word(r, 1) as u8);
        if timer.constant != word(r, 4) || timer.counter != word(r, 5) {
            if timer_tempo_errors < 3 {
                eprintln!("Tempo timer {i}: {timer:?} differs");
            }
            timer_tempo_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-timer-interval.bin"))?;
    if raw.len() != 32768 * 24 {
        return Err("Incomplete external timer interval corpus".into());
    }
    let mut timer_interval_errors = 0;
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let mut timer = radias_synth_domain::midi_clock::ClockTimerReload {
            last_external_interval: word(r, 2),
            ..Default::default()
        };
        timer.set_external_interval(word(r, 0), word(r, 1));
        if timer.constant != word(r, 3)
            || timer.counter != word(r, 4)
            || timer.last_external_interval != word(r, 5)
        {
            if timer_interval_errors < 3 {
                eprintln!("External timer {i}: {timer:?} differs");
            }
            timer_interval_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-timeouts.bin"))?;
    if raw.len() != 65536 * 16 {
        return Err("Incomplete clock timeout corpus".into());
    }
    let mut timeout_errors = 0;
    for r in raw.chunks_exact(16) {
        let mut timestamp = radias_synth_domain::midi_clock::ClockTimestamp {
            timeout_ticks: word(r, 0) as u8,
            flags: word(r, 1) as u8,
            ..Default::default()
        };
        timestamp.service_timeout();
        if timestamp.timeout_ticks != word(r, 2) as u8 || timestamp.flags != word(r, 3) as u8 {
            timeout_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-clock-history.bin"))?;
    if raw.len() != 16384 * 216 {
        return Err("Incomplete clock interval history corpus".into());
    }
    let mut history_errors = 0;
    for (i, r) in raw.chunks_exact(216).enumerate() {
        let mut actual = history(r, 2);
        actual.observe(word(r, 0), word(r, 1) as u8);
        let expected = history(r, 28);
        if actual != expected || word(r, 27) != word(r, 53) {
            if history_errors < 3 {
                eprintln!("Clock history {i}: {actual:?} != {expected:?}");
            }
            history_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/lfo-tempo-continuous.bin"))?;
    if raw.len() != 8192 * 172 {
        return Err("Incomplete continuous tempo LFO corpus".into());
    }
    let lfo_tables = radias_synth_infrastructure::firmware::lfo_tables(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let mut pair = radias_synth_application::lfo::LfoPairController {
        states: [Default::default(); 2],
        parameters: [Default::default(); 2],
    };
    let mut tempo = [LfoTempoState::default(); 2];
    let mut seed = 0;
    let mut continuous_errors = 0;
    for (i, r) in raw.chunks_exact(172).enumerate() {
        let scenario = word(r, 0);
        let tick = word(r, 1);
        if tick == 0 {
            pair.states = [Default::default(); 2];
            tempo = [Default::default(); 2];
            seed = word(r, 2) as u16;
        }
        let divisions = [word(r, 16) as u8, word(r, 17) as u8];
        for family in 0..2 {
            let n = 4 + 6 * family;
            pair.parameters[family] = radias_synth_application::lfo::LfoParameters {
                waveform: word(r, n) as u8,
                shape: word(r, n + 1) as u8,
                frequency: word(r, n + 2) as u8,
                phase_sync: word(r, n + 3) as u8,
                frequency_offset: word(r, n + 4) as i8,
                frequency_modulation: word(r, n + 5) as i16,
            };
            if tick.is_multiple_of(64) {
                let rate =
                    radias_synth_domain::lfo_tempo::TempoSetting::clamped(200 + scenario * 20)
                        .clock_rate();
                tempo[family].previous_increment = tables
                    .compile_increment(divisions[family] as i32, 0, rate)
                    .1;
            }
            if word(r, 3) != 0 {
                tempo[family].clock_count = tempo[family].clock_count.wrapping_add(1);
                tempo[family].reference_phase = pair.states[family].phase;
                tempo[family].correction_active = 1;
                tempo[family].correction_hold = 10;
            }
        }
        let values = pair.tick_with_tempo(&lfo_tables, &tables, divisions, &mut tempo, &mut seed);
        let values_match = values[0] == word(r, 18) as i16 && values[1] == word(r, 19) as i16;
        let states_match = (0..2).all(|family| {
            let n = 36 + 3 * family;
            let s = pair.states[family];
            tempo[family] == state(r, 20 + 8 * family)
                && s.phase == tempo[family].phase
                && s.previous_random == word(r, n) as i16
                && s.random == word(r, n + 1) as i16
                && s.half_cycle == word(r, n + 2) as u8
        });
        if !values_match || !states_match || seed != word(r, 42) as u16 {
            if continuous_errors < 3 {
                eprintln!(
                    "Continuous tempo tick {i}: values {values:?}, states {tempo:?}, seed {seed} differ"
                );
            }
            continuous_errors += 1;
        }
    }
    let passed = errors == [0; 2]
        && compiler_errors == 0
        && setting_errors == 0
        && continuous_errors == 0
        && initialization_errors == 0
        && clock_errors == 0
        && interval_errors == 0
        && timestamp_errors == 0
        && dispatch_errors == 0
        && measurement_errors == 0
        && gate_errors == 0
        && smoothing_errors == 0
        && propagation_errors == 0
        && timer_tempo_errors == 0
        && timer_interval_errors == 0
        && timeout_errors == 0
        && history_errors == 0;
    let report = serde_json::json!({"passed":passed,"phase_correction_cases":16384,"phase_correction_errors":errors[0],"rate_selection_cases":16384,"rate_selection_errors":errors[1],"tempo_compiler_cases":82085,"tempo_compiler_errors":compiler_errors,"tempo_setting_cases":65542,"tempo_setting_errors":setting_errors,"all_supported_bpm_division_products":49317,"tempo_note_initialization_cases":32768,"tempo_note_initialization_errors":initialization_errors,"clock_event_state_cases":49152,"clock_event_state_errors":clock_errors,"continuous_pair_ticks":8192,"continuous_errors":continuous_errors,"original_sh3_executed":true,"controlled_clock_inputs":true,"midi_clock_delivery_qualified":false,"tempo_rate_compiler_qualified":compiler_errors==0&&setting_errors==0,"global_bpm_propagation_qualified":false,"complete_tempo_lfo":false});
    let mut report = report;
    report["clock_interval_cases"] = 98304.into();
    report["clock_interval_errors"] = interval_errors.into();
    report["clock_timestamp_cases"] = 32768.into();
    report["clock_timestamp_errors"] = timestamp_errors.into();
    report["timestamp_timer_read_preserved"] = true.into();
    report["timestamp_timer_held_stopped"] = true.into();
    report["clock_dispatcher_calls"] = 2048.into();
    report["clock_dispatcher_state_observations"] = 133120.into();
    report["clock_dispatcher_state_errors"] = dispatch_errors.into();
    report["clock_dispatcher_all_65_states_qualified"] = (dispatch_errors == 0).into();
    report["clock_measurement_prefix_cases"] = 32768.into();
    report["clock_measurement_prefix_errors"] = measurement_errors.into();
    report["clock_measurement_pending_start_excluded"] = true.into();
    report["clock_propagation_gate_cases"] = 768.into();
    report["clock_propagation_gate_errors"] = gate_errors.into();
    report["clock_smoothing_block_cases"] = 32768.into();
    report["clock_smoothing_block_errors"] = smoothing_errors.into();
    report["global_tempo_routine_calls"] = 2048.into();
    report["global_tempo_lfo_state_observations"] = 133120.into();
    report["global_tempo_lfo_propagation_errors"] = propagation_errors.into();
    report["global_bpm_lfo_rate_propagation_qualified"] = (propagation_errors == 0).into();
    report["internal_timer_tempo_cases"] = 65536.into();
    report["internal_timer_tempo_errors"] = timer_tempo_errors.into();
    report["external_timer_interval_cases"] = 32768.into();
    report["external_timer_interval_errors"] = timer_interval_errors.into();
    report["clock_timeout_cases"] = 65536.into();
    report["clock_timeout_errors"] = timeout_errors.into();
    report["clock_interval_history_cases"] = 16384.into();
    report["clock_interval_history_errors"] = history_errors.into();
    fs::write(
        root.join("runs/native-clone/lfo-tempo-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native LFO tempo state differs".into());
    }
    Ok(())
}
