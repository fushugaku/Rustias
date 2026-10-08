//! Browser-only PCM source; all processing uses the native Rust DSP kernels.
use radias_synth_application::{
    amplifier::{AmplifierController, ControllerTables},
    clock::InstrumentClock,
    comb::CombVoiceControl,
    drum_program::DrumInstrumentProgram,
    modulation::{VoiceModulation, VoiceModulationTables},
    note_pitch::VoiceNotePitch,
    scale_bus,
    voice_envelopes::VoiceEnvelopes,
};
use radias_synth_domain::{
    Sample,
    comb::{Comb, CombDelay, CombFeedback},
    control_slew::SlewWeights,
    controller_comb::CombControlTables,
    controller_filter::ControllerFilterTables,
    controller_filter2::Filter2ControlTables,
    controller_pan::{PanControl, PanTables},
    envelope::EnvelopeLevel,
    filter_routing::{DualFilterGraph, Filter2Frame},
    fixed::{multiply_q15, saturate},
    mixer::OscillatorMix,
    modulation::ControllerSources,
    note_pitch::ScaleContext,
    pan::{self, PanSmoother, StereoFrame},
    pitch::PhaseIncrement,
    waveshaper::{ShaperParameters, ShaperPosition, ShaperSignal, ShaperTables, Waveshaper},
};
use radias_synth_infrastructure::{standalone::StandaloneSynth, standalone_tables as tables};
use std::rc::Rc;

pub const MAX_FRAMES: usize = 48_000 * 30;
const VOICES: usize = 24;
const SLEW: SlewWeights = SlewWeights {
    target: 0x1d4,
    memory: 0x7e2d,
};

struct SampleVoice {
    instrument: usize,
    data: Rc<Vec<f32>>,
    mode: u8,
    position: f64,
    step: f64,
    pitch: u16,
    increment: PhaseIncrement,
    relative_pitch: i16,
    velocity: u8,
    released: bool,
    born: u64,
    amplitude: AmplifierController,
    envelopes: VoiceEnvelopes,
    modulation: VoiceModulation,
    filter: DualFilterGraph,
    first: radias_synth_domain::filter::FilterCoefficients,
    second: radias_synth_domain::filter_routing::Filter2Coefficients,
    shaper: Waveshaper,
    shaper_parameters: Option<ShaperParameters>,
    envelope: EnvelopeLevel,
    pan: PanSmoother,
    comb: Box<Comb>,
}

pub struct Sampler {
    samples: [Option<Rc<Vec<f32>>>; 16],
    modes: [u8; 16],
    pending: Vec<f32>,
    pending_instrument: usize,
    voices: [Option<SampleVoice>; VOICES],
    programs: [DrumInstrumentProgram; 16],
    controllers: Box<ControllerTables>,
    filter_tables: ControllerFilterTables,
    filter2_tables: Filter2ControlTables,
    comb_tables: Box<CombControlTables>,
    modulation_tables: Box<VoiceModulationTables>,
    note_tables: Box<radias_synth_domain::note_pitch::NotePitchTables>,
    pan_tables: PanTables,
    shaper_tables: ShaperTables,
    clock: InstrumentClock,
    seed: u16,
    frames: u64,
}
impl Sampler {
    pub fn new(synth: &StandaloneSynth) -> Self {
        Self {
            samples: Default::default(),
            modes: [0; 16],
            pending: Vec::new(),
            pending_instrument: 0,
            voices: std::array::from_fn(|_| None),
            programs: std::array::from_fn(|i| synth.drum_program(i)),
            controllers: Box::new(tables::controllers()),
            filter_tables: tables::filter(),
            filter2_tables: tables::filter2(),
            comb_tables: Box::new(tables::comb()),
            modulation_tables: Box::new(tables::modulation()),
            note_tables: Box::new(tables::note_pitch()),
            pan_tables: PanTables {
                targets: std::array::from_fn(|i| (i as u32 * 32697 / 127) as u16),
            },
            shaper_tables: ShaperTables {
                sub_edges: [0; 129],
            },
            clock: InstrumentClock::internal(tables::tempo(), synth.settings[0][89] as u16),
            seed: 0x2345,
            frames: 0,
        }
    }
    pub fn buffer(&mut self, instrument: usize, frames: usize) -> *mut f32 {
        if instrument >= 16 || !(2..=MAX_FRAMES).contains(&frames) {
            return std::ptr::null_mut();
        }
        self.pending_instrument = instrument;
        self.pending.resize(frames, 0.0);
        self.pending.as_mut_ptr()
    }
    pub fn commit(&mut self, instrument: usize, frames: usize, mode: u8) -> bool {
        if instrument >= 16
            || instrument != self.pending_instrument
            || frames != self.pending.len()
            || !(2..=MAX_FRAMES).contains(&frames)
            || mode > 2
            || self.pending.iter().any(|v| !v.is_finite())
        {
            return false;
        }
        for v in &mut self.pending {
            *v = v.clamp(-1.0, 1.0);
        }
        self.clear(instrument);
        self.samples[instrument] = Some(Rc::new(std::mem::take(&mut self.pending)));
        self.modes[instrument] = mode;
        true
    }
    pub fn clear(&mut self, instrument: usize) {
        if instrument >= 16 {
            return;
        }
        self.samples[instrument] = None;
        for voice in &mut self.voices {
            if voice.as_ref().is_some_and(|v| v.instrument == instrument) {
                *voice = None;
            }
        }
    }
    pub fn set_mode(&mut self, instrument: usize, mode: u8) -> bool {
        if instrument >= 16 || mode > 2 {
            return false;
        }
        self.modes[instrument] = mode;
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.instrument == instrument)
        {
            voice.mode = mode;
        }
        true
    }
    pub fn assigned(&self, instrument: usize) -> bool {
        self.samples[instrument].is_some()
    }
    pub fn active_count(&self) -> usize {
        self.voices.iter().flatten().count()
    }
    pub fn stop(&mut self) {
        for voice in &mut self.voices {
            *voice = None;
        }
    }
    pub fn choke(&mut self, synth: &StandaloneSynth, instrument: usize) {
        let group = synth.drum_settings[instrument][147];
        if group != 0 {
            for voice in &mut self.voices {
                if voice
                    .as_ref()
                    .is_some_and(|v| synth.drum_settings[v.instrument][147] == group)
                {
                    *voice = None;
                }
            }
        }
    }
    pub fn sync(&mut self, synth: &StandaloneSynth) {
        self.programs = std::array::from_fn(|i| synth.drum_program(i));
        let owner = synth.settings[0][141] as usize;
        if synth.settings[0][140] == 0 || synth.settings[owner][71] == 0 {
            self.stop();
        }
        self.clock.set_program_tempo(synth.settings[0][89] as u16);
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            if let Some(v) = voice {
                let program = self.programs[v.instrument];
                v.amplitude.edit_program(
                    program.controls.amplifier(
                        synth.engine.source_gain(owner as u8),
                        Some(synth.settings[0][143] as u8),
                        0,
                    ),
                    &self.controllers,
                );
                v.envelopes.edit(
                    [program.controls.envelope[0], program.controls.envelope[2]],
                    &self.controllers,
                );
                v.envelopes.filter = Some(program.graph.dynamic_filter);
                let _ = v
                    .modulation
                    .edit_with_clock(program.controls.modulation, true);
                self.clock.divisions.voices[slot] = program.controls.modulation.tempo_divisions;
                self.clock.bank.voices[slot]
                    .iter_mut()
                    .zip(program.controls.modulation.tempo_divisions)
                    .for_each(|(s, d)| {
                        s.previous_increment = self
                            .clock
                            .tables
                            .compile_increment(d as i32, 0, self.clock.receiver.tempo.clock_rate())
                            .1;
                    });
            }
        }
    }
    pub fn trigger(&mut self, synth: &StandaloneSynth, instrument: usize, velocity: u8) {
        if velocity == 0 {
            for voice in self
                .voices
                .iter_mut()
                .flatten()
                .filter(|v| v.instrument == instrument && v.mode != 0 && !v.released)
            {
                voice.released = true;
                voice.amplitude.release(&self.controllers);
                voice.envelopes.release(&self.controllers);
            }
            return;
        }
        let owner = synth.settings[0][141] as usize;
        if synth.settings[0][140] == 0 || synth.settings[owner][71] == 0 {
            return;
        }
        let Some(data) = self.samples[instrument].clone() else {
            return;
        };
        self.choke(synth, instrument);
        let slot = self
            .voices
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.as_ref().unwrap().born)
                    .unwrap()
                    .0
            });
        let p = self.programs[instrument];
        let note =
            radias_synth_domain::note_pitch::fold_note(60 + p.controls.pitch.transpose as i32 - 64);
        let pitch = note as u16 * 256;
        let modulation = VoiceModulation::from_prior_with_clock(
            p.controls.modulation,
            (pitch as i32) << 8,
            [Default::default(); 2],
            [Default::default(); 2],
            &mut self.seed,
            true,
        )
        .unwrap();
        self.clock.divisions.voices[slot] = p.controls.modulation.tempo_divisions;
        for (state, division) in self.clock.bank.voices[slot]
            .iter_mut()
            .zip(p.controls.modulation.tempo_divisions)
        {
            state.initialize_note(64, division, Default::default());
            state.previous_increment = self
                .clock
                .tables
                .compile_increment(division as i32, 0, self.clock.receiver.tempo.clock_rate())
                .1;
        }
        self.voices[slot] = Some(SampleVoice {
            instrument,
            data,
            mode: self.modes[instrument],
            position: 0.0,
            step: 1.0,
            pitch,
            increment: PhaseIncrement(0),
            relative_pitch: (note as i16 - 60) * 256,
            velocity,
            released: false,
            born: self.frames,
            amplitude: AmplifierController::from_program(
                p.controls.amplifier(
                    synth.engine.source_gain(owner as u8),
                    Some(synth.settings[0][143] as u8),
                    0,
                ),
                note,
                velocity,
                &self.controllers,
            ),
            envelopes: VoiceEnvelopes::new(
                [p.controls.envelope[0], p.controls.envelope[2]],
                Some(p.graph.dynamic_filter),
                note,
                velocity,
                &self.controllers,
            ),
            modulation,
            filter: Default::default(),
            first: p.graph.filter,
            second: p.graph.filter2,
            shaper: Default::default(),
            shaper_parameters: p.controls.shaper.parameters_with_pitch(pitch),
            envelope: Default::default(),
            pan: PanSmoother {
                current: 16384,
                target: 16384,
            },
            comb: Box::new(Comb {
                feedback: CombFeedback::default(),
                delay: CombDelay::default(),
            }),
        });
        self.update_voice(slot, synth, true);
    }
    fn update_voice(&mut self, slot: usize, synth: &StandaloneSynth, initial: bool) {
        let Some(v) = &mut self.voices[slot] else {
            return;
        };
        let p = self.programs[v.instrument];
        let owner = synth.settings[0][141] as usize;
        let settings = synth.settings[owner];
        let aux = v.envelopes.levels();
        let lfo = v.modulation.pair.values(&self.modulation_tables.lfo);
        let sources = ControllerSources {
            envelope_levels: [aux[0], v.amplitude.envelope.segment.level, aux[1]],
            envelope_velocity_sensitivity: p
                .controls
                .envelope
                .map(|e| e.velocity_level_sensitivity),
            lfo,
            velocity: v.velocity,
            bend: settings[137] as i16,
            wheel: settings[138] as u8,
            relative_pitch: v.relative_pitch,
            auxiliary: 0,
        }
        .normalized(&self.controllers.amplifier);
        let mut inputs = [0; 16];
        inputs[..10].copy_from_slice(&sources);
        let targets = v
            .modulation
            .service_published(&self.modulation_tables.matrix, inputs, lfo);
        let note_tables = &self.note_tables;
        let c = p.controls.pitch;
        let scale = ScaleContext {
            selection: synth.settings[0][122] as u8 | ((synth.settings[0][123] as u8) << 4),
            global_transpose: Some(synth.settings[0][124] as i8),
            custom_cents: std::array::from_fn(|i| synth.settings[0][125 + i] as i8),
        };
        let note = c
            .initialize(60, scale, note_tables, &mut self.seed)
            .unwrap();
        v.modulation.base_pitch_q16 =
            VoiceNotePitch { note }.drum_base(c, note_tables, synth.settings[0][121] * 65536 / 100);
        v.modulation.vibrato_depth = c.vibrato_depth(settings[138] as u8, &note_tables.vibrato);
        v.pitch = v.modulation.pitch_code_published(lfo);
        v.increment = self
            .modulation_tables
            .pitch
            .increment(radias_synth_domain::pitch::PitchCode::new(v.pitch).unwrap());
        let root = self
            .modulation_tables
            .pitch
            .increment(radias_synth_domain::pitch::PitchCode::new(60 * 256).unwrap());
        v.step = (v.increment.0 as f64 / root.0 as f64).clamp(1.0 / 64.0, 64.0);
        v.amplitude.modulations(
            [targets.controls[13], targets.controls[9]],
            &self.controllers,
        );
        let first = v
            .envelopes
            .filter_target_with_pitch(
                &self.filter_tables,
                &self.controllers,
                [
                    targets.controls[5],
                    targets.controls[15],
                    targets.controls[16],
                ],
                v.relative_pitch,
            )
            .unwrap();
        if initial {
            v.first = first;
        } else {
            SLEW.filter(&mut v.first, first);
        }
        let input = CombVoiceControl {
            eg1_level: aux[0],
            velocity: v.velocity,
            eg1_velocity_sensitivity: p.controls.envelope[0].velocity_level_sensitivity,
            relative_pitch: v.relative_pitch,
            modulation: [
                targets.controls[7],
                targets.controls[17],
                targets.controls[18],
                targets.controls[19],
            ],
        };
        v.second = if let Some(comb) = p.graph.comb {
            comb.for_voice(input, &self.filter_tables)
                .coefficients(&self.comb_tables, &self.controllers.amplifier)
        } else {
            p.graph
                .dynamic_filter2
                .and_then(|f| {
                    f.coefficients(
                        input,
                        &self.filter_tables,
                        &self.filter2_tables,
                        &self.controllers.amplifier,
                    )
                })
                .unwrap_or(p.graph.filter2)
        };
        let mut shaper = p.controls.shaper;
        shaper.control.modulation = targets.controls[8];
        let mut next = shaper.parameters_with_pitch(v.pitch);
        if let (Some(old), Some(new)) = (v.shaper_parameters, &mut next) {
            if std::mem::discriminant(&old.coefficients)
                == std::mem::discriminant(&new.coefficients)
            {
                if let Some(gain) = old.coefficients.gain_current() {
                    new.coefficients.set_gain_current(gain);
                }
            }
        }
        v.shaper_parameters = next;
        v.pan.target = self.pan_tables.compile(
            PanControl {
                position: p.controls.pan,
                modulation: targets.controls[10],
                midi_pan: Some(synth.settings[0][144] as u8),
                ..Default::default()
            }
            .target(),
        ) as i16;
    }
    pub fn sample(&mut self, synth: &StandaloneSynth) -> StereoFrame {
        self.clock.next_audio_frame();
        if self.frames.is_multiple_of(24) {
            self.clock.controller_service();
        }
        let mut bus = StereoFrame::default();
        for slot in 0..VOICES {
            if self.frames.is_multiple_of(96) {
                if let Some(v) = &mut self.voices[slot] {
                    v.modulation.pair.tick_with_tempo(
                        &self.modulation_tables.lfo,
                        &self.clock.tables,
                        v.modulation.tempo_divisions,
                        &mut self.clock.bank.voices[slot],
                        &mut self.seed,
                    );
                }
            }
            if self.frames.is_multiple_of(24) {
                self.update_voice(slot, synth, false);
            }
            let Some(v) = &mut self.voices[slot] else {
                continue;
            };
            if v.amplitude.finished() || (v.mode != 2 && v.position >= v.data.len() as f64) {
                self.voices[slot] = None;
                continue;
            }
            let length = v.data.len();
            if v.mode == 2 {
                v.position %= length as f64;
            }
            let index = v.position as usize;
            let next = if v.mode == 2 {
                (index + 1) % length
            } else {
                (index + 1).min(length - 1)
            };
            let input = v.data[index] as f64
                + (v.data[next] - v.data[index]) as f64 * (v.position - index as f64);
            v.position += v.step;
            let input = Sample((input * 2147483647.0) as i32);
            let increment = v.increment;
            if let Some(shaper) = &mut v.shaper_parameters {
                if let (Some(current), Some(target)) = (
                    shaper.coefficients.gain_current(),
                    shaper.coefficients.gain_target(v.pitch),
                ) {
                    shaper
                        .coefficients
                        .set_gain_current(SLEW.word(current, target));
                }
            }
            let shaper = v.shaper_parameters;
            let mut transform = |input| {
                if let Some(s) = shaper.filter(|s| s.position == ShaperPosition::PreFilter) {
                    v.shaper.process(
                        ShaperSignal {
                            input,
                            primary_pitch_code: v.pitch,
                            primary_increment: increment,
                        },
                        s.coefficients,
                        &self.shaper_tables,
                    )
                } else {
                    input
                }
            };
            let filtered = if let Some(route) = self.programs[v.instrument].graph.filter_routing {
                v.filter.sample_with_prefilter_and_comb(
                    route,
                    OscillatorMix {
                        primary_gain: 32767,
                        secondary_gain: 0,
                        noise_gain: 0,
                    },
                    v.first,
                    Filter2Frame {
                        coefficients: v.second,
                        comb: Some(&mut v.comb),
                    },
                    (input, Sample(0), 0, 0),
                    transform,
                )
            } else {
                v.filter.first.next_sample(transform(input), v.first)
            };
            let filtered = if let Some(s) = shaper.filter(|s| s.position == ShaperPosition::PreAmp)
            {
                v.shaper.process(
                    ShaperSignal {
                        input: filtered,
                        primary_pitch_code: v.pitch,
                        primary_increment: increment,
                    },
                    s.coefficients,
                    &self.shaper_tables,
                )
            } else {
                filtered
            };
            v.envelopes.next(&self.controllers);
            let level = v
                .envelope
                .step(v.amplitude.next_target(&self.controllers), 0x1d4);
            bus = pan::route(
                Sample(saturate(multiply_q15(filtered.0, level))),
                v.pan.next(SLEW),
                bus,
            );
        }
        self.frames = self.frames.wrapping_add(1);
        scale_bus(bus)
    }
}
