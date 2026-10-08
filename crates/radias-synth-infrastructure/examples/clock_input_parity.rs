use radias_synth_application::clock::{
    ClockActions, ClockInputState, ControllerClockBank, ExternalClockReceiver, TempoDivisions,
};
use radias_synth_domain::{
    lfo_tempo::{LfoTempoState, TempoSetting},
    midi_clock::{
        AudioTimerClock, ClockInputPort, ClockIntervalHistory, ClockIntervalMeasurement,
        ClockTimerReload, ClockTimestamp, SequencedClockCounter, TempoUpdateLatch,
    },
};
use radias_synth_infrastructure::firmware::lfo_tempo_tables;
use std::{fs, path::PathBuf};

fn word(data: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(data[4 * i..4 * i + 4].try_into().unwrap())
}
fn lfo(words: &[u32]) -> LfoTempoState {
    LfoTempoState {
        phase: words[0],
        previous_increment: words[1],
        reference_phase: words[2],
        clock_count: words[3] as u16,
        observed_clock_count: words[4] as u16,
        correction_active: words[5] as u8,
        correction_hold: words[6] as u8,
        division: words[7] as u8,
    }
}
fn input(w: &[u32]) -> ClockInputState {
    ClockInputState {
        measurement: ClockIntervalMeasurement {
            interval: w[0],
            flags: w[1] as u8,
            warmup_count: w[2] as u8,
            timestamp: ClockTimestamp {
                countdown: w[3],
                timeout_ticks: w[4] as u8,
                flags: w[5] as u8,
            },
        },
        input_timeout: w[6] as u16,
        acceptance_delay: w[7] as u8,
        refresh_ticks: w[33] as u16,
        history: ClockIntervalHistory {
            changed: w[8] as u8,
            cursor: w[9] as u8,
            instability: w[10] as u8,
            intervals: w[11..27].try_into().unwrap(),
            half_sum: w[27],
            value: w[28],
            scaled_value: w[29],
            accepted_value: w[30],
            delay: w[31] as u8,
            direction: w[32] as u8,
        },
    }
}
fn snapshot(
    receiver: &ExternalClockReceiver,
    bank: &ControllerClockBank,
    timer: &ClockTimerReload,
) -> Vec<u32> {
    let mut w = Vec::with_capacity(638);
    for input in receiver.inputs {
        let m = input.measurement;
        let h = input.history;
        w.extend([
            m.interval,
            m.flags as u32,
            m.warmup_count as u32,
            m.timestamp.countdown,
            m.timestamp.timeout_ticks as u32,
            m.timestamp.flags as u32,
            input.input_timeout as u32,
            input.acceptance_delay as u32,
            h.changed as u32,
            h.cursor as u32,
            h.instability as u32,
        ]);
        w.extend(h.intervals);
        w.extend([
            h.half_sum,
            h.value,
            h.scaled_value,
            h.accepted_value,
            h.delay as u32,
            h.direction as u32,
        ]);
        w.push(input.refresh_ticks as u32);
    }
    for s in bank
        .voices
        .iter()
        .flatten()
        .chain(bank.timbres.iter().flatten())
        .chain(core::iter::once(&bank.global))
    {
        w.extend([
            s.phase,
            s.previous_increment,
            s.reference_phase,
            s.clock_count as u32,
            s.observed_clock_count as u32,
            s.correction_active as u32,
            s.correction_hold as u32,
            s.division as u32,
        ]);
    }
    w.extend([
        receiver.reported_tenths_bpm as u32,
        receiver.tempo.tenths_bpm() as u32,
        receiver.tempo.clock_rate(),
        receiver.notification.tenths_bpm as u32,
        receiver.notification.pending as u32,
        receiver.pulse_position as u32,
        receiver.beat_phase as u32,
        receiver.timer_interrupts as u32,
        receiver.sequenced_clock.countdown as u32,
        receiver.sequenced_clock.remaining as u32,
        receiver.sequenced_clock.block_length as u32,
        receiver.sequenced_clock.completed_blocks as u32,
        timer.constant,
        timer.counter,
        timer.last_external_interval,
        receiver.source_mode as u32,
    ]);
    w
}
fn compare(actual: &[u32], expected: &[u32]) -> Option<usize> {
    actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .or_else(|| (actual.len() != expected.len()).then_some(actual.len().min(expected.len())))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let data = fs::read(root.join("runs/native-clone/clock-input.bin"))?;
    const RECORD_BYTES: usize = (14 + 638 * 2) * 4;
    if data.len() != 65 * 4 + 4096 * RECORD_BYTES {
        return Err("Incomplete original clock input corpus".into());
    }
    let tables = lfo_tempo_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut divisions = TempoDivisions::default();
    for (i, v) in divisions
        .voices
        .iter_mut()
        .flatten()
        .chain(divisions.timbres.iter_mut().flatten())
        .chain(core::iter::once(&mut divisions.global))
        .enumerate()
    {
        *v = word(&data, i) as u8;
    }
    let mut bank = ControllerClockBank::default();
    let mut timer = ClockTimerReload::default();
    let mut receiver = ExternalClockReceiver {
        inputs: [Default::default(); 3],
        settings: 0,
        source_mode: 0,
        controller_mode: 0,
        reported_tenths_bpm: 1200,
        tempo: TempoSetting::clamped(1200),
        notification: TempoUpdateLatch::default(),
        pulse_position: 0,
        beat_phase: 0,
        timer_interrupts: 0,
        sequenced_clock: SequencedClockCounter::default(),
    };
    let mut errors = 0;
    let mut callback_errors = 0;
    let mut before_errors = 0;
    let mut counts = [0usize; 11];
    let mut restart_count = 0;
    let mut pulse_count = 0;
    let mut tempo_count = 0;
    let mut stop_count = 0;
    let mut negative_rejected = false;
    for (n, r) in data[65 * 4..].chunks_exact(RECORD_BYTES).enumerate() {
        let tick = word(r, 1);
        let kind = word(r, 2) as usize;
        let port_index = word(r, 3) as usize;
        let port = match port_index {
            0 => ClockInputPort::A,
            1 => ClockInputPort::B,
            2 => ClockInputPort::Other,
            _ => return Err("Invalid input port".into()),
        };
        let before: Vec<_> = (0..638).map(|i| word(r, 14 + i)).collect();
        let after: Vec<_> = (0..638).map(|i| word(r, 14 + 638 + i)).collect();
        if tick == 0 {
            receiver.inputs = core::array::from_fn(|i| input(&before[34 * i..34 * i + 34]));
            for (i, s) in bank
                .voices
                .iter_mut()
                .flatten()
                .chain(bank.timbres.iter_mut().flatten())
                .chain(core::iter::once(&mut bank.global))
                .enumerate()
            {
                *s = lfo(&before[102 + 8 * i..102 + 8 * i + 8]);
            }
            receiver.reported_tenths_bpm = before[622] as u16;
            receiver.tempo = TempoSetting::clamped(before[623]);
            receiver.notification = TempoUpdateLatch {
                tenths_bpm: before[625] as u16,
                pending: before[626] as u16,
            };
            receiver.pulse_position = before[627] as u16;
            receiver.beat_phase = before[628] as u8;
            receiver.timer_interrupts = before[629] as u8;
            receiver.sequenced_clock = SequencedClockCounter {
                countdown: before[630] as u16,
                remaining: before[631] as u16,
                block_length: before[632] as u16,
                completed_blocks: before[633] as u8,
            };
            timer = ClockTimerReload {
                constant: before[634],
                counter: before[635],
                last_external_interval: before[636],
            };
            receiver.source_mode = before[637] as u8;
        }
        if let Some(i) = compare(&snapshot(&receiver, &bank, &timer), &before) {
            before_errors += 1;
            if before_errors <= 3 {
                eprintln!("Clock before {n}, word {i} differs");
            }
        }
        receiver.settings = word(r, 4) as u8;
        receiver.controller_mode = word(r, 6) as u8;
        let actions = match kind {
            0 => receiver.observe_clock(port, word(r, 7), &mut bank, &divisions, &tables),
            1 | 2 => {
                receiver.arm_transport(port, kind == 1);
                ClockActions::default()
            }
            3 => receiver.stop_transport(port),
            4 => {
                receiver.inputs[port_index]
                    .measurement
                    .timestamp
                    .service_timeout();
                ClockActions::default()
            }
            5 => {
                receiver.deliver_history(port, port_index as u32 + 1);
                ClockActions::default()
            }
            6 => receiver.timer_interrupt(&mut bank),
            7 => {
                receiver.notification.service();
                ClockActions::default()
            }
            8 => {
                let _ = receiver.notification.take_ready();
                ClockActions::default()
            }
            9 => receiver.select_source(
                word(r, 13) as u16,
                &mut timer,
                &mut bank,
                &divisions,
                &tables,
            ),
            10 => {
                receiver.service_input_liveness(port, false);
                ClockActions::default()
            }
            _ => return Err("Unknown clock input event".into()),
        };
        counts[kind] += 1;
        let actual_actions = [
            actions.restart as u32,
            actions.pulse as u32,
            actions.tempo_changed as u32,
            actions.stop as u32,
            actions.source_update as u32,
        ];
        let expected_actions = core::array::from_fn::<_, 5, _>(|i| word(r, 8 + i));
        if actual_actions != expected_actions {
            callback_errors += 1;
            if callback_errors <= 3 {
                eprintln!("Clock callbacks {n}: {actual_actions:?} != {expected_actions:?}");
            }
        }
        restart_count += actions.restart as usize;
        pulse_count += actions.pulse as usize;
        tempo_count += actions.tempo_changed as usize;
        stop_count += actions.stop as usize;
        let actual = snapshot(&receiver, &bank, &timer);
        if let Some(i) = compare(&actual, &after) {
            errors += 1;
            if errors <= 3 {
                eprintln!("Clock after {n}, word {i}: {} != {}", actual[i], after[i]);
            }
        }
        if n == 0 {
            let mut altered = after.clone();
            altered[100] ^= 1;
            negative_rejected = compare(&actual, &altered).is_some();
        }
    }
    let raw = fs::read(root.join("runs/native-clone/clock-input-liveness.bin"))?;
    if raw.len() != 16384 * 32 {
        return Err("Incomplete clock liveness corpus".into());
    }
    let mut liveness_errors = 0;
    for r in raw.chunks_exact(32) {
        let mut timeout = word(r, 2) as u16;
        let mut refresh = word(r, 3) as u16;
        let mut flags = word(r, 4) as u8;
        radias_synth_domain::midi_clock::service_clock_liveness(
            word(r, 0) as u8,
            word(r, 1) != 0,
            &mut timeout,
            &mut refresh,
            &mut flags,
        );
        if timeout as u32 != word(r, 5)
            || refresh as u32 != word(r, 6)
            || flags as u32 != word(r, 7)
        {
            liveness_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/clock-input-timers.bin"))?;
    if raw.len() != 16384 * 20 {
        return Err("Incomplete peripheral timer corpus".into());
    }
    let mut timer_errors = 0;
    for r in raw.chunks_exact(20) {
        let mut timer = ClockTimerReload {
            constant: word(r, 0),
            counter: word(r, 1),
            last_external_interval: 0,
        };
        let expired = timer.advance_ticks(word(r, 2));
        if expired != word(r, 3) || timer.counter != word(r, 4) {
            timer_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/clock-input-sequence-counter.bin"))?;
    if raw.len() != 16384 * 32 {
        return Err("Incomplete sequence timer counter corpus".into());
    }
    let mut sequence_counter_errors = 0;
    for r in raw.chunks_exact(32) {
        let mut counter = SequencedClockCounter {
            countdown: word(r, 0) as u16,
            remaining: word(r, 1) as u16,
            block_length: word(r, 2) as u16,
            completed_blocks: word(r, 3) as u8,
        };
        counter.service();
        if counter
            != (SequencedClockCounter {
                countdown: word(r, 4) as u16,
                remaining: word(r, 5) as u16,
                block_length: word(r, 6) as u16,
                completed_blocks: word(r, 7) as u8,
            })
        {
            sequence_counter_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/clock-input-audio-timers.bin"))?;
    if raw.len() != 24576 * 24 {
        return Err("Incomplete audio timer corpus".into());
    }
    let mut audio_clock = AudioTimerClock::default();
    let mut audio_timer = ClockTimerReload::default();
    let mut audio_timer_errors = 0;
    for r in raw.chunks_exact(24) {
        let frame = word(r, 1);
        if frame == 0 {
            audio_clock = AudioTimerClock::default();
        }
        if frame.is_multiple_of(512) {
            audio_timer.set_tempo(word(r, 2), 0);
        }
        let expired = audio_timer.advance_ticks(audio_clock.next_ticks());
        if expired != word(r, 3)
            || audio_timer.counter != word(r, 4)
            || audio_clock.fraction != word(r, 5) * 500
        {
            audio_timer_errors += 1;
        }
    }
    let passed = errors == 0
        && callback_errors == 0
        && before_errors == 0
        && negative_rejected
        && liveness_errors == 0
        && timer_errors == 0
        && sequence_counter_errors == 0
        && audio_timer_errors == 0;
    let report = serde_json::json!({"passed":passed,"whole_original_sys_calls":4096,"continuous_scenarios":16,"state_words_per_snapshot":638,"before_state_errors":before_errors,"after_state_errors":errors,"callback_errors":callback_errors,"event_counts":counts,"restart_notifications":restart_count,"pulse_notifications":pulse_count,"tempo_notifications":tempo_count,"stop_notifications":stop_count,"one_bit_state_change_rejected":negative_rejected,"native_interpreter_used":false,"original_instructions_modified":false,"sequence_effect_programs_idle":true,"sequence_forwarding_effect_outputs_compared":false,"sample_hpi_timing_qualified":false,"complete_clock_engine":false});
    let mut report = report;
    report["input_liveness_cases"] = 16384.into();
    report["input_liveness_errors"] = liveness_errors.into();
    report["peripheral_model_timer_cases"] = 16384.into();
    report["peripheral_model_timer_errors"] = timer_errors.into();
    report["sequence_timer_counter_cases"] = 16384.into();
    report["sequence_timer_counter_errors"] = sequence_counter_errors.into();
    report["audio_frame_timer_comparisons"] = 24576.into();
    report["audio_frame_timer_errors"] = audio_timer_errors.into();
    report["timer_model_cpu_instruction_latency_excluded"] = true.into();
    fs::write(
        root.join("runs/native-clone/clock-input-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native clock input differs from whole SYS execution".into());
    }
    Ok(())
}
