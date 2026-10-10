//! Complete firmware-free composition of the native engine's data ports.
use crate::{
    prepared::PreparedVoice,
    standalone_tables as tables,
    synthesizer::{Command, Synthesizer},
};
use radias_synth_application::{
    amplifier::AmplifierProgram,
    drum_program::{CompiledDrumKit, DrumInstrumentProgram},
    mixer::MixerProgram,
    modulation::{ModulationProgram, PatchRoute},
    polyphony::TIMBRE_COUNT,
    program::TimbreControls,
    secondary::SecondaryProgram,
    stored_program::{CompiledTimbre, ProgramFilterTables},
    voice_envelopes::ModEnvelopeProgram,
};
use radias_synth_domain::{
    Phase,
    control_slew::SlewWeights,
    controller_pan::{PanControl, PanTables},
    controller_secondary::SecondaryPitch,
    drum::DrumKit,
    filter::FilterCoefficients,
    mixer::OscillatorMix,
    modulation::ModulationDestination,
    mono_notes::{NotePriority, VoiceMode},
    note_pitch::{PitchProgram, ScaleContext},
    oscillator::Oscillator,
    pan::VoiceBus,
    performance::GlobalPerformance,
    pitch::PhaseIncrement,
    portamento::PortamentoProgram,
    primary_oscillator::PrimaryParameters,
    sustain::SustainProgram,
    voice::{Voice, VoiceParameters},
    voice_group::VoiceGroupProgram,
    waveform::{ShapeParameters, Transfer, WaveformTable},
    waveshaper::ShaperPosition,
};

pub const SAMPLE_RATE: u32 = 48_000;
pub const NATIVE_PARAMETER_COUNT: usize = 155;
pub const PARAMETER_COUNT: usize = if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
    163
} else {
    NATIVE_PARAMETER_COUNT
};
pub const PARAMETER_SCHEMA: &str = include_str!("parameters.json");
pub type Values = [i32; PARAMETER_COUNT];
#[derive(Clone, serde::Deserialize)]
pub struct Parameter {
    pub id: usize,
    pub min: i32,
    pub max: i32,
    pub default: i32,
    #[serde(default)]
    pub values: Option<Vec<i32>>,
    #[serde(default)]
    pub scope: String,
}
pub fn parameters() -> Vec<Parameter> {
    let mut spec: Vec<Parameter> =
        serde_json::from_str(PARAMETER_SCHEMA).expect("Static parameter schema");
    if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
        spec[141].max = (TIMBRE_COUNT - 1) as i32;
        for i in 0..6 {
            spec[91 + i * 4].max = 41;
            spec[91 + i * 4].values.as_mut().unwrap().extend([40, 41]);
        }
        for id in NATIVE_PARAMETER_COUNT..PARAMETER_COUNT {
            let mut p = spec[90 + (id - NATIVE_PARAMETER_COUNT) % 4].clone();
            p.id = id;
            spec.push(p);
        }
    }
    spec
}
pub fn default_values() -> Values {
    let mut v = [0; PARAMETER_COUNT];
    for p in parameters() {
        v[p.id] = p.default;
    }
    v
}
pub fn envelope_seconds(value: u8) -> f64 {
    tables::seconds(value)
}
fn envelope(v: &Values, index: usize) -> ModEnvelopeProgram {
    let (a, b) = match index {
        0 => (32, 40),
        1 => (3, 44),
        _ => (36, 48),
    };
    ModEnvelopeProgram {
        adsr: core::array::from_fn(|i| v[a + i] as u8),
        curve: v[b] as u8,
        velocity_level_sensitivity: v[b + 1] as u8,
        velocity_time_sensitivity: v[b + 2] as u8,
        key_tracking: v[b + 3] as u8,
    }
}
fn modulation(v: &Values) -> ModulationProgram {
    ModulationProgram {
        lfo: core::array::from_fn(|i| {
            let b = 73 + i * 8;
            radias_synth_application::lfo::LfoParameters {
                waveform: v[b] as u8,
                shape: v[b + 1] as u8,
                frequency: v[b + 2] as u8,
                phase_sync: ([0, 32, 64][v[b + 3] as usize]
                    | v[b + 4] as u8
                    | if v[b + 5] != 0 { 128 } else { 0 }),
                frequency_offset: v[b + 7] as i8,
                frequency_modulation: 0,
            }
        }),
        tempo_divisions: [v[79] as u8, v[87] as u8],
        routes: core::array::from_fn(|i| {
            let b = if i < 6 { 90 + i * 4 } else { 155 + (i - 6) * 4 };
            PatchRoute {
                source: v[b] as u8,
                destination: ModulationDestination::new(v[b + 1] as u8).unwrap(),
                intensity: v[b + 2] as u8,
            }
        }),
        manual_offsets: core::array::from_fn(|i| {
            v[if i < 6 { 93 + i * 4 } else { 158 + (i - 6) * 4 }] as i8
        }),
        vibrato_depth: 0,
    }
}
fn controls(v: &Values) -> TimbreControls {
    let selection = v[0] as u8 | ((v[10] as u8) << 4);
    let secondary = v[13] as u8 | ((v[14] as u8) << 4);
    TimbreControls {
        voice_mode: VoiceMode {
            polyphonic: v[62] != 0,
            multi_trigger: v[63] != 0,
            priority: [
                NotePriority::Last,
                NotePriority::Lowest,
                NotePriority::Highest,
            ][v[64] as usize],
        },
        voice_group: VoiceGroupProgram {
            raw: (v[68] as u8 - 2) | if v[67] != 0 { 128 } else { 0 },
            detune: v[69] as u8,
            spread: v[70] as u8,
        },
        sustain: SustainProgram {
            enabled: v[65] != 0,
        },
        pitch: PitchProgram {
            transpose: v[53] as u8,
            fine_tune: v[54] as u8,
            vibrato_intensity: v[55] as u8,
            bend_range: v[56] as u8,
            bend_enabled: v[57] != 0,
            wheel_enabled: v[58] != 0,
        },
        portamento: PortamentoProgram {
            time: v[59] as u8,
            curve: v[60] as u8,
            switch_required: v[61] != 0,
        },
        oscillator_selection: selection,
        oscillator_controls: [v[11] as u8, v[12] as u8],
        secondary: SecondaryProgram {
            selection: secondary,
            pitch: SecondaryPitch {
                semitone: v[15] as u8,
                fine_tune: v[16] as u8,
                ..Default::default()
            },
        },
        mixer: MixerProgram {
            selections: [selection, secondary],
            levels: [v[17] as u8, v[18] as u8, v[19] as u8],
            manual_offsets: [0; 3],
        },
        filter_route: v[20] as u8 | ((v[21] as u8) << 4) | if v[24] != 0 { 128 } else { 0 },
        cutoff: [v[1] as u8, v[22] as u8],
        resonance: [v[2] as u8, v[23] as u8],
        filter_type: v[9] as u8,
        eg1_intensity: v[25] as u8,
        filter_key_tracking: v[26] as u8,
        filter2_eg_intensity: v[27] as u8,
        filter2_key_tracking: v[28] as u8,
        amplifier_level: v[7] as u8,
        amplifier_key_tracking: v[52] as u8,
        pan: v[8] as u8,
        shaper: radias_synth_application::shaper::ShaperProgram {
            mode: radias_synth_application::shaper::ShaperMode::from_allocation(
                v[29] as u8,
                v[154] as u8,
            )
            .unwrap(),
            position: if v[30] == 0 {
                ShaperPosition::PreFilter
            } else {
                ShaperPosition::PreAmp
            },
            control: radias_synth_domain::controller_shaper::ShaperControl {
                depth: v[31] as u8,
                ..Default::default()
            },
        },
        envelope: core::array::from_fn(|i| envelope(v, i)),
        modulation: modulation(v),
    }
}
fn base_filter() -> FilterCoefficients {
    FilterCoefficients {
        input_gain: 24576,
        feedback: 0,
        integrator_gain: 0,
        post_gain: 32767,
        post_feedback: 0,
        mix: [0, 0, 0, 32767, 0],
    }
}
fn plan(wave: u8) -> PreparedVoice {
    let increment = PhaseIncrement((261.625565 * 4294967296.0 / SAMPLE_RATE as f64) as u32);
    PreparedVoice {
        initial: Voice {
            primary: Default::default(),
            secondary: Oscillator::new(
                Phase(0),
                increment,
                0,
                Transfer::CorrectedRamp,
                ShapeParameters {
                    subtract_edge: false,
                    edge_coefficient: 0,
                    waveform_control: 0,
                    gain: 32767,
                },
            ),
            filter: Default::default(),
            second_filter: Default::default(),
            waveshaper: Default::default(),
            envelope: Default::default(),
            previous_secondary: Default::default(),
            previous_primary: Phase(0),
            mixer_noise: Default::default(),
        },
        parameters: VoiceParameters {
            primary: PrimaryParameters::waveform(wave, increment, 0).unwrap(),
            primary_pitch_code: 60 * 256,
            mix: OscillatorMix {
                primary_gain: 32767,
                secondary_gain: 0,
                noise_gain: 0,
            },
            filter: base_filter(),
            routing: None,
            shaper: None,
            envelope_target: 0,
            envelope_rate: 0x1d4,
            pan_position: 0x4000_0000,
            secondary_modulation: Default::default(),
        },
        events: Vec::new(),
        control_slew: SlewWeights {
            target: 0x1d4,
            memory: 0x7e2d,
        },
        reference_start_frame: 0,
        reference_voice_frames: 0,
        bus: VoiceBus::new(0).unwrap(),
    }
}
pub struct StandaloneSynth {
    pub engine: Synthesizer,
    pub settings: [Values; TIMBRE_COUNT],
    pub drum_settings: [Values; 16],
    spec: Vec<Parameter>,
    map: crate::prepared::ControlMap,
    mix: radias_synth_domain::filter_control::FilterMixTable,
}
impl Default for StandaloneSynth {
    fn default() -> Self {
        Self::new()
    }
}
impl StandaloneSynth {
    pub fn new() -> Self {
        let mut engine = Synthesizer::new(
            (0..4).map(plan).collect(),
            WaveformTable {
                correction: [0; 129],
                shapers: tables::shapers(),
            },
            Some((tables::pitch(), tables::bandwidth())),
            Some(tables::controllers()),
            None,
            Some(tables::modulation()),
            Some(tables::tempo()),
        )
        .unwrap();
        engine.apply(Command::PanTables(
            Box::new(PanTables {
                targets: core::array::from_fn(|i| (i as u32 * 32697 / 127) as u16),
            }),
            SlewWeights {
                target: 0x1d4,
                memory: 0x7e2d,
            },
        ));
        engine.apply(Command::FilterTables(Box::new(tables::filter())));
        engine.apply(Command::Filter2Tables(Box::new(tables::filter2())));
        engine.apply(Command::CombTables(Box::new(tables::comb())));
        engine.apply(Command::MixerScales(Box::new(tables::mixer())));
        engine.apply(Command::SecondaryTable(Box::new(tables::fine())));
        engine.apply(Command::NoiseTables(Box::new(tables::noise())));
        engine.apply(Command::VoiceGroupTables(Box::new(tables::groups())));
        engine.apply(Command::PortamentoTables(Box::new(tables::portamento())));
        let defaults = default_values();
        let mut settings = [defaults; TIMBRE_COUNT];
        for (i, v) in settings.iter_mut().enumerate() {
            v[72] = i as i32;
            if i >= 4 {
                v[71] = 0;
            }
        }
        let mut drum_settings = [defaults; 16];
        for (i, v) in drum_settings.iter_mut().enumerate() {
            v[146] = 60 + i as i32;
            v[3] = 0;
            v[4] = 32;
            v[5] = 0;
            v[6] = 20;
        }
        let mut out = Self {
            engine,
            settings,
            drum_settings,
            spec: parameters(),
            map: tables::control_map(),
            mix: tables::mix(),
        };
        out.global_pitch();
        out.performance();
        for i in 0..TIMBRE_COUNT as u8 {
            out.apply_timbre(i, None);
        }
        out
    }
    fn graph(&self, v: &Values) -> CompiledTimbre {
        CompiledTimbre::compile(
            controls(v),
            &ProgramFilterTables {
                frequencies: &self.map.frequencies,
                resonances: &self.map.resonances,
                input_gains: &self.map.input_gains,
                normalization: self.map.normalization,
                mix: &self.mix,
            },
            base_filter(),
        )
    }
    fn active_drums(&self, t: u8) -> bool {
        self.settings[0][140] != 0 && self.settings[0][141] == t as i32
    }
    pub fn value(&self, t: u8, id: usize) -> i32 {
        if t as usize >= TIMBRE_COUNT || id >= PARAMETER_COUNT {
            return 0;
        }
        if id == 118 {
            return if self.settings[t as usize][67] != 0 {
                self.settings[t as usize][68] - 1
            } else {
                0
            };
        }
        if self.spec[id].scope != "global"
            && self.active_drums(t)
            && !matches!(id,59..=72|114..=118|119..=120|137..=139|150..=151|153)
        {
            self.drum_settings[self.settings[0][142] as usize][id]
        } else {
            self.settings[t as usize][id]
        }
    }
    fn channel(&self, t: u8) -> u8 {
        let v = self.settings[t as usize];
        if v[72] == 16 {
            v[148] as u8
        } else {
            v[72] as u8
        }
    }
    fn apply_timbre(&mut self, t: u8, changed: Option<usize>) {
        let v = self.settings[t as usize];
        let c = controls(&v);
        let is = |ids: &[usize]| changed.is_none_or(|id| ids.contains(&id));
        if is(&[71, 72]) {
            self.engine
                .apply(Command::Timbre(t, v[71] != 0, self.channel(t)));
        }
        if is(&[119, 120]) {
            self.engine.set_key_window(t, [v[119] as u8, v[120] as u8]);
        }
        if is(&[151, 153]) {
            self.engine.set_receive_flags(
                t,
                159 | if v[151] != 0 { 64 } else { 0 } | if v[153] != 0 { 32 } else { 0 },
            );
        }
        if is(&[0, 10, 11, 12]) {
            self.engine.apply(Command::Primary(t, c.primary()));
        }
        if is(&[13, 14, 15, 16]) {
            self.engine.apply(Command::Secondary(t, c.secondary));
        }
        if is(&[0, 10, 13, 14, 17, 18, 19]) {
            self.engine.apply(Command::Mixer(t, c.mixer));
        }
        if is(&[1, 2, 9, 20, 21, 22, 23, 24, 25, 26, 27, 28]) {
            let graph = self.graph(&v);
            self.engine.apply(Command::Filter(t, graph.filter));
            self.engine
                .apply(Command::DynamicFilter(t, graph.dynamic_filter));
            self.engine.apply(Command::FilterRouting(
                t,
                graph.filter_routing,
                graph.filter2,
            ));
            if let Some(comb) = graph.comb {
                self.engine
                    .apply(Command::Comb(t, graph.filter_routing, comb));
            } else if let Some(second) = graph.dynamic_filter2 {
                self.engine.apply(Command::Filter2Program(t, second));
            }
        }
        if is(&[29, 30, 31, 154]) {
            self.engine.apply(Command::Shaper(t, c.shaper));
        }
        if changed.is_none_or(|id| matches!(id,32..=43|48..=51)) {
            self.engine
                .apply(Command::Auxiliary(t, [c.envelope[0], c.envelope[2]]));
        }
        if is(&[
            3, 4, 5, 6, 7, 44, 45, 46, 47, 52, 114, 115, 116, 117, 118, 151, 153,
        ]) {
            if self.active_drums(t) {
                if is(&[115]) && v[152] == 0 {
                    self.engine.set_source_gain(t, v[115] as u16);
                }
                if is(&[151, 153]) {
                    self.engine
                        .apply(Command::Expression(self.channel(t), v[150] as u8));
                }
            } else {
                self.engine.apply(Command::AmplifierProgram(
                    t,
                    AmplifierProgram {
                        envelope: c.envelope[1],
                        level: v[7] as u8,
                        level_offset: v[114] as i8,
                        key_tracking: v[52] as u8,
                        source_gain: v[115] as u16,
                        midi_volume: (v[116] != 0).then_some(v[117] as u8),
                        program_volume: v[118] as u8,
                    },
                ));
            }
        }
        if is(&[8]) {
            self.engine.apply(Command::Pan(
                t,
                PanControl {
                    position: v[8] as u8,
                    ..Default::default()
                },
            ));
        }
        if is(&[53, 54, 55, 56, 57, 58]) {
            self.engine.apply(Command::Pitch(t, c.pitch));
        }
        if is(&[59, 60, 61]) {
            self.engine.apply(Command::Portamento(t, c.portamento));
        }
        if is(&[62, 63, 64]) {
            self.engine.apply(Command::VoiceMode(t, c.voice_mode));
        }
        if is(&[67, 68, 69, 70]) {
            self.engine.apply(Command::VoiceGroup(t, c.voice_group));
        }
        if is(&[65]) {
            self.engine.apply(Command::SustainProgram(t, c.sustain));
        }
        if changed.is_none_or(|id| matches!(id,73..=88|90..=113|155..=162)) {
            self.engine.apply(Command::Modulation(t, c.modulation));
        }
    }
    fn global_pitch(&mut self) {
        let v = self.settings[0];
        self.engine.apply(Command::NotePitchTables(
            Box::new(tables::note_pitch()),
            ScaleContext {
                selection: v[122] as u8 | ((v[123] as u8) << 4),
                global_transpose: Some(v[124] as i8),
                custom_cents: core::array::from_fn(|i| v[125 + i] as i8),
            },
            v[121] * 65536 / 100,
        ));
        self.engine.apply(Command::Tempo(v[89] as u16));
    }
    fn performance(&mut self) {
        let v = self.settings[0];
        self.engine.apply(Command::Performance(GlobalPerformance {
            channel: v[148] as u8,
            amplitude_receive_mode: v[149] as u8,
        }));
        self.engine.set_performance_enabled(v[152] != 0);
        if v[152] == 0 {
            for t in 0..TIMBRE_COUNT as u8 {
                self.engine
                    .set_source_gain(t, self.settings[t as usize][115] as u16);
            }
        }
    }
    fn kit(&self) -> CompiledDrumKit {
        let v = self.settings[0];
        let mut raw = [0u8; 0x700];
        raw[..12].copy_from_slice(b"Rustias Kit ");
        for (i, p) in self.drum_settings.iter().enumerate() {
            raw[18 + i] = p[147] as u8;
            raw[36 + i] = p[146] as u8;
        }
        let kit = DrumKit::from_bytes(&raw).unwrap();
        let mut program = radias_synth_domain::drum::DrumProgram::from_raw(
            ((v[141] + 1) << 5) as u8,
            v[143] as u8,
            v[144] as u8,
            v[145] as u8,
        );
        if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
            program.timbre = Some(v[141] as u8);
        }
        CompiledDrumKit {
            kit,
            program,
            instruments: core::array::from_fn(|i| DrumInstrumentProgram {
                controls: controls(&self.drum_settings[i]),
                graph: self.graph(&self.drum_settings[i]),
            }),
        }
    }
    /// A PCM adapter can use the same instrument controllers and DSP graph.
    pub fn drum_program(&self, index: usize) -> DrumInstrumentProgram {
        self.compile_drum_values(&self.drum_settings[index])
    }
    /// Compile a browser PCM profile through the same graph as a drum row.
    pub fn compile_drum_values(&self, values: &Values) -> DrumInstrumentProgram {
        DrumInstrumentProgram {
            controls: controls(values),
            graph: self.graph(values),
        }
    }
    pub fn valid_parameter(&self, id: usize, value: i32) -> bool {
        self.spec.get(id).is_some_and(|p| {
            value >= p.min
                && value <= p.max
                && !p
                    .values
                    .as_ref()
                    .is_some_and(|allowed| !allowed.contains(&value))
        })
    }
    pub fn control(&mut self, t: u8, id: usize, value: i32) -> bool {
        if t as usize >= TIMBRE_COUNT || id >= self.spec.len() {
            return false;
        }
        let p = &self.spec[id];
        if value < p.min
            || value > p.max
            || p.values
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(&value))
        {
            return false;
        }
        if id == 118 {
            return value == self.value(t, 118);
        }
        if id == 10 && value != 0 && self.value(t, 0) >= 4 {
            return false;
        }
        if id == 119 && value > self.settings[t as usize][120]
            || id == 120 && value < self.settings[t as usize][119]
        {
            return false;
        }
        if p.scope == "global" {
            for v in &mut self.settings {
                v[id] = value;
            }
            match id {
                89 => self.engine.apply(Command::Tempo(value as u16)),
                121..=136 => self.global_pitch(),
                148..=149 | 152 => {
                    self.performance();
                    if id == 148 {
                        for t in 0..TIMBRE_COUNT as u8 {
                            if self.settings[t as usize][72] == 16 {
                                self.apply_timbre(t, Some(72));
                            }
                        }
                    }
                }
                140..=141 => {
                    if self.settings[0][140] != 0 {
                        let kit = Box::new(self.kit());
                        self.engine.drum_kit(kit);
                    } else {
                        self.engine.clear_drums();
                    }
                }
                143..=145 => {
                    let v = self.settings[0];
                    self.engine
                        .update_drum_common(v[143] as u8, v[144] as u8, v[145] as u8);
                }
                _ => {}
            }
            return true;
        }
        if self.active_drums(t) && !matches!(id,59..=72|114..=118|119..=120|137..=139|150..=151|153)
        {
            return self.drum_control(self.settings[0][142] as u8, id, value);
        }
        self.settings[t as usize][id] = value;
        if id == 0 && value >= 4 {
            self.settings[t as usize][10] = 0;
        }
        let channel = self.channel(t);
        if matches!(id, 66 | 137..=139 | 150) {
            for i in 0..TIMBRE_COUNT as u8 {
                if self.channel(i) == channel {
                    self.settings[i as usize][id] = value;
                }
            }
        }
        match id {
            66 => self
                .engine
                .apply(Command::Sustain(channel, if value == 0 { 0 } else { 127 })),
            137 => self
                .engine
                .apply(Command::Bend(channel, (value + 8192) as u16)),
            138 => self.engine.apply(Command::Wheel(channel, value as u8)),
            139 => self
                .engine
                .apply(Command::PortamentoSwitch(channel, value != 0)),
            150 => self.engine.apply(Command::Expression(channel, value as u8)),
            146..=147 => {}
            _ => self.apply_timbre(t, Some(id)),
        }
        true
    }
    pub fn drum_control(&mut self, index: u8, id: usize, value: i32) -> bool {
        if index >= 16 || id >= self.spec.len() {
            return false;
        }
        let p = &self.spec[id];
        if value < p.min
            || value > p.max
            || p.values.as_ref().is_some_and(|v| !v.contains(&value))
            || p.scope == "global"
            || matches!(id,59..=72|114..=120|137..=139|150..=151|153)
        {
            return false;
        }
        let i = index as usize;
        if id == 10 && value != 0 && self.drum_settings[i][0] >= 4 {
            return false;
        }
        self.drum_settings[i][id] = value;
        if id == 0 && value >= 4 {
            self.drum_settings[i][10] = 0;
        }
        if self.settings[0][140] != 0 {
            let program = DrumInstrumentProgram {
                controls: controls(&self.drum_settings[i]),
                graph: self.graph(&self.drum_settings[i]),
            };
            self.engine
                .apply(Command::DrumInstrument(index, Box::new(program)));
            self.engine.update_drum_mapping(
                i,
                self.drum_settings[i][146] as u8,
                self.drum_settings[i][147] as u8,
            );
        }
        true
    }
    pub fn midi(&mut self, status: u8, first: u8, second: u8) {
        let channel = status & 15;
        let (id, value, command) = match status & 0xf0 {
            0x90 => (None, 0, Command::Midi(channel, first, second)),
            0x80 => (None, 0, Command::Midi(channel, first, 0)),
            0xe0 => {
                let raw = first as u16 | ((second as u16) << 7);
                (Some(137), raw as i32 - 8192, Command::Bend(channel, raw))
            }
            0xb0 => match first {
                1 => (Some(138), second as i32, Command::Wheel(channel, second)),
                11 => (
                    Some(150),
                    second as i32,
                    Command::Expression(channel, second),
                ),
                64 => (
                    Some(66),
                    (second >= 64) as i32,
                    Command::Sustain(channel, second),
                ),
                65 => (
                    Some(139),
                    (second >= 64) as i32,
                    Command::PortamentoSwitch(channel, second >= 64),
                ),
                120 => (None, 0, Command::AllSoundOff(channel)),
                123 => (None, 0, Command::AllNotesOff(channel)),
                _ => return,
            },
            _ => return,
        };
        if let Some(id) = id {
            for i in 0..TIMBRE_COUNT as u8 {
                if self.channel(i) == channel {
                    self.settings[i as usize][id] = value;
                }
            }
        }
        self.engine.apply(command);
    }
    pub fn save(&self) -> Vec<u8> {
        serde_json::to_vec(&ProgramState {
            version: 1,
            timbres: self.settings.iter().map(|v| v.to_vec()).collect(),
            drums: self.drum_settings.iter().map(|v| v.to_vec()).collect(),
        })
        .unwrap()
    }
    pub fn load(bytes: &[u8]) -> Option<Self> {
        let mut p: ProgramState = serde_json::from_slice(bytes).ok()?;
        for v in p.timbres.iter_mut().chain(&mut p.drums) {
            if v.len() == 153 {
                v.push(v[151]);
            }
            if v.len() == 154 {
                let legacy = v[29];
                v.push(match legacy {
                    3 => 0,
                    2 => 1,
                    4..=12 => legacy - 2,
                    _ => 1,
                });
                if legacy >= 2 {
                    v[29] = 2;
                }
            }
        }
        if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
            let defaults = default_values();
            for v in p.timbres.iter_mut().chain(&mut p.drums) {
                if v.len() == NATIVE_PARAMETER_COUNT {
                    v.extend_from_slice(&defaults[NATIVE_PARAMETER_COUNT..]);
                }
            }
            if (4..=TIMBRE_COUNT).contains(&p.timbres.len()) {
                let spec = parameters();
                while p.timbres.len() < TIMBRE_COUNT {
                    let mut v = defaults;
                    v[71] = 0;
                    v[72] = p.timbres.len() as i32;
                    for parameter in spec.iter().filter(|p| p.scope == "global") {
                        v[parameter.id] = p.timbres[0][parameter.id];
                    }
                    p.timbres.push(v.to_vec());
                }
            }
        }
        if p.version != 1 || p.timbres.len() != TIMBRE_COUNT || p.drums.len() != 16 {
            return None;
        }
        let spec = parameters();
        for v in p.timbres.iter().chain(&p.drums) {
            if v.len() != PARAMETER_COUNT || v[0] >= 4 && v[10] != 0 || v[119] > v[120] {
                return None;
            }
            for s in &spec {
                if v[s.id] < s.min
                    || v[s.id] > s.max
                    || s.values.as_ref().is_some_and(|a| !a.contains(&v[s.id]))
                {
                    return None;
                }
            }
        }
        for s in spec.iter().filter(|s| s.scope == "global") {
            if p.timbres.iter().any(|v| v[s.id] != p.timbres[0][s.id]) {
                return None;
            }
        }
        let mut out = Self::new();
        for i in 0..TIMBRE_COUNT as u8 {
            out.settings[i as usize].copy_from_slice(&p.timbres[i as usize]);
        }
        for i in 0..16 {
            out.drum_settings[i].copy_from_slice(&p.drums[i]);
        }
        out.global_pitch();
        out.performance();
        for t in 0..TIMBRE_COUNT as u8 {
            out.apply_timbre(t, None);
            let v = out.settings[t as usize];
            out.engine
                .apply(Command::Expression(out.channel(t), v[150] as u8));
            out.engine
                .apply(Command::Bend(out.channel(t), (v[137] + 8192) as u16));
            out.engine
                .apply(Command::Wheel(out.channel(t), v[138] as u8));
            out.engine.apply(Command::Sustain(
                out.channel(t),
                if v[66] != 0 { 127 } else { 0 },
            ));
            out.engine
                .apply(Command::PortamentoSwitch(out.channel(t), v[139] != 0));
        }
        if out.settings[0][140] != 0 {
            let kit = Box::new(out.kit());
            out.engine.drum_kit(kit);
        }
        Some(out)
    }
    pub fn note(&mut self, t: u8, n: u8, velocity: u8) -> bool {
        if t as usize >= TIMBRE_COUNT || n > 127 || velocity > 127 {
            return false;
        }
        self.engine.apply(Command::Note(t, n, velocity));
        true
    }
    pub fn drum_pad(&mut self, index: u8, velocity: u8) -> bool {
        if index >= 16 || velocity > 127 || self.settings[0][140] == 0 {
            return false;
        }
        self.engine.apply(Command::DrumPad(index, velocity));
        true
    }
    pub fn stop(&mut self) {
        self.engine.apply(Command::Stop);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ProgramState {
    version: u8,
    timbres: Vec<Vec<i32>>,
    drums: Vec<Vec<i32>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn browser_expansion_never_changes_native_capacities() {
        assert_eq!(TIMBRE_COUNT, 4);
        assert_eq!(PARAMETER_COUNT, 155);
        assert_eq!(radias_synth_application::modulation::PATCH_ROUTES, 6);
        assert_eq!(radias_synth_domain::voice_allocation::VOICE_COUNT, 24);
        assert_eq!(radias_synth_application::effect_audio::EFFECT_SLOTS, 9);
        assert_eq!(parameters()[141].max, 3);
        assert!(radias_synth_domain::pan::VoiceBus::new(4).is_none());
        assert!(ModulationDestination::new(40).is_none());
    }
    fn run(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn every_parameter_is_validated_and_forwarded() {
        run(|| {
            let mut s = StandaloneSynth::new();
            for p in parameters() {
                assert!(s.control(0, p.id, p.default), "{} default", p.id);
                assert!(!s.control(0, p.id, p.max + 1), "{} bounds", p.id);
            }
        });
    }
    #[test]
    fn drive_and_ws_keep_type_depth_and_position_independently() {
        run(|| {
            let mut s = StandaloneSynth::new();
            for t in 0..TIMBRE_COUNT as u8 {
                for kind in 0..11 {
                    assert!(s.control(t, 154, kind));
                    assert!(s.control(t, 31, 70));
                    assert!(s.control(t, 30, 1));
                    for mode in [2, 0, 1, 2] {
                        assert!(s.control(t, 29, mode));
                        assert_eq!(s.value(t, 154), kind);
                        assert_eq!(s.value(t, 31), 70);
                        assert_eq!(s.value(t, 30), 1);
                    }
                    assert_eq!(
                        controls(&s.settings[t as usize]).shaper.allocation_type(),
                        kind as u8
                    );
                }
            }
            let saved = s.save();
            let loaded = StandaloneSynth::load(&saved).unwrap();
            assert_eq!(loaded.save(), saved);
        });
    }
    #[test]
    fn legacy_combined_ws_programs_migrate_without_losing_the_type() {
        run(|| {
            let mut s = StandaloneSynth::new();
            s.control(0, 29, 2);
            s.control(0, 154, 10);
            let mut p: ProgramState = serde_json::from_slice(&s.save()).unwrap();
            for v in p.timbres.iter_mut().chain(&mut p.drums) {
                let kind = v.pop().unwrap();
                if v[29] == 2 {
                    v[29] = match kind {
                        0 => 3,
                        1 => 2,
                        _ => kind + 2,
                    };
                }
            }
            let restored = StandaloneSynth::load(&serde_json::to_vec(&p).unwrap()).unwrap();
            assert_eq!(restored.value(0, 29), 2);
            assert_eq!(restored.value(0, 154), 10);
        });
    }
    #[test]
    fn all_primary_generators_render_without_rom() {
        run(|| {
            for mode in 0..4 {
                for waveform in 0..if mode == 0 { 6 } else { 4 } {
                    let mut s = StandaloneSynth::new();
                    s.control(0, 0, waveform);
                    s.control(0, 10, mode);
                    s.control(0, 11, 64);
                    s.note(0, 60, 100);
                    let peak = (0..6000)
                        .map(|_| s.engine.sample().left.0.saturating_abs())
                        .max()
                        .unwrap();
                    assert!(peak > 1000, "{waveform}/{mode} silent");
                }
            }
        });
    }
    #[test]
    fn releases_and_timbres_remain_independent() {
        run(|| {
            let mut s = StandaloneSynth::new();
            s.control(3, 11, 76);
            assert_eq!(s.value(0, 11), 0);
            s.note(3, 69, 100);
            assert!((0..6000).any(|_| s.engine.sample().right.0 != 0));
            s.note(3, 69, 0);
            for _ in 0..48000 {
                s.engine.sample();
            }
            assert_eq!(s.engine.active_count(), 0);
        });
    }
    #[test]
    fn callback_boundaries_preserve_audio() {
        run(|| {
            let mut a = StandaloneSynth::new();
            let mut b = StandaloneSynth::new();
            a.note(0, 64, 100);
            b.note(0, 64, 100);
            let x: Vec<_> = (0..1024).map(|_| a.engine.sample()).collect();
            let mut y = Vec::new();
            for n in [17, 111, 256, 640] {
                y.extend((0..n).map(|_| b.engine.sample()));
            }
            assert_eq!(x, y);
        });
    }
    #[test]
    fn drums_use_native_instrument_dispatch() {
        run(|| {
            let mut s = StandaloneSynth::new();
            assert!(s.control(0, 140, 1));
            assert!(s.drum_pad(0, 100));
            assert!(s.engine.active_count() > 0);
            assert!((0..6000).any(|_| s.engine.sample().left.0 != 0));
            s.drum_pad(0, 0);
            s.control(0, 142, 1);
            s.control(0, 11, 91);
            assert_eq!(s.value(0, 11), 91);
            s.control(0, 142, 0);
            assert_ne!(s.value(0, 11), 91);
        });
    }
}
