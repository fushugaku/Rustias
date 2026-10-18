use radias_synth_application::polyphony::TIMBRE_COUNT;
// Browser-only PCM source; all processing uses the native Rust DSP kernels.
use radias_synth_application::{
    amplifier::{AmplifierController, AmplifierProgram, ControllerTables},
    clock::InstrumentClock,
    comb::CombVoiceControl,
    drum_program::DrumInstrumentProgram,
    lfo::LFO_COUNT,
    modulation::{SOURCE_COUNT, VoiceModulation, VoiceModulationTables},
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
use radias_synth_infrastructure::{
    standalone::{StandaloneSynth, Values, default_values},
    standalone_tables as tables,
};
use std::{collections::BTreeMap, rc::Rc};

pub const MAX_FRAMES: usize = 48_000 * 30;
const VOICES: usize = radias_synth_domain::voice_allocation::VOICE_COUNT;
const SLEW: SlewWeights = SlewWeights {
    target: 0x1d4,
    memory: 0x7e2d,
};

struct SampleVoice {
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    motion: radias_synth_infrastructure::circuit::motion::VoiceMotion,
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    motion_start: u64,
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    motion_envelopes: bool,
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    model_values: [f64;6],
    circuit: Option<Box<dyn radias_synth_application::VoiceCircuit>>,
    instrument: usize,
    library: Option<u32>,
    timbre: u8,
    settings: Values,
    program: DrumInstrumentProgram,
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
    midi_volume_gain: u16,
    envelopes: VoiceEnvelopes,
    modulation: VoiceModulation,
    filter: DualFilterGraph,
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    modeled: [radias_synth_infrastructure::circuit::filters::Model;2],
    first: radias_synth_domain::filter::FilterCoefficients,
    second: radias_synth_domain::filter_routing::Filter2Coefficients,
    shaper: Waveshaper,
    shaper_parameters: Option<ShaperParameters>,
    envelope: EnvelopeLevel,
    pan: PanSmoother,
    comb: Box<Comb>,
}

struct LibrarySample {
    data: Rc<Vec<f32>>,
    timbre: u8,
    mode: u8,
    settings: Values,
    program: DrumInstrumentProgram,
}

pub struct Sampler {
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    motion: [std::sync::Arc<radias_synth_application::motion::Program>;TIMBRE_COUNT],
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    motion_tempo: Option<f64>,
    circuits: [Option<Box<dyn radias_synth_application::VoiceCircuit>>; TIMBRE_COUNT],
    samples: [Option<Rc<Vec<f32>>>; 16],
    modes: [u8; 16],
    pending: Vec<f32>,
    pending_instrument: usize,
    pending_library: Option<u32>,
    assets: BTreeMap<u32, Rc<Vec<f32>>>,
    library: BTreeMap<u32, LibrarySample>,
    gain: f64,
    #[cfg(rustias_plugin)]
    voices: Box<[Option<SampleVoice>]>,
    #[cfg(not(rustias_plugin))]
    voices: [Option<SampleVoice>; VOICES],
    render_slots: u128,
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
    birth: u64,
}
impl Sampler {
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    pub fn motion_tempo(&mut self,tempo:f64) {
        self.motion_tempo=Some(tempo);
        for circuit in self.circuits.iter_mut().flatten() {circuit.motion_tempo(tempo);}
        for voice in self.voices.iter_mut().flatten() {if let Some(circuit)=&mut voice.circuit {circuit.motion_tempo(tempo);}}
    }
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    pub fn motion_program(&mut self,timbre:usize,program:std::sync::Arc<radias_synth_application::motion::Program>) {
        self.motion[timbre]=program.clone();
        for voice in self.voices.iter_mut().flatten().filter(|v|v.timbre as usize==timbre) {
            voice.motion.configure(program.clone());
            if let Some(circuit)=&mut voice.circuit {circuit.motion_program(program.clone());}
        }
    }
    #[cfg(any(target_arch = "wasm32", rustias_plugin))]
    pub fn motion_position(&self,timbre:usize,lane:usize)->i16 {
        self.voices.iter().flatten().filter(|v|v.timbre as usize==timbre&&!v.released)
            .filter_map(|v|{let pos=v.circuit.as_ref().map_or(v.motion.positions[lane],|c|c.motion_status().1[lane]);(pos>=0).then_some((v.born,pos))})
            .min_by_key(|(born,_)|*born).map_or(-1,|(_,pos)|pos)
    }
    pub fn set_circuit(
        &mut self,
        timbre: usize,
        prototype: Option<Box<dyn radias_synth_application::VoiceCircuit>>,
    ) {
        for v in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre as usize == timbre)
        {
            match (&mut v.circuit, &prototype) {
                (Some(current), Some(next)) => current.reconfigure(&**next),
                (current, next) => *current = next.as_ref().map(|p| p.fresh()),
            }
        }
        self.circuits[timbre] = prototype;
    }

    fn amplifier_program(
        synth: &StandaloneSynth,
        owner: u8,
        settings: Values,
        program: DrumInstrumentProgram,
    ) -> AmplifierProgram {
        let source_gain = if synth.settings[0][152] == 0 {
            settings[115] as u16
        } else {
            synth.engine.source_gain(owner)
        };
        let mut amplifier =
            program
                .controls
                .amplifier(source_gain, Some(synth.settings[0][143] as u8), 0);
        amplifier.level_offset = settings[114] as i8;
        amplifier
    }
    pub fn new(synth: &StandaloneSynth) -> Self {
        Self {
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            motion: core::array::from_fn(|_|std::sync::Arc::default()),
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            motion_tempo: None,
            circuits: core::array::from_fn(|_| None),
            samples: Default::default(),
            modes: [0; 16],
            pending: Vec::new(),
            pending_instrument: 0,
            pending_library: None,
            assets: BTreeMap::new(),
            library: BTreeMap::new(),
            gain: 1.0,
            #[cfg(rustias_plugin)]
            voices: std::iter::repeat_with(|| None)
                .take(VOICES)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            #[cfg(not(rustias_plugin))]
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
            shaper_tables: tables::shapers(),
            clock: InstrumentClock::internal(tables::tempo(), synth.settings[0][89] as u16),
            render_slots: 0,
            seed: 0x2345,
            frames: 0,
            birth: 0,
        }
    }
    pub fn buffer(&mut self, instrument: usize, frames: usize) -> *mut f32 {
        if instrument >= 16 || !(2..=MAX_FRAMES).contains(&frames) {
            return std::ptr::null_mut();
        }
        self.pending_library = None;
        self.pending_instrument = instrument;
        self.pending.resize(frames, 0.0);
        self.pending.as_mut_ptr()
    }
    pub fn commit(&mut self, instrument: usize, frames: usize, mode: u8) -> bool {
        if instrument >= 16
            || self.pending_library.is_some()
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
        for voice in self.voices.iter_mut() {
            if voice
                .as_ref()
                .is_some_and(|v| v.library.is_none() && v.instrument == instrument)
            {
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
            .filter(|v| v.library.is_none() && v.instrument == instrument)
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
    pub fn stop_kit(&mut self) {
        for voice in self.voices.iter_mut() {
            if voice.as_ref().is_some_and(|v| v.library.is_none()) {
                *voice = None;
            }
        }
    }
    pub fn stop(&mut self) {
        for voice in self.voices.iter_mut() {
            *voice = None;
        }
    }
    pub fn choke(&mut self, synth: &StandaloneSynth, instrument: usize) {
        let group = synth.drum_settings[instrument][147];
        if group != 0 {
            for voice in self.voices.iter_mut() {
                if voice.as_ref().is_some_and(|v| {
                    v.timbre == synth.settings[0][141] as u8 && v.settings[147] == group
                }) {
                    *voice = None;
                }
            }
        }
    }
    pub fn set_gain(&mut self, gain: f64) {
        self.gain = gain;
    }
    pub fn steal_oldest_voice(&mut self) -> bool {
        let slot = self
            .voices
            .iter()
            .enumerate()
            .filter_map(|(slot, v)| v.as_ref().map(|v| (slot, v.born)))
            .min_by_key(|(_, born)| *born);
        if let Some((slot, _)) = slot {
            self.voices[slot] = None;
            true
        } else {
            false
        }
    }
    pub fn library_buffer(&mut self, asset: u32, frames: usize) -> *mut f32 {
        if asset == 0
            || !(2..=MAX_FRAMES).contains(&frames)
            || (self.assets.len() >= 1024 && !self.assets.contains_key(&asset))
        {
            return std::ptr::null_mut();
        }
        self.pending_library = Some(asset);
        self.pending.resize(frames, 0.0);
        self.pending.as_mut_ptr()
    }
    pub fn library_commit(&mut self, asset: u32, frames: usize) -> bool {
        if self.pending_library != Some(asset)
            || frames != self.pending.len()
            || !(2..=MAX_FRAMES).contains(&frames)
            || self.pending.iter().any(|v| !v.is_finite())
        {
            return false;
        }
        for v in &mut self.pending {
            *v = v.clamp(-1.0, 1.0);
        }
        self.assets
            .insert(asset, Rc::new(std::mem::take(&mut self.pending)));
        self.pending_library = None;
        true
    }
    pub fn library_profile(
        &mut self,
        synth: &StandaloneSynth,
        id: u32,
        asset: u32,
        timbre: u8,
        mode: u8,
    ) -> bool {
        if id == 0
            || timbre as usize >= TIMBRE_COUNT
            || mode > 2
            || (self.library.len() >= 1024 && !self.library.contains_key(&id))
        {
            return false;
        }
        let Some(data) = self.assets.get(&asset).cloned() else {
            return false;
        };
        let settings = self
            .library
            .get(&id)
            .map_or_else(default_values, |p| p.settings);
        self.library.insert(
            id,
            LibrarySample {
                data,
                timbre,
                mode,
                settings,
                program: synth.compile_drum_values(&settings),
            },
        );
        true
    }
    pub fn library_owned(&self, id: u32, timbre: u8) -> bool {
        self.library.get(&id).is_some_and(|p| p.timbre == timbre)
    }
    pub fn library_value(&self, id: u32, parameter: usize) -> i32 {
        self.library
            .get(&id)
            .and_then(|p| p.settings.get(parameter))
            .copied()
            .unwrap_or(0)
    }
    pub fn library_control(
        &mut self,
        synth: &StandaloneSynth,
        id: u32,
        parameter: usize,
        value: i32,
    ) -> bool {
        if !synth.valid_parameter(parameter, value) {
            return false;
        }
        let Some(profile) = self.library.get_mut(&id) else {
            return false;
        };
        if parameter == 10 && value != 0 && profile.settings[0] >= 4 {
            return false;
        }
        #[cfg(any(target_arch = "wasm32", rustias_plugin))]
        if parameter == 172 && value != 0 && profile.settings[171] >= 4 {
            return false;
        }
        if parameter == 0 && value >= 4 {
            profile.settings[10] = 0;
        }
        #[cfg(any(target_arch = "wasm32", rustias_plugin))]
        if parameter == 171 && value >= 4 {
            profile.settings[172] = 0;
        }
        profile.settings[parameter] = value;
        profile.program = synth.compile_drum_values(&profile.settings);
        true
    }
    pub fn library_reset(&mut self) {
        for voice in self.voices.iter_mut() {
            if voice.as_ref().is_some_and(|v| v.library.is_some()) {
                *voice = None;
            }
        }
        self.library.clear();
        self.assets.clear();
    }
    pub fn library_midi(&mut self, timbre: u8, cc: u8, value: u8) {
        if cc == 7 {
            for profile in self
                .library
                .values_mut()
                .filter(|p| p.timbre == timbre && p.settings[116] != 0)
            {
                profile.settings[117] = value as i32;
            }
        }
        for voice in self.voices.iter_mut() {
            if let Some(v) = voice
                .as_mut()
                .filter(|v| v.library.is_some() && v.timbre == timbre)
            {
                if cc == 120 {
                    *voice = None;
                } else if cc == 123 && v.mode != 0 {
                    v.released = true;
                    v.amplitude.release(&self.controllers);
                    v.envelopes.release(&self.controllers);
                }
            }
        }
    }
    pub fn sync(&mut self, synth: &StandaloneSynth) {
        self.programs = std::array::from_fn(|i| synth.drum_program(i));
        let owner = synth.settings[0][141] as usize;
        self.clock.set_program_tempo(synth.settings[0][89] as u16);
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            if let Some(v) = voice {
                if synth.settings[v.timbre as usize][71] == 0
                    || (v.library.is_none()
                        && (synth.settings[0][140] == 0 || owner != v.timbre as usize))
                {
                    *voice = None;
                    continue;
                }
                if let Some(id) = v.library {
                    let Some(profile) = self.library.get(&id) else {
                        *voice = None;
                        continue;
                    };
                    v.settings = profile.settings;
                    v.program = profile.program;
                    v.mode = profile.mode;
                } else {
                    v.settings = synth.drum_settings[v.instrument];
                    v.program = self.programs[v.instrument];
                }
                let program = v.program;
                v.amplitude.edit_program(
                    Self::amplifier_program(synth, v.timbre, v.settings, program),
                    &self.controllers,
                );
                v.midi_volume_gain = if v.settings[116] != 0 {
                    self.controllers.amplifier.midi_volume[v.settings[117] as usize]
                } else {
                    8192
                };
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
    fn release_voice(v: &mut SampleVoice, controllers: &ControllerTables) {
        if v.mode != 0 && !v.released {
            v.released = true;
            v.amplitude.release(controllers);
            v.envelopes.release(controllers);
        }
    }
    pub fn trigger(&mut self, synth: &StandaloneSynth, instrument: usize, velocity: u8) {
        if velocity == 0 {
            for v in self
                .voices
                .iter_mut()
                .flatten()
                .filter(|v| v.library.is_none() && v.instrument == instrument)
            {
                Self::release_voice(v, &self.controllers);
            }
            return;
        }
        let owner = synth.settings[0][141] as u8;
        if synth.settings[0][140] == 0 || synth.settings[owner as usize][71] == 0 {
            return;
        }
        let Some(data) = self.samples[instrument].clone() else {
            return;
        };
        self.choke(synth, instrument);
        self.start_voice(
            synth,
            instrument,
            None,
            owner,
            self.modes[instrument],
            synth.drum_settings[instrument],
            self.programs[instrument],
            data,
            velocity,
        );
    }
    pub fn library_trigger(
        &mut self,
        synth: &StandaloneSynth,
        timbre: u8,
        id: u32,
        velocity: u8,
    ) -> bool {
        let Some(profile) = self.library.get(&id).filter(|p| p.timbre == timbre) else {
            return false;
        };
        if velocity == 0 {
            for v in self
                .voices
                .iter_mut()
                .flatten()
                .filter(|v| v.library == Some(id) && v.timbre == timbre)
            {
                Self::release_voice(v, &self.controllers);
            }
            return true;
        }
        if synth.settings[timbre as usize][71] == 0 {
            return true;
        }
        let (settings, program, mode, data) = (
            profile.settings,
            profile.program,
            profile.mode,
            profile.data.clone(),
        );
        if settings[147] != 0 {
            for voice in self.voices.iter_mut() {
                if voice
                    .as_ref()
                    .is_some_and(|v| v.timbre == timbre && v.settings[147] == settings[147])
                {
                    *voice = None;
                }
            }
        }
        self.start_voice(
            synth,
            0,
            Some(id),
            timbre,
            mode,
            settings,
            program,
            data,
            velocity,
        );
        true
    }
    #[allow(clippy::too_many_arguments)]
    fn start_voice(
        &mut self,
        synth: &StandaloneSynth,
        instrument: usize,
        library: Option<u32>,
        timbre: u8,
        mode: u8,
        settings: Values,
        p: DrumInstrumentProgram,
        data: Rc<Vec<f32>>,
        velocity: u8,
    ) {
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
        let note =
            radias_synth_domain::note_pitch::fold_note(60 + p.controls.pitch.transpose as i32 - 64);
        let pitch = note as u16 * 256;
        let modulation = VoiceModulation::from_prior_with_clock(
            p.controls.modulation,
            (pitch as i32) << 8,
            [Default::default(); LFO_COUNT],
            [Default::default(); LFO_COUNT],
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
        self.birth = self.birth.wrapping_add(1);
        let mut circuit=self.circuits[timbre as usize].as_ref().map(|p|p.fresh());
        #[cfg(any(target_arch = "wasm32", rustias_plugin))]
        if let Some(c)=&mut circuit {c.extension_parameters(&settings);c.motion_program(self.motion[timbre as usize].clone());}
        self.voices[slot] = Some(SampleVoice {
            circuit,
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            motion: {let mut motion=radias_synth_infrastructure::circuit::motion::VoiceMotion::default();motion.configure(self.motion[timbre as usize].clone());motion.seed=(self.birth as u32).max(1);motion},
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            motion_start: self.frames,
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            motion_envelopes: false,
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            model_values: [0.0;6],
            instrument,
            library,
            timbre,
            settings,
            program: p,
            data,
            mode,
            position: 0.0,
            step: 1.0,
            pitch,
            increment: PhaseIncrement(0),
            relative_pitch: (note as i16 - 60) * 256,
            velocity,
            released: false,
            born: self.birth,
            amplitude: AmplifierController::from_program(
                Self::amplifier_program(synth, timbre, settings, p),
                note,
                velocity,
                &self.controllers,
            ),
            midi_volume_gain: if settings[116] != 0 {
                self.controllers.amplifier.midi_volume[settings[117] as usize]
            } else {
                8192
            },
            envelopes: VoiceEnvelopes::new(
                [p.controls.envelope[0], p.controls.envelope[2]],
                Some(p.graph.dynamic_filter),
                note,
                velocity,
                &self.controllers,
            ),
            modulation,
            filter: Default::default(),
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            modeled: Default::default(),
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
        let p = v.program;
        v.relative_pitch = (radias_synth_domain::note_pitch::fold_note(
            60 + p.controls.pitch.transpose as i32 - 64,
        ) as i16
            - 60)
            * 256;
        let settings = synth.settings[v.timbre as usize];
        let aux = v.envelopes.levels();
        let lfo = v.modulation.pair.values(&self.modulation_tables.lfo);
        let sources = ControllerSources {
            envelope_levels: [aux[0], v.amplitude.envelope.segment.level, aux[1]],
            envelope_velocity_sensitivity: p
                .controls
                .envelope
                .map(|e| e.velocity_level_sensitivity),
            lfo: [lfo[0], lfo[1]],
            velocity: v.velocity,
            bend: settings[137] as i16,
            wheel: settings[138] as u8,
            relative_pitch: v.relative_pitch,
            auxiliary: 0,
        }
        .normalized(&self.controllers.amplifier);
        let mut inputs = [0; SOURCE_COUNT];
        inputs[..10].copy_from_slice(&sources);
        let targets = v
            .modulation
            .service_published(&self.modulation_tables.matrix, inputs, lfo);
        #[cfg(any(target_arch = "wasm32", rustias_plugin))]
        let targets={
            let targets=v.motion.tick(self.frames-v.motion_start,self.motion_tempo.unwrap_or(synth.settings[0][89] as f64/10.0),targets);
            if let Some(circuit)=&mut v.circuit {circuit.extension_parameters(&v.settings);circuit.set_motion_offsets(&v.motion.offsets,v.motion.positions);}
            v.modulation.patches.targets=targets;
            if let Some(filter)=&mut v.envelopes.filter {filter.resonance=((v.settings[2]+targets.controls[6] as i32).clamp(0,127) as f64/127.0*0.95*2147483647.0) as i32;}
            v.motion_envelopes |= v.motion.program.lanes.iter().any(|l|matches!(l.parameter,3..=6|32..=51));
            if v.motion_envelopes {
                let get=|id:usize|(v.settings[id] as f64+v.motion.offsets[id]).round().clamp(0.0,127.0) as u8;
                let programs:[radias_synth_application::voice_envelopes::ModEnvelopeProgram;3]=core::array::from_fn(|i|{let (a,b)=match i {0=>(32,40),1=>(3,44),_=>(36,48)};radias_synth_application::voice_envelopes::ModEnvelopeProgram {adsr:core::array::from_fn(|i|get(a+i)),curve:get(b).min(7),velocity_level_sensitivity:get(b+1),velocity_time_sensitivity:get(b+2),key_tracking:get(b+3)}});
                if initial {v.amplitude.initialize_motion_envelope(programs[1],&self.controllers);} else {v.amplitude.edit_motion_envelope(programs[1],&self.controllers);}
                let aux=[programs[0],programs[2]];if initial {v.envelopes.initialize_motion_envelopes(aux,&self.controllers);} else if v.envelopes.programs!=aux {v.envelopes.edit(aux,&self.controllers);}
            }
            for i in 0..6 {v.model_values[i]=v.settings[189+i] as f64+v.motion.offsets[189+i]+targets.controls[61+i] as f64;}
            targets
        };
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
        let key_track = self
            .controllers
            .amplifier
            .key_modulation(p.controls.amplifier_key_tracking, v.relative_pitch);
        v.amplitude.modulations(
            [
                (targets.controls[13] as i32 + key_track as i32).clamp(-32767, 32767) as i16,
                targets.controls[9],
            ],
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
        self.next_buses(synth, false)[0]
    }
    pub fn prepare_render(&mut self) {
        // Voice creation/control occurs between render calls. Rebuild once per
        // call so every mutation path is covered; preserve ascending slot order.
        self.render_slots = self.voices.iter().enumerate().fold(0, |mask, (slot, voice)| {
            mask | if voice.is_some() { 1u128 << slot } else { 0 }
        });
    }
    pub fn sample_buses(&mut self, synth: &StandaloneSynth) -> [StereoFrame; TIMBRE_COUNT] {
        self.next_buses(synth, true)
    }
    fn next_buses(
        &mut self,
        synth: &StandaloneSynth,
        separate: bool,
    ) -> [StereoFrame; TIMBRE_COUNT] {
        self.clock.next_audio_frame();
        if self.frames.is_multiple_of(24) {
            self.clock.controller_service();
        }
        let mut buses = [StereoFrame::default(); TIMBRE_COUNT];
        let mut slots = self.render_slots;
        while slots != 0 {
            let slot = slots.trailing_zeros() as usize;
            slots &= slots - 1;
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
            let bus = &mut buses[if separate { v.timbre as usize } else { 0 }];
            if (v.amplitude.finished() && !v.circuit.as_ref().is_some_and(|p| p.tail_active()))
                || (v.mode != 2 && v.position >= v.data.len() as f64)
            {
                self.voices[slot] = None;
                self.render_slots &= !(1u128 << slot);
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
            v.envelopes.next(&self.controllers);
            let target = ((v.amplitude.next_target(&self.controllers) as i32
                * v.midi_volume_gain as i32)
                >> 13)
                .clamp(0, 32767) as i16;
            let level = v.envelope.step(target, 0x1d4);
            if let Some(circuit) = &mut v.circuit {
                let eg = v.envelopes.levels();
                let lfo = v.modulation.pair.values(&self.modulation_tables.lfo);
                #[cfg(any(target_arch = "wasm32", rustias_plugin))]
                circuit.oscillator3(v.program.controls.third, v.modulation.patches.targets);
                circuit.controls([
                    if v.released { 0.0 } else { 1.0 },
                    v.velocity as f64 / 127.0,
                    60.0,
                    eg[0] as f64 / 65535.0,
                    eg[1] as f64 / 65535.0,
                    lfo[0] as f64 / 32768.0,
                    lfo[1] as f64 / 32768.0,
                    lfo.get(2).copied().unwrap_or(0) as f64 / 32768.0,
                ]);
                let parameters = radias_synth_domain::voice::VoiceParameters {
                    primary: radias_synth_domain::primary_oscillator::PrimaryParameters::waveform(
                        0,
                        v.increment,
                        0,
                    )
                    .unwrap(),
                    primary_pitch_code: v.pitch,
                    mix: OscillatorMix {
                        primary_gain: 32767,
                        secondary_gain: 0,
                        noise_gain: 0,
                    },
                    filter: v.first,
                    routing: v.program.graph.filter_routing.map(|route| {
                        radias_synth_domain::filter_routing::DualFilterParameters {
                            route,
                            first: v.first,
                            second: v.second,
                        }
                    }),
                    shaper: v.shaper_parameters,
                    envelope_target: target,
                    envelope_rate: 0x1d4,
                    pan_position: 0,
                    secondary_modulation: Default::default(),
                };
                let output = circuit.process(
                    [input, Sample(0), Sample(0)],
                    parameters,
                    level,
                    synth.engine.waveform_table(),
                );
                *bus = pan::route(
                    Sample(saturate((output.0 as f64 * self.gain) as i64)),
                    v.pan.next(SLEW),
                    *bus,
                );
                continue;
            }
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
            #[cfg(any(target_arch = "wasm32", rustias_plugin))]
            let modeled = v.model_values[0]>=0.5 || v.model_values[1]>=0.5;
            #[cfg(not(any(target_arch = "wasm32", rustias_plugin)))]
            let modeled = false;
            let filtered = if modeled {
                #[cfg(any(target_arch = "wasm32", rustias_plugin))]
                {
                    use radias_synth_domain::filter_routing::{FilterRouting,Filter2Output};
                    let first_input=transform(input);
                    let first=if v.model_values[0]<0.5 {v.filter.first.next_sample(first_input,v.first)} else {
                        Sample((v.modeled[0].sample_coefficient(first_input.0 as f64/2147483648.0,v.model_values[0].round().clamp(0.0,17.0) as u8,v.first.integrator_gain,(v.settings[2]+v.modulation.patches.targets.controls[6] as i32).clamp(0,127) as f64/127.0,v.model_values[2]/127.0,v.model_values[4]/127.0).clamp(-1.0,1.0)*2147483647.0) as i32)
                    };
                    if let Some(route)=v.program.graph.filter_routing {
                        let second_input=if route==FilterRouting::Serial {first} else if route==FilterRouting::Individual {Sample(0)} else {input};
                        let second=if v.model_values[1]>=0.5 {
                            Sample((v.modeled[1].sample_coefficient(second_input.0 as f64/2147483648.0,v.model_values[1].round().clamp(0.0,17.0) as u8,v.second.integrator_gain,(v.settings[23]+v.modulation.patches.targets.controls[17] as i32).clamp(0,127) as f64/127.0,v.model_values[3]/127.0,v.model_values[5]/127.0).clamp(-1.0,1.0)*2147483647.0) as i32)
                        } else if v.second.output==Filter2Output::Comb {v.comb.sample(second_input,v.second.feedback,v.second.integrator_gain as u32)} else {v.filter.second.sample(second_input,v.second)};
                        match route {FilterRouting::Serial=>second,FilterRouting::Parallel=>Sample(saturate((first.0 as i64+second.0 as i64)>>1)),FilterRouting::Individual=>Sample(saturate(first.0 as i64+second.0 as i64))}
                    } else {first}
                }
                #[cfg(not(any(target_arch = "wasm32", rustias_plugin)))]
                {input}
            } else if let Some(route) = v.program.graph.filter_routing {
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
            *bus = pan::route(
                Sample(saturate(
                    (multiply_q15(filtered.0, level) as f64 * self.gain) as i64,
                )),
                v.pan.next(SLEW),
                *bus,
            );
        }
        self.frames = self.frames.wrapping_add(1);
        buses.map(scale_bus)
    }
}
