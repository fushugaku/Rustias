//! Device-independent native generator shared by CPAL and Web Audio.
use crate::prepared::PreparedVoice;
use radias_synth_application::VoiceRenderer;
use radias_synth_application::amplifier::{
    AmplifierController, AmplifierProgram, ControllerTables,
};
use radias_synth_application::mixer::MixerProgram;
use radias_synth_application::modulation::{ModulationProgram, VoiceModulationTables};
use radias_synth_application::polyphony::{ActiveVoice, PolyphonicRenderer, TIMBRE_COUNT};
use radias_synth_application::secondary::SecondaryProgram;
use radias_synth_application::voice_envelopes::{
    DynamicFilter, ModEnvelopeProgram, VoiceEnvelopes,
};
use radias_synth_domain::{
    bandlimit::BandwidthTable,
    filter::FilterCoefficients,
    pan::{StereoFrame, VoiceBus},
    pitch::{PitchCode, PitchTable},
    voice_allocation::{VoiceCostParameters, VoiceCostTables},
    waveform::WaveformTable,
};

pub enum Command {
    Program(Box<crate::stored_program::CompiledProgram>),
    Start,
    Stop,
    Note(u8, u8, u8),
    Midi(u8, u8, u8),
    Bend(u8, u16),
    Wheel(u8, u8),
    PortamentoSwitch(u8, bool),
    Sustain(u8, u8),
    SustainProgram(u8, radias_synth_domain::sustain::SustainProgram),
    VoiceGroup(u8, radias_synth_domain::voice_group::VoiceGroupProgram),
    VoiceGroupTables(Box<radias_synth_domain::voice_group::VoiceGroupTables>),
    Portamento(u8, radias_synth_domain::portamento::PortamentoProgram),
    VoiceMode(u8, radias_synth_domain::mono_notes::VoiceMode),
    PortamentoTables(Box<radias_synth_application::portamento::PortamentoTables>),
    Pitch(u8, radias_synth_domain::note_pitch::PitchProgram),
    NotePitchTables(
        Box<radias_synth_domain::note_pitch::NotePitchTables>,
        radias_synth_domain::note_pitch::ScaleContext,
        i32,
    ),
    Filter(u8, FilterCoefficients),
    Shaper(u8, radias_synth_application::shaper::ShaperProgram),
    Comb(
        u8,
        Option<radias_synth_domain::filter_routing::FilterRouting>,
        radias_synth_application::comb::CombProgram,
    ),
    CombTables(Box<radias_synth_domain::controller_comb::CombControlTables>),
    FilterRouting(
        u8,
        Option<radias_synth_domain::filter_routing::FilterRouting>,
        radias_synth_domain::filter_routing::Filter2Coefficients,
    ),
    Waveform(u8, usize),
    Envelope(u8, [u8; 4]),
    AmplifierLevel(u8, u8),
    AmplifierProgram(u8, AmplifierProgram),
    Mixer(u8, MixerProgram),
    MixerScales(Box<radias_synth_domain::controller_mixer::MixerScales>),
    Secondary(u8, SecondaryProgram),
    Primary(u8, radias_synth_application::primary::PrimaryProgram),
    NoiseTables(Box<radias_synth_application::noise::NoiseTables>),
    PhysicalFrames(Box<[radias_synth_domain::noise::NoiseFrameSeeds; 2]>),
    SecondaryTable(Box<radias_synth_domain::controller_secondary::FineTuneTable>),
    Pan(u8, radias_synth_domain::controller_pan::PanControl),
    PanTables(
        Box<radias_synth_domain::controller_pan::PanTables>,
        radias_synth_domain::control_slew::SlewWeights,
    ),
    Modulation(u8, ModulationProgram),
    Tempo(u16),
    Auxiliary(u8, [ModEnvelopeProgram; 2]),
    DynamicFilter(u8, DynamicFilter),
    FilterTables(Box<radias_synth_domain::controller_filter::ControllerFilterTables>),
    Filter2Tables(Box<radias_synth_domain::controller_filter2::Filter2ControlTables>),
    Filter2Program(u8, radias_synth_application::filter2::Filter2Program),
    Timbre(u8, bool, u8),
    AllNotesOff(u8),
    AllSoundOff(u8),
}

pub struct Synthesizer {
    plans: Box<[PreparedVoice]>,
    timbres: [Timbre; TIMBRE_COUNT],
    voice_costs: Option<VoiceCostTables>,
    table: WaveformTable,
    pool: Box<PolyphonicRenderer>,
    buffer: [StereoFrame; 128],
    position: usize,
    tuning: Option<(PitchTable, BandwidthTable)>,
    controller_tables: Option<ControllerTables>,
    modulation_tables: Option<VoiceModulationTables>,
    midi_pitch: [radias_synth_application::note_pitch::MidiPitch; 16],
    portamento_switches: [bool; 16],
}

#[derive(Clone, Copy)]
struct Timbre {
    key_window: [u8; 2],
    pitch: radias_synth_domain::note_pitch::PitchProgram,
    portamento: radias_synth_domain::portamento::PortamentoProgram,
    waveform: usize,
    filter: Option<FilterCoefficients>,
    shaper: radias_synth_application::shaper::ShaperProgram,
    comb_program: Option<radias_synth_application::comb::CombProgram>,
    filter_routing: Option<(
        Option<radias_synth_domain::filter_routing::FilterRouting>,
        radias_synth_domain::filter_routing::Filter2Coefficients,
    )>,
    amplifier: AmplifierProgram,
    pan: radias_synth_domain::controller_pan::PanControl,
    mixer: MixerProgram,
    secondary: SecondaryProgram,
    primary: radias_synth_application::primary::PrimaryProgram,
    enabled: bool,
    channel: u8,
    modulation: ModulationProgram,
    auxiliary: [ModEnvelopeProgram; 2],
    dynamic_filter: Option<DynamicFilter>,
}
impl Synthesizer {
    pub fn new(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
        voice_costs: Option<VoiceCostTables>,
        modulation_tables: Option<VoiceModulationTables>,
        tempo_tables: Option<radias_synth_domain::lfo_tempo::LfoTempoTables>,
    ) -> Result<Self, String> {
        if plans.is_empty() {
            return Err("Native voice programs absent".into());
        }
        if modulation_tables.is_some() && controller_tables.is_none() {
            return Err("Live modulation requires native controller tables".into());
        }
        if tempo_tables.is_some() && modulation_tables.is_none() {
            return Err("Tempo LFO requires native modulation tables".into());
        }
        let mut pool = Box::new(PolyphonicRenderer::default());
        if let Some(tables) = tempo_tables {
            pool.enable_tempo_clock(tables, 1200);
        }
        Ok(Self {
            plans: plans.into_boxed_slice(),
            timbres: core::array::from_fn(|i| Timbre {
                key_window: [0, 127],
                pitch: Default::default(),
                portamento: Default::default(),
                waveform: 0,
                filter: None,
                filter_routing: None,
                shaper: Default::default(),
                comb_program: None,
                amplifier: AmplifierProgram::default(),
                pan: Default::default(),
                mixer: Default::default(),
                secondary: Default::default(),
                primary: Default::default(),
                enabled: i == 0,
                channel: i as u8,
                modulation: ModulationProgram::default(),
                auxiliary: [ModEnvelopeProgram::default(); 2],
                dynamic_filter: None,
            }),
            voice_costs,
            table,
            pool,
            buffer: [StereoFrame::default(); 128],
            position: 128,
            tuning,
            controller_tables,
            modulation_tables,
            midi_pitch: [Default::default(); 16],
            portamento_switches: [false; 16],
        })
    }
    pub fn active_count(&self) -> usize {
        self.pool.active_count()
    }
    pub fn held_count(&self) -> usize {
        self.pool.held_count()
    }
    #[cfg(feature = "desktop-io")]
    pub(crate) fn pool(&self) -> &PolyphonicRenderer {
        &self.pool
    }

    fn note_on(&mut self, timbre: u8, note: u8, velocity: u8, retrigger: bool) {
        let settings = self.timbres[timbre as usize];
        if !settings.enabled {
            return;
        }
        let plan = &self.plans[settings.waveform];
        let synthesis_note = radias_synth_domain::note_pitch::fold_note(
            note as i32 + settings.pitch.transpose as i32 - 64,
        );
        let pitch_code = synthesis_note as u16 * 256;
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        renderer.control_slew(plan.control_slew, (plan.reference_start_frame & 3) as u8);
        // D534 starts the ordinary waveform modulation current at zero. The
        // desktop template's observed sustained current is not a new-note state.
        renderer.initialize_primary_control(0, 3);
        if let Some((pitch, bandwidth)) = &self.tuning
            && let Some(code) = PitchCode::new(pitch_code)
        {
            let increment = pitch.increment(code);
            renderer.set_pitch(increment, bandwidth.coefficient(increment));
            renderer.set_primary_pitch_code(code);
            if let Some(primary) = settings
                .primary
                .compile_waveform(increment, bandwidth.coefficient(increment))
            {
                renderer.select_primary(primary);
            }
            if settings.primary.selection & 0x30 == 0x20 {
                renderer.update_unison_phases(
                    settings.primary.control,
                    settings.primary.selection & 3 == 2,
                );
            }
            if settings.primary.selection & 0x30 == 0x30 {
                renderer.set_primary_ratio(settings.primary.control.vpm_ratio());
            }
            renderer.set_secondary_pitch(code, pitch, bandwidth);
        }
        if let Some(filter) = settings.filter {
            renderer.set_filter_immediate(filter);
        }
        if let Some((route, second)) = settings.filter_routing {
            renderer.set_filter_routing(route, second);
            renderer.set_filter2_immediate(second);
        }
        if let (Some(program), Some(comb_tables), Some(tables)) = (
            settings.comb_program,
            self.pool.comb_tables(),
            &self.controller_tables,
        ) {
            renderer.set_filter2_immediate(program.coefficients(comb_tables, &tables.amplifier));
        }
        renderer.set_shaper_immediate(settings.shaper.parameters_with_pitch(pitch_code));
        let amplifier = self.controller_tables.as_ref().map(|tables| {
            AmplifierController::from_program(settings.amplifier, synthesis_note, velocity, tables)
        });
        let auxiliary = self.controller_tables.as_ref().map(|tables| {
            VoiceEnvelopes::new(
                settings.auxiliary,
                settings.dynamic_filter,
                synthesis_note,
                velocity,
                tables,
            )
        });
        if amplifier.is_some() {
            renderer.voice.envelope.0 = 0;
        }
        // Live Waveform/Cross selections use their original cost descriptors;
        // Filter routing/shaper descriptors are compiled; full RDL is separate.
        let cost = self
            .voice_costs
            .as_ref()
            .and_then(|tables| {
                tables.cost(VoiceCostParameters {
                    primary: settings.primary.selection,
                    secondary: settings.secondary.selection,
                    filter_route: settings.filter_routing.map_or(0, |(route, second)| {
                        use radias_synth_domain::filter_routing::{Filter2Output, FilterRouting};
                        let code = match route {
                            None => 0,
                            Some(FilterRouting::Serial) => 1,
                            Some(FilterRouting::Parallel) => 2,
                            Some(FilterRouting::Individual) => 3,
                        };
                        code | match second.output {
                            Filter2Output::LowPass => 0,
                            Filter2Output::HighPass => 16,
                            Filter2Output::BandPass => 32,
                            Filter2Output::Comb => 48,
                        }
                    }),
                    drive_mode: settings.shaper.allocation_mode(),
                    shaper_type: settings.shaper.allocation_type(),
                })
            })
            .unwrap_or(4283) as u16;
        let voice = ActiveVoice {
            renderer,
            amplifier,
            modulation: None,
            auxiliary,
            pan: Some(settings.pan),
            mixer: Some(settings.mixer),
            secondary: Some(settings.secondary),
            primary: Some(settings.primary),
            shaper: Some(settings.shaper),
            comb_program: settings.comb_program,
            timbre,
            note,
            velocity,
            held: true,
            program: settings.waveform,
            bus: VoiceBus::new(timbre).unwrap(),
        };
        if self.modulation_tables.is_some() {
            if retrigger {
                self.pool
                    .retrigger_modulated(voice, cost, settings.modulation);
            } else {
                self.pool
                    .trigger_modulated(voice, cost, settings.modulation);
            }
        } else {
            self.pool.trigger(voice, cost);
        }
        self.position = 128;
    }
    /// Apply one validated adapter command before generating the next block.
    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Program(program) => {
                self.pool.stop();
                self.buffer.fill(StereoFrame::default());
                self.position = 128;
                self.pool.set_tempo(program.stored.tempo_tenths);
                for timbre in 0..TIMBRE_COUNT {
                    let source = program.stored.timbres[timbre];
                    let c = source.controls;
                    let compiled = program.timbres[timbre];
                    self.timbres[timbre] = Timbre {
                        key_window: source.key_window,
                        pitch: c.pitch,
                        portamento: c.portamento,
                        waveform: (c.oscillator_selection & 3) as usize,
                        filter: Some(compiled.filter),
                        shaper: c.shaper,
                        comb_program: compiled.comb,
                        filter_routing: Some((compiled.filter_routing, compiled.filter2)),
                        amplifier: c.amplifier(0x7f00, None, 0),
                        pan: radias_synth_domain::controller_pan::PanControl {
                            position: c.pan,
                            ..Default::default()
                        },
                        mixer: c.mixer,
                        secondary: c.secondary,
                        primary: c.primary(),
                        enabled: source.enabled,
                        channel: source.channel,
                        modulation: c.modulation,
                        auxiliary: [c.envelope[0], c.envelope[2]],
                        dynamic_filter: Some(compiled.dynamic_filter),
                    };
                    let t = timbre as u8;
                    self.pool.set_timbre_modulation_active(t, source.enabled);
                    self.pool.edit_pitch_program(t, c.pitch);
                    self.pool.edit_portamento_program(t, c.portamento);
                    self.pool.set_voice_mode(t, c.voice_mode);
                    self.pool.edit_voice_group(t, c.voice_group);
                    self.pool.edit_sustain_program(t, c.sustain, 0);
                    self.pool.edit_filter2_program(t, compiled.dynamic_filter2);
                    let _ = self.pool.edit_modulation(t, c.modulation);
                    self.pool
                        .set_midi_pitch(t, self.midi_pitch[source.channel as usize]);
                    self.pool.set_portamento_switch(
                        t,
                        self.portamento_switches[source.channel as usize],
                    );
                }
            }
            Command::Start => {
                self.note(0, 60, 100);
            }
            Command::Waveform(timbre, index) => {
                if index < self.plans.len() {
                    self.timbres[timbre as usize].waveform = index;
                    let selection =
                        (self.timbres[timbre as usize].primary.selection & 0x30) | index as u8;
                    self.timbres[timbre as usize].mixer.selections[0] = selection;
                    self.timbres[timbre as usize].primary.selection = selection;
                    let primary = self.timbres[timbre as usize]
                        .primary
                        .compile_waveform(radias_synth_domain::pitch::PhaseIncrement(0), 0)
                        .unwrap_or(self.plans[index].parameters.primary);
                    self.pool.edit_primary(timbre, index, primary);
                    self.pool
                        .edit_mixer(timbre, self.timbres[timbre as usize].mixer);
                    self.pool
                        .edit_primary_control(timbre, self.timbres[timbre as usize].primary);
                }
            }
            Command::Stop => {
                self.pool.stop();
                self.buffer.fill(StereoFrame::default());
                self.position = 128;
            }
            Command::Note(timbre, note, velocity) => self.note(timbre, note, velocity),
            Command::Midi(channel, note, velocity) => {
                // Original global-note dispatcher visits timbres 4..1.
                for timbre in (0..TIMBRE_COUNT).rev() {
                    let settings = self.timbres[timbre];
                    if settings.enabled
                        && settings.channel == channel
                        && settings.key_window[0] <= note
                        && note <= settings.key_window[1]
                    {
                        self.note(timbre as u8, note, velocity);
                    }
                }
            }
            Command::Bend(channel, raw) => {
                self.midi_pitch[channel as usize].bend =
                    radias_synth_domain::note_pitch::normalize_bend(raw);
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool
                            .set_midi_pitch(timbre as u8, self.midi_pitch[channel as usize]);
                    }
                }
            }
            Command::Wheel(channel, value) => {
                self.midi_pitch[channel as usize].wheel = value;
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool
                            .set_midi_pitch(timbre as u8, self.midi_pitch[channel as usize]);
                    }
                }
            }
            Command::Pitch(timbre, program) => {
                self.timbres[timbre as usize].pitch = program;
                self.pool.edit_pitch_program(timbre, program);
            }
            Command::PortamentoSwitch(channel, value) => {
                self.portamento_switches[channel as usize] = value;
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool.set_portamento_switch(timbre as u8, value);
                    }
                }
            }
            Command::Sustain(channel, value) => {
                let event = 0x10000000 | ((value as u32) << 16) | channel as u32;
                self.pool.sustain_event(
                    self.timbres.map(|t| t.channel),
                    event,
                    self.controller_tables.as_ref(),
                );
            }
            Command::SustainProgram(timbre, program) => {
                self.pool.edit_sustain_program(timbre, program, 0)
            }
            Command::VoiceGroup(timbre, program) => self.pool.edit_voice_group(timbre, program),
            Command::VoiceGroupTables(tables) => self.pool.configure_voice_groups(*tables),
            Command::Portamento(timbre, program) => {
                self.timbres[timbre as usize].portamento = program;
                self.pool.edit_portamento_program(timbre, program);
            }
            Command::VoiceMode(timbre, mode) => self.pool.set_voice_mode(timbre, mode),
            Command::PortamentoTables(tables) => {
                self.pool.configure_portamento(*tables);
                for timbre in 0..TIMBRE_COUNT {
                    self.pool
                        .edit_portamento_program(timbre as u8, self.timbres[timbre].portamento);
                }
            }
            Command::NotePitchTables(tables, scale, master) => {
                self.pool.configure_note_pitch(*tables, scale, master);
                for timbre in 0..TIMBRE_COUNT {
                    self.pool
                        .edit_pitch_program(timbre as u8, self.timbres[timbre].pitch);
                }
            }
            Command::Filter(timbre, coefficients) => {
                self.timbres[timbre as usize].filter = Some(coefficients);
                self.pool.edit_filter(timbre, coefficients);
            }
            Command::Shaper(timbre, shaper) => {
                self.timbres[timbre as usize].shaper = shaper;
                self.pool.edit_shaper(timbre, shaper);
            }
            Command::FilterRouting(timbre, route, second) => {
                self.pool.edit_filter2_program(timbre, None);
                self.timbres[timbre as usize].comb_program = None;
                self.pool.edit_comb_program(timbre, None);
                self.timbres[timbre as usize].filter_routing = Some((route, second));
                self.pool.edit_filter_routing(timbre, route, second);
            }
            Command::CombTables(tables) => self.pool.configure_comb(*tables),
            Command::Comb(timbre, route, program) => {
                self.pool.edit_filter2_program(timbre, None);
                if let (Some(comb_tables), Some(tables)) =
                    (self.pool.comb_tables(), &self.controller_tables)
                {
                    let second = program.coefficients(comb_tables, &tables.amplifier);
                    self.timbres[timbre as usize].comb_program = Some(program);
                    self.timbres[timbre as usize].filter_routing = Some((route, second));
                    self.pool.edit_filter_routing(timbre, route, second);
                    self.pool.edit_comb_program(timbre, Some(program));
                }
            }
            Command::Envelope(timbre, values) => {
                self.timbres[timbre as usize].amplifier.envelope.adsr = values;
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_adsr(timbre, values, tables);
                }
            }
            Command::AmplifierLevel(timbre, level) => {
                self.timbres[timbre as usize].amplifier.level = level;
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_amplifier_level(timbre, level, tables);
                }
            }
            Command::AmplifierProgram(timbre, program) => {
                self.timbres[timbre as usize].amplifier = program;
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_amplifier_program(timbre, program, tables);
                }
            }
            Command::Pan(timbre, pan) => {
                self.timbres[timbre as usize].pan = pan;
                self.pool.edit_pan(timbre, pan);
            }
            Command::PanTables(tables, weights) => self.pool.configure_pan(*tables, weights),
            Command::Mixer(timbre, program) => {
                self.timbres[timbre as usize].mixer = program;
                self.pool.edit_mixer(timbre, program);
            }
            Command::MixerScales(scales) => self.pool.configure_mixer(*scales),
            Command::Secondary(timbre, program) => {
                self.timbres[timbre as usize].secondary = program;
                self.timbres[timbre as usize].mixer.selections[1] = program.selection;
                self.pool.edit_secondary(timbre, program);
                self.pool
                    .edit_mixer(timbre, self.timbres[timbre as usize].mixer);
            }
            Command::Primary(timbre, program) => {
                if self.timbres[timbre as usize].primary.selection != program.selection {
                    let template = if matches!(program.selection, 4 | 5) {
                        0
                    } else {
                        (program.selection & 3) as usize
                    };
                    self.timbres[timbre as usize].waveform = template;
                    let carrier = program
                        .compile_waveform(radias_synth_domain::pitch::PhaseIncrement(0), 0)
                        .unwrap();
                    self.pool.edit_primary(timbre, template, carrier);
                    self.timbres[timbre as usize].mixer.selections[0] = program.selection;
                    self.pool
                        .edit_mixer(timbre, self.timbres[timbre as usize].mixer);
                }
                self.timbres[timbre as usize].primary = program;
                self.pool.edit_primary_control(timbre, program);
            }
            Command::NoiseTables(tables) => {
                // Software boot chooses explicit deterministic input words.
                // A comparison can override these with the same inputs
                // accepted by the reference; no trace file is read here.
                self.pool.initialize_physical_frames([
                    radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(0, 0),
                    radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(0, 0),
                ]);
                self.pool.configure_noise(*tables);
            }
            Command::PhysicalFrames(seeds) => self.pool.initialize_physical_frames(*seeds),
            Command::SecondaryTable(table) => self.pool.configure_secondary(*table),
            Command::Modulation(timbre, program) => {
                self.timbres[timbre as usize].modulation = program;
                let _ = self.pool.edit_modulation(timbre, program);
            }
            Command::Tempo(tempo) => self.pool.set_tempo(tempo),
            Command::FilterTables(tables) => self.pool.controller_filter_tables(*tables),
            Command::Filter2Tables(tables) => self.pool.configure_filter2(*tables),
            Command::Filter2Program(timbre, program) => {
                self.pool.edit_filter2_program(timbre, Some(program))
            }
            Command::Auxiliary(timbre, programs) => {
                self.timbres[timbre as usize].auxiliary = programs;
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_auxiliary(timbre, programs, tables);
                }
            }
            Command::DynamicFilter(timbre, filter) => {
                self.timbres[timbre as usize].dynamic_filter = Some(filter);
                self.pool.edit_dynamic_filter(timbre, filter);
            }
            Command::Timbre(timbre, enabled, channel) => {
                self.pool.set_timbre_modulation_active(timbre, enabled);
                let old_channel = self.timbres[timbre as usize].channel;
                self.timbres[timbre as usize].enabled = enabled;
                self.timbres[timbre as usize].channel = channel;
                self.pool
                    .set_midi_pitch(timbre, self.midi_pitch[channel as usize]);
                self.pool
                    .set_portamento_switch(timbre, self.portamento_switches[channel as usize]);
                if !enabled || old_channel != channel {
                    self.pool
                        .release_all_notes(timbre, self.controller_tables.as_ref());
                }
            }
            Command::AllNotesOff(channel) => {
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool
                            .release_all_notes(timbre as u8, self.controller_tables.as_ref());
                    }
                }
            }
            Command::AllSoundOff(channel) => {
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool.stop_timbre(timbre as u8);
                    }
                }
            }
        }
    }
    fn note(&mut self, timbre: u8, note: u8, velocity: u8) {
        if !self.timbres[timbre as usize].enabled {
            return;
        }
        let window = self.timbres[timbre as usize].key_window;
        if note < window[0] || note > window[1] {
            return;
        }
        let event = ((0x10 | self.timbres[timbre as usize].channel as u32) << 24)
            | ((velocity as u32) << 8)
            | note as u32
            | if velocity != 0 { 128 } else { 0 };
        self.pool.begin_note_event((event >> 24) as u8);
        if let Some(decision) = self.pool.mono_event(timbre, event) {
            use radias_synth_domain::mono_notes::MonoAction;
            let selected_note = decision.event as u8 & 127;
            let selected_velocity = (decision.event >> 8) as u8 & 127;
            match decision.action {
                MonoAction::Ignore => {}
                MonoAction::Legato => {
                    self.pool.legato(timbre, selected_note, selected_velocity);
                }
                MonoAction::Allocate => {
                    self.note_on(timbre, selected_note, selected_velocity, false)
                }
                MonoAction::Retrigger => {
                    self.note_on(timbre, selected_note, selected_velocity, true)
                }
                MonoAction::Release => {
                    self.pool
                        .release_note(timbre, selected_note, self.controller_tables.as_ref())
                }
            }
            self.pool.finish_note_event();
            return;
        }
        if velocity == 0 {
            self.pool
                .release_note(timbre, note, self.controller_tables.as_ref());
        } else {
            self.note_on(timbre, note, velocity, false);
        }
        self.pool.finish_note_event();
    }
    pub fn sample(&mut self) -> StereoFrame {
        if self.position == 128 {
            for sample in &mut self.buffer {
                *sample = self.pool.next_sample_with_modulation(
                    &self.table,
                    self.controller_tables.as_ref(),
                    self.modulation_tables.as_ref(),
                    |program| &self.plans[program].events,
                );
            }
            self.position = 0;
        }
        let sample = self.buffer[self.position];
        self.position += 1;
        sample
    }
}
