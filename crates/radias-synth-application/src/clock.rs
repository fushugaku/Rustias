//! Controller clock delivery. Inactive physical slots receive clocks too.
use crate::{lfo::LFO_COUNT, polyphony::TIMBRE_COUNT};
use radias_synth_domain::lfo_tempo::{LfoTempoState, LfoTempoTables, TempoSetting};
use radias_synth_domain::midi_clock::{
    ClockInputPort, ClockIntervalHistory, ClockIntervalMeasurement, ClockTimerReload,
    SequencedClockCounter, TempoUpdateLatch, accepts_clock, interval_to_tenths_bpm,
    propagation_suppressed, selected_clock_mode,
};
use radias_synth_domain::voice_allocation::VOICE_COUNT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockPulse {
    ExternalFour,
    TimerOne,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerClockBank {
    pub voices: [[LfoTempoState; LFO_COUNT]; VOICE_COUNT],
    /// Two synthesis LFOs and two effect LFOs for each timbre.
    pub timbres: [[LfoTempoState; LFO_COUNT + 2]; TIMBRE_COUNT],
    pub global: LfoTempoState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TempoDivisions {
    pub voices: [[u8; LFO_COUNT]; VOICE_COUNT],
    pub timbres: [[u8; LFO_COUNT + 2]; TIMBRE_COUNT],
    pub global: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockInputState {
    pub measurement: ClockIntervalMeasurement,
    pub history: ClockIntervalHistory,
    pub input_timeout: u16,
    pub acceptance_delay: u8,
    pub refresh_ticks: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockActions {
    /// The synth/sequence applications must also receive this notification.
    pub restart: bool,
    pub pulse: bool,
    pub tempo_changed: bool,
    pub stop: bool,
    pub source_update: bool,
}

/// External clock aggregate. Context B's measurement flags are the actual
/// propagation gate for context A; they must not be duplicated as configuration.
pub struct ExternalClockReceiver {
    pub inputs: [ClockInputState; 3],
    pub settings: u8,
    pub source_mode: u8,
    pub controller_mode: u8,
    pub reported_tenths_bpm: u16,
    pub tempo: TempoSetting,
    pub notification: TempoUpdateLatch,
    pub pulse_position: u16,
    pub beat_phase: u8,
    pub timer_interrupts: u8,
    pub sequenced_clock: SequencedClockCounter,
}

/// Native instrument clock at the synthesis sample boundary. Processor
/// instruction/HPI latency is a separate reference qualification.
pub struct InstrumentClock {
    pub bank: ControllerClockBank,
    pub divisions: TempoDivisions,
    pub receiver: ExternalClockReceiver,
    pub tables: LfoTempoTables,
    pub program_tempo: u16,
    timer: ClockTimerReload,
    frame_clock: radias_synth_domain::midi_clock::AudioTimerClock,
    countdown: u32,
}

impl Default for ControllerClockBank {
    fn default() -> Self {
        Self {
            voices: [[Default::default(); LFO_COUNT]; VOICE_COUNT],
            timbres: [[Default::default(); LFO_COUNT + 2]; TIMBRE_COUNT],
            global: Default::default(),
        }
    }
}
impl Default for TempoDivisions {
    fn default() -> Self {
        Self {
            voices: [[0; LFO_COUNT]; VOICE_COUNT],
            timbres: [[0; LFO_COUNT + 2]; TIMBRE_COUNT],
            global: 0,
        }
    }
}
impl InstrumentClock {
    pub fn internal(tables: LfoTempoTables, tempo: u16) -> Self {
        let mut inputs = [ClockInputState::default(); 3];
        for input in &mut inputs {
            input.measurement.timestamp = radias_synth_domain::midi_clock::ClockTimestamp {
                countdown: u32::MAX,
                timeout_ticks: 250,
                flags: 1,
            };
            input.history.instability = 3;
        }
        let mut clock = Self {
            bank: ControllerClockBank::default(),
            divisions: TempoDivisions {
                voices: [[8; LFO_COUNT]; VOICE_COUNT],
                timbres: [[8; LFO_COUNT + 2]; TIMBRE_COUNT],
                global: 8,
            },
            receiver: ExternalClockReceiver {
                inputs,
                settings: 4,
                source_mode: 0,
                controller_mode: 0,
                reported_tenths_bpm: tempo,
                tempo: TempoSetting::clamped(tempo as u32),
                notification: TempoUpdateLatch {
                    tenths_bpm: tempo,
                    pending: 0,
                },
                pulse_position: 0,
                beat_phase: 0,
                timer_interrupts: 0,
                sequenced_clock: SequencedClockCounter::default(),
            },
            tables,
            program_tempo: tempo,
            timer: ClockTimerReload::default(),
            frame_clock: Default::default(),
            countdown: u32::MAX,
        };
        clock.set_program_tempo(tempo);
        clock
    }
    /// 00551e's internal-source parameter update. Store the program word even
    /// when the currently selected external source owns the live tempo.
    pub fn set_program_tempo(&mut self, tempo: u16) {
        self.program_tempo = tempo;
        if self.receiver.source_mode & 3 == 0 {
            self.receiver.reported_tenths_bpm = tempo;
            self.timer
                .set_tempo(tempo as u32, self.receiver.controller_mode);
            self.receiver.publish_tempo(
                tempo as u32,
                &mut self.bank,
                &self.divisions,
                &self.tables,
            );
        }
    }
    pub fn next_audio_frame(&mut self) {
        let ticks = self.frame_clock.next_ticks();
        self.countdown = self.countdown.wrapping_sub(ticks);
        let expired = self.timer.advance_ticks(ticks);
        for _ in 0..expired {
            self.receiver.timer_interrupt(&mut self.bank);
        }
    }
    /// Supply this at the instrument's controller service. The effects layer
    /// receives a ready deferred tempo notification separately from LFO rates.
    pub fn controller_service(&mut self) -> Option<u16> {
        for port in [ClockInputPort::A, ClockInputPort::B] {
            self.receiver.inputs[port_index(port)]
                .measurement
                .timestamp
                .service_timeout();
            self.receiver.service_input_liveness(port, false);
            self.receiver
                .deliver_history(port, port_index(port) as u32 + 1);
        }
        self.receiver.notification.service();
        self.receiver.select_source(
            self.program_tempo,
            &mut self.timer,
            &mut self.bank,
            &self.divisions,
            &self.tables,
        );
        self.receiver.notification.take_ready()
    }
    pub fn external_clock(&mut self, port: ClockInputPort) -> ClockActions {
        self.receiver.observe_clock(
            port,
            self.countdown,
            &mut self.bank,
            &self.divisions,
            &self.tables,
        )
    }
}

fn port_index(port: ClockInputPort) -> usize {
    match port {
        ClockInputPort::A => 0,
        ClockInputPort::B => 1,
        ClockInputPort::Other => 2,
    }
}

impl ExternalClockReceiver {
    pub fn service_input_liveness(&mut self, port: ClockInputPort, busy: bool) {
        let input = &mut self.inputs[port_index(port)];
        radias_synth_domain::midi_clock::service_clock_liveness(
            self.settings,
            busy,
            &mut input.input_timeout,
            &mut input.refresh_ticks,
            &mut input.measurement.flags,
        );
    }
    fn publish_tempo(
        &mut self,
        raw: u32,
        bank: &mut ControllerClockBank,
        divisions: &TempoDivisions,
        tables: &LfoTempoTables,
    ) {
        self.tempo = TempoSetting::clamped(raw);
        bank.compile_rates(divisions, self.tempo, tables);
        if self.source_mode & 3 == 0 && self.controller_mode != 2 {
            self.notification.request(self.tempo.tenths_bpm() as u32);
        }
    }

    /// Whole 028a40 source transition. The program's tempo is little-endian
    /// at the file adapter; only its decoded word reaches this application.
    pub fn select_source(
        &mut self,
        program_tempo: u16,
        timer: &mut ClockTimerReload,
        bank: &mut ControllerClockBank,
        divisions: &TempoDivisions,
        tables: &LfoTempoTables,
    ) -> ClockActions {
        for input in &mut self.inputs[..2] {
            let f = &mut input.measurement.flags;
            if (*f & 2 != 0) != (*f & 16 != 0) {
                *f |= 4;
            }
        }
        let mode = selected_clock_mode(
            self.settings,
            self.inputs[0].measurement.flags,
            self.inputs[1].measurement.flags,
        );
        let changed = mode != self.source_mode;
        self.source_mode = mode;
        let mut actions = ClockActions::default();
        let internal = if changed {
            matches!(mode, 0 | 4)
        } else {
            mode == 4 && program_tempo as u32 != self.reported_tenths_bpm as i16 as i32 as u32
        };
        if internal {
            self.reported_tenths_bpm = program_tempo;
            if changed {
                timer.set_tempo(program_tempo as u32, self.controller_mode);
            }
            self.publish_tempo(program_tempo as u32, bank, divisions, tables);
            actions.tempo_changed = true;
            actions.source_update = true;
        } else {
            let selected = if changed {
                match mode {
                    1 | 5 => Some(0),
                    2 | 6 => Some(1),
                    _ => None,
                }
            } else {
                match mode {
                    5 => Some(0),
                    6 | 7 => Some(1),
                    _ => None,
                }
            };
            if let Some(index) = selected {
                let flags = self.inputs[index].measurement.flags;
                let force = changed && mode >= 5;
                if force || flags & 4 != 0 {
                    if force || flags & 2 != 0 {
                        let raw = interval_to_tenths_bpm(self.inputs[index].measurement.interval);
                        if force || raw != self.reported_tenths_bpm as i16 as i32 as u32 {
                            self.reported_tenths_bpm = raw as u16;
                            self.publish_tempo(raw, bank, divisions, tables);
                            actions.tempo_changed = true;
                            actions.source_update = true;
                        }
                    } else if flags & 16 != 0 {
                        actions.source_update = true;
                    }
                }
            }
        }
        for input in &mut self.inputs[..2] {
            let flags = &mut input.measurement.flags;
            *flags &= !(4 | 16);
            if *flags & 2 != 0 {
                *flags |= 16;
            }
        }
        actions
    }
    /// Original 02e76c ISR gates. Mode 2 services its sequence counter;
    /// other modes deliver a one-count pulse only for the internal source.
    pub fn timer_interrupt(&mut self, bank: &mut ControllerClockBank) -> ClockActions {
        self.timer_interrupts = self.timer_interrupts.wrapping_add(1);
        if self.controller_mode == 2 {
            self.sequenced_clock.service();
            return ClockActions::default();
        }
        if self.source_mode & 3 != 0 {
            return ClockActions::default();
        }
        self.pulse_position = self.pulse_position.wrapping_add(1);
        let phase = self.beat_phase as u16 + 1;
        self.beat_phase = (if phase >= 96 { phase - 96 } else { phase }) as u8;
        bank.pulse(ClockPulse::TimerOne);
        ClockActions {
            pulse: true,
            ..Default::default()
        }
    }
    fn suppressed(&self, port: ClockInputPort) -> bool {
        propagation_suppressed(port, self.inputs[1].measurement.flags)
    }

    /// SYS 026edc/026f0c arm start or accept continue before the next clock.
    /// These handlers check internal mode, independently of the port selector.
    pub fn arm_transport(&mut self, port: ClockInputPort, start: bool) {
        if self.settings & 12 != 4 {
            let input = &mut self.inputs[port_index(port)];
            input.input_timeout = 500;
            if start {
                input.measurement.flags |= 128;
            }
            input.measurement.flags &= !4;
        }
    }

    /// SYS 026f3c emits stop callbacks without resetting LFO phases or counts.
    pub fn stop_transport(&self, port: ClockInputPort) -> ClockActions {
        ClockActions {
            stop: self.settings & 12 != 4 && !self.suppressed(port),
            ..Default::default()
        }
    }

    /// Original 026d78 orchestration; arithmetic stays in the domain. Sequence,
    /// forwarding and effect applications consume the returned notifications.
    pub fn observe_clock(
        &mut self,
        port: ClockInputPort,
        countdown: u32,
        bank: &mut ControllerClockBank,
        divisions: &TempoDivisions,
        tables: &LfoTempoTables,
    ) -> ClockActions {
        let mut actions = ClockActions::default();
        if !accepts_clock(self.settings, port) {
            return actions;
        }
        let index = port_index(port);
        let suppressed = self.suppressed(port);
        self.inputs[index].input_timeout = 500;
        if self.inputs[index].measurement.flags & 128 != 0 {
            self.inputs[index].measurement.flags &= !128;
            if !suppressed {
                actions.restart = true;
                // 02b37c's byte is context A +78, regardless of input port.
                self.inputs[0].measurement.flags |= 64;
                self.pulse_position = 0;
                self.beat_phase = 0;
                bank.restart_shared();
            }
        }
        let measured = self.inputs[index].measurement.observe(countdown);
        if suppressed {
            return actions;
        }
        actions.pulse = true;
        self.pulse_position = (self.pulse_position & !3).wrapping_add(4);
        let phase = (self.beat_phase & !3) as u16 + 4;
        self.beat_phase = (if phase >= 96 { phase - 96 } else { phase }) as u8;
        bank.pulse(ClockPulse::ExternalFour);
        let input = &mut self.inputs[index];
        if input.measurement.flags & 2 != 0 {
            input.measurement.smooth_interval(measured);
            input
                .history
                .observe(input.measurement.interval, input.acceptance_delay);
            let raw_tempo = interval_to_tenths_bpm(input.measurement.interval);
            // The original global report word is sign-extended for comparison.
            if raw_tempo != self.reported_tenths_bpm as i16 as i32 as u32 {
                self.reported_tenths_bpm = raw_tempo as u16;
                self.tempo = TempoSetting::clamped(raw_tempo);
                bank.compile_rates(divisions, self.tempo, tables);
                if self.source_mode & 3 == 0 && self.controller_mode != 2 {
                    self.notification.request(self.tempo.tenths_bpm() as u32);
                }
                actions.tempo_changed = true;
            }
            input.measurement.flags |= 4;
        }
        actions
    }

    /// Original 029344 accepted-history notification, separate from the LFO
    /// tempo setting that is updated immediately by each measured clock.
    pub fn deliver_history(&mut self, port: ClockInputPort, expected_mode: u32) {
        let input = &mut self.inputs[port_index(port)];
        if let Some(tempo) = input.history.take_accepted_tempo(
            self.source_mode,
            expected_mode,
            input.measurement.flags,
            self.controller_mode,
        ) {
            self.notification.request(tempo as u32);
        }
    }
}

impl ControllerClockBank {
    /// Original 01815c resets sixteen shared timbre/effect LFOs and the
    /// global LFO on transport restart. Private voice LFOs retain their state.
    pub fn restart_shared(&mut self) {
        for state in self
            .timbres
            .iter_mut()
            .flatten()
            .chain(core::iter::once(&mut self.global))
        {
            state.reset_clock();
        }
    }
    /// The LFO rate stores performed by 0173a6 through 0173fc/017510/01763c.
    /// Global propagation deliberately uses offset zero for every physical
    /// voice, regardless of activity. Other per-control updates are separate.
    pub fn compile_rates(
        &mut self,
        divisions: &TempoDivisions,
        setting: TempoSetting,
        tables: &LfoTempoTables,
    ) {
        let states = self
            .voices
            .iter_mut()
            .flatten()
            .chain(self.timbres.iter_mut().flatten())
            .chain(core::iter::once(&mut self.global));
        let divisions = divisions
            .voices
            .iter()
            .flatten()
            .chain(divisions.timbres.iter().flatten())
            .chain(core::iter::once(&divisions.global));
        for (state, division) in states.zip(divisions) {
            state.previous_increment = tables
                .compile_increment((division & 31) as i32, 0, setting.clock_rate())
                .1;
        }
    }
    /// Original 017ce8/017db2 visit all 65 physical states, in this order.
    pub fn pulse(&mut self, pulse: ClockPulse) {
        for state in self
            .voices
            .iter_mut()
            .flatten()
            .chain(self.timbres.iter_mut().flatten())
            .chain(core::iter::once(&mut self.global))
        {
            match pulse {
                ClockPulse::ExternalFour => state.clock_pulse_four(),
                ClockPulse::TimerOne => state.clock_pulse_one(),
            }
        }
    }
}
