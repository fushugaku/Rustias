//! Direct device adapter. No firmware machine, audio queue or offline preview.
use crate::prepared::PreparedVoice;
use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
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
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};

enum Command {
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
#[derive(Default)]
struct Counters {
    callbacks: AtomicU64,
    native_frames: AtomicU64,
    audible_frames: AtomicU64,
    worst_render_ns: AtomicU64,
    deadline_misses: AtomicU64,
    gain: AtomicU32,
    failed: AtomicBool,
    active_voices: AtomicU32,
    held_voices: AtomicU32,
    held_notes: [AtomicU32; TIMBRE_COUNT],
    sustain_flags: [AtomicU32; TIMBRE_COUNT],
    output_peak: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct AudioStatus {
    pub callbacks: u64,
    pub native_frames: u64,
    pub audible_frames: u64,
    pub worst_render_ns: u64,
    pub deadline_misses: u64,
    pub failed: bool,
    pub device: String,
    pub sample_rate: u32,
    pub active_voices: u32,
    pub held_voices: u32,
    pub output_peak: f32,
}

pub struct NativePlayer {
    commands: SyncSender<Command>,
    counters: Arc<Counters>,
    device: String,
    rate: u32,
    modulation_available: bool,
    tempo_available: bool,
    portamento_available: AtomicBool,
    filter2_available: AtomicBool,
    _stream: cpal::Stream,
}

#[derive(Clone)]
pub struct NativeInput {
    commands: SyncSender<Command>,
}
impl NativeInput {
    /// MIDI is translated at the adapter boundary; the controller compiles
    /// velocity and envelope targets independently of the audio device.
    pub fn midi(&self, message: &[u8]) -> Result<(), String> {
        if message.len() != 3 {
            return Ok(());
        }
        let command = match message[0] & 0xf0 {
            0x90 => Command::Midi(message[0] & 15, message[1], message[2]),
            0x80 => Command::Midi(message[0] & 15, message[1], 0),
            0xe0 => Command::Bend(
                message[0] & 15,
                message[1] as u16 | ((message[2] as u16) << 7),
            ),
            0xb0 if message[1] == 1 => Command::Wheel(message[0] & 15, message[2]),
            0xb0 if message[1] == 65 => {
                Command::PortamentoSwitch(message[0] & 15, message[2] & 64 != 0)
            }
            0xb0 if message[1] == 64 => Command::Sustain(message[0] & 15, message[2]),
            0xb0 if message[1] == 123 => Command::AllNotesOff(message[0] & 15),
            0xb0 if message[1] == 120 => Command::AllSoundOff(message[0] & 15),
            _ => return Ok(()),
        };
        if message[1] > 127 || message[2] > 127 {
            return Err("Invalid MIDI data byte".into());
        }
        self.commands.try_send(command).map_err(|e| e.to_string())
    }
}

struct Generator {
    plans: Box<[PreparedVoice]>,
    timbres: [Timbre; TIMBRE_COUNT],
    voice_costs: Option<VoiceCostTables>,
    table: WaveformTable,
    pool: PolyphonicRenderer,
    commands: Receiver<Command>,
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
impl Generator {
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
    fn service(&mut self) {
        for _ in 0..64 {
            match self.commands.try_recv() {
                Ok(Command::Program(program)) => {
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
                Ok(Command::Start) => {
                    self.note(0, 60, 100);
                }
                Ok(Command::Waveform(timbre, index)) => {
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
                Ok(Command::Stop) => {
                    self.pool.stop();
                    self.buffer.fill(StereoFrame::default());
                    self.position = 128;
                }
                Ok(Command::Note(timbre, note, velocity)) => self.note(timbre, note, velocity),
                Ok(Command::Midi(channel, note, velocity)) => {
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
                Ok(Command::Bend(channel, raw)) => {
                    self.midi_pitch[channel as usize].bend =
                        radias_synth_domain::note_pitch::normalize_bend(raw);
                    for timbre in 0..TIMBRE_COUNT {
                        if self.timbres[timbre].channel == channel {
                            self.pool
                                .set_midi_pitch(timbre as u8, self.midi_pitch[channel as usize]);
                        }
                    }
                }
                Ok(Command::Wheel(channel, value)) => {
                    self.midi_pitch[channel as usize].wheel = value;
                    for timbre in 0..TIMBRE_COUNT {
                        if self.timbres[timbre].channel == channel {
                            self.pool
                                .set_midi_pitch(timbre as u8, self.midi_pitch[channel as usize]);
                        }
                    }
                }
                Ok(Command::Pitch(timbre, program)) => {
                    self.timbres[timbre as usize].pitch = program;
                    self.pool.edit_pitch_program(timbre, program);
                }
                Ok(Command::PortamentoSwitch(channel, value)) => {
                    self.portamento_switches[channel as usize] = value;
                    for timbre in 0..TIMBRE_COUNT {
                        if self.timbres[timbre].channel == channel {
                            self.pool.set_portamento_switch(timbre as u8, value);
                        }
                    }
                }
                Ok(Command::Sustain(channel, value)) => {
                    let event = 0x10000000 | ((value as u32) << 16) | channel as u32;
                    self.pool.sustain_event(
                        self.timbres.map(|t| t.channel),
                        event,
                        self.controller_tables.as_ref(),
                    );
                }
                Ok(Command::SustainProgram(timbre, program)) => {
                    self.pool.edit_sustain_program(timbre, program, 0)
                }
                Ok(Command::VoiceGroup(timbre, program)) => {
                    self.pool.edit_voice_group(timbre, program)
                }
                Ok(Command::VoiceGroupTables(tables)) => self.pool.configure_voice_groups(*tables),
                Ok(Command::Portamento(timbre, program)) => {
                    self.timbres[timbre as usize].portamento = program;
                    self.pool.edit_portamento_program(timbre, program);
                }
                Ok(Command::VoiceMode(timbre, mode)) => self.pool.set_voice_mode(timbre, mode),
                Ok(Command::PortamentoTables(tables)) => {
                    self.pool.configure_portamento(*tables);
                    for timbre in 0..TIMBRE_COUNT {
                        self.pool
                            .edit_portamento_program(timbre as u8, self.timbres[timbre].portamento);
                    }
                }
                Ok(Command::NotePitchTables(tables, scale, master)) => {
                    self.pool.configure_note_pitch(*tables, scale, master);
                    for timbre in 0..TIMBRE_COUNT {
                        self.pool
                            .edit_pitch_program(timbre as u8, self.timbres[timbre].pitch);
                    }
                }
                Ok(Command::Filter(timbre, coefficients)) => {
                    self.timbres[timbre as usize].filter = Some(coefficients);
                    self.pool.edit_filter(timbre, coefficients);
                }
                Ok(Command::Shaper(timbre, shaper)) => {
                    self.timbres[timbre as usize].shaper = shaper;
                    self.pool.edit_shaper(timbre, shaper);
                }
                Ok(Command::FilterRouting(timbre, route, second)) => {
                    self.pool.edit_filter2_program(timbre, None);
                    self.timbres[timbre as usize].comb_program = None;
                    self.pool.edit_comb_program(timbre, None);
                    self.timbres[timbre as usize].filter_routing = Some((route, second));
                    self.pool.edit_filter_routing(timbre, route, second);
                }
                Ok(Command::CombTables(tables)) => self.pool.configure_comb(*tables),
                Ok(Command::Comb(timbre, route, program)) => {
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
                Ok(Command::Envelope(timbre, values)) => {
                    self.timbres[timbre as usize].amplifier.envelope.adsr = values;
                    if let Some(tables) = &self.controller_tables {
                        self.pool.edit_adsr(timbre, values, tables);
                    }
                }
                Ok(Command::AmplifierLevel(timbre, level)) => {
                    self.timbres[timbre as usize].amplifier.level = level;
                    if let Some(tables) = &self.controller_tables {
                        self.pool.edit_amplifier_level(timbre, level, tables);
                    }
                }
                Ok(Command::AmplifierProgram(timbre, program)) => {
                    self.timbres[timbre as usize].amplifier = program;
                    if let Some(tables) = &self.controller_tables {
                        self.pool.edit_amplifier_program(timbre, program, tables);
                    }
                }
                Ok(Command::Pan(timbre, pan)) => {
                    self.timbres[timbre as usize].pan = pan;
                    self.pool.edit_pan(timbre, pan);
                }
                Ok(Command::PanTables(tables, weights)) => {
                    self.pool.configure_pan(*tables, weights)
                }
                Ok(Command::Mixer(timbre, program)) => {
                    self.timbres[timbre as usize].mixer = program;
                    self.pool.edit_mixer(timbre, program);
                }
                Ok(Command::MixerScales(scales)) => self.pool.configure_mixer(*scales),
                Ok(Command::Secondary(timbre, program)) => {
                    self.timbres[timbre as usize].secondary = program;
                    self.timbres[timbre as usize].mixer.selections[1] = program.selection;
                    self.pool.edit_secondary(timbre, program);
                    self.pool
                        .edit_mixer(timbre, self.timbres[timbre as usize].mixer);
                }
                Ok(Command::Primary(timbre, program)) => {
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
                Ok(Command::NoiseTables(tables)) => {
                    // Software boot chooses explicit deterministic input words.
                    // A comparison can override these with the same inputs
                    // accepted by the reference; no trace file is read here.
                    self.pool.initialize_physical_frames([
                        radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(0, 0),
                        radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(0, 0),
                    ]);
                    self.pool.configure_noise(*tables);
                }
                Ok(Command::PhysicalFrames(seeds)) => self.pool.initialize_physical_frames(*seeds),
                Ok(Command::SecondaryTable(table)) => self.pool.configure_secondary(*table),
                Ok(Command::Modulation(timbre, program)) => {
                    self.timbres[timbre as usize].modulation = program;
                    let _ = self.pool.edit_modulation(timbre, program);
                }
                Ok(Command::Tempo(tempo)) => self.pool.set_tempo(tempo),
                Ok(Command::FilterTables(tables)) => self.pool.controller_filter_tables(*tables),
                Ok(Command::Filter2Tables(tables)) => self.pool.configure_filter2(*tables),
                Ok(Command::Filter2Program(timbre, program)) => {
                    self.pool.edit_filter2_program(timbre, Some(program))
                }
                Ok(Command::Auxiliary(timbre, programs)) => {
                    self.timbres[timbre as usize].auxiliary = programs;
                    if let Some(tables) = &self.controller_tables {
                        self.pool.edit_auxiliary(timbre, programs, tables);
                    }
                }
                Ok(Command::DynamicFilter(timbre, filter)) => {
                    self.timbres[timbre as usize].dynamic_filter = Some(filter);
                    self.pool.edit_dynamic_filter(timbre, filter);
                }
                Ok(Command::Timbre(timbre, enabled, channel)) => {
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
                Ok(Command::AllNotesOff(channel)) => {
                    for timbre in 0..TIMBRE_COUNT {
                        if self.timbres[timbre].channel == channel {
                            self.pool
                                .release_all_notes(timbre as u8, self.controller_tables.as_ref());
                        }
                    }
                }
                Ok(Command::AllSoundOff(channel)) => {
                    for timbre in 0..TIMBRE_COUNT {
                        if self.timbres[timbre].channel == channel {
                            self.pool.stop_timbre(timbre as u8);
                        }
                    }
                }
                Err(_) => break,
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
    fn sample(&mut self) -> StereoFrame {
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

impl NativePlayer {
    pub fn configure_voice_groups(
        &self,
        tables: radias_synth_domain::voice_group::VoiceGroupTables,
    ) -> Result<(), String> {
        if !self.modulation_available {
            return Err("Voice groups require native controller tables".into());
        }
        self.commands
            .try_send(Command::VoiceGroupTables(Box::new(tables)))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_voice_group(
        &self,
        timbre: u8,
        program: radias_synth_domain::voice_group::VoiceGroupProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if program.raw & 128 != 0 && program.raw & 15 > 6 {
            return Err("Unison voice count must be2 through8".into());
        }
        self.commands
            .try_send(Command::VoiceGroup(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn sustain_flags(&self) -> [u8; TIMBRE_COUNT] {
        core::array::from_fn(|t| self.counters.sustain_flags[t].load(Ordering::Relaxed) as u8)
    }
    pub fn timbre_sustain_program(
        &self,
        timbre: u8,
        program: radias_synth_domain::sustain::SustainProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::SustainProgram(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn held_notes(&self) -> [Option<u8>; TIMBRE_COUNT] {
        core::array::from_fn(|timbre| {
            let raw = self.counters.held_notes[timbre].load(Ordering::Relaxed);
            (raw != 0).then(|| (raw - 1) as u8)
        })
    }
    pub fn input(&self) -> NativeInput {
        NativeInput {
            commands: self.commands.clone(),
        }
    }
    pub fn new(plan: PreparedVoice, table: WaveformTable) -> Result<Self, String> {
        Self::with_tuning(plan, table, None)
    }
    pub fn with_tuning(
        plan: PreparedVoice,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
    ) -> Result<Self, String> {
        Self::with_programs(vec![plan], table, tuning)
    }
    pub fn with_programs(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
    ) -> Result<Self, String> {
        Self::with_controller(plans, table, tuning, None)
    }
    pub fn with_controller(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
    ) -> Result<Self, String> {
        Self::with_instrument(plans, table, tuning, controller_tables, None)
    }
    pub fn with_instrument(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
        voice_costs: Option<VoiceCostTables>,
    ) -> Result<Self, String> {
        Self::with_modulation(plans, table, tuning, controller_tables, voice_costs, None)
    }
    pub fn with_modulation(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
        voice_costs: Option<VoiceCostTables>,
        modulation_tables: Option<VoiceModulationTables>,
    ) -> Result<Self, String> {
        Self::with_clock(
            plans,
            table,
            tuning,
            controller_tables,
            voice_costs,
            modulation_tables,
            None,
        )
    }
    pub fn with_clock(
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
        let modulation_available = modulation_tables.is_some();
        let tempo_available = tempo_tables.is_some();
        if tempo_available && !modulation_available {
            return Err("Tempo LFO requires native modulation tables".into());
        }
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("Нет аудиовыхода")?;
        let name = device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_else(|_| "Default audio".into());
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        // Startup queues ROM adapters and four complete timbre settings before
        // CoreAudio necessarily invokes its first callback. Keep that burst
        // intact; service still handles at most 64 commands per callback.
        let (tx, rx) = mpsc::sync_channel(256);
        let counters = Arc::new(Counters::default());
        counters.gain.store(0.2f32.to_bits(), Ordering::Relaxed);
        let mut pool = PolyphonicRenderer::default();
        if let Some(tables) = tempo_tables {
            pool.enable_tempo_clock(tables, 1200);
        }
        let generator = Generator {
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
            commands: rx,
            buffer: [StereoFrame::default(); 128],
            position: 128,
            tuning,
            controller_tables,
            modulation_tables,
            midi_pitch: [Default::default(); 16],
            portamento_switches: [false; 16],
        };
        let stream = match format {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config, generator, counters.clone()),
            cpal::SampleFormat::F64 => stream::<f64>(&device, &config, generator, counters.clone()),
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config, generator, counters.clone()),
            cpal::SampleFormat::I32 => stream::<i32>(&device, &config, generator, counters.clone()),
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config, generator, counters.clone()),
            _ => return Err(format!("Unsupported audio output format: {format:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            commands: tx,
            counters,
            device: name,
            rate: config.sample_rate,
            modulation_available,
            tempo_available,
            portamento_available: AtomicBool::new(false),
            filter2_available: AtomicBool::new(false),
            _stream: stream,
        })
    }
    pub fn start(&self) -> Result<(), String> {
        self.commands
            .try_send(Command::Start)
            .map_err(|e| e.to_string())
    }
    pub fn load_program(
        &self,
        program: crate::stored_program::CompiledProgram,
    ) -> Result<(), String> {
        program.validate_native_generators().map_err(|e| match e {
            radias_synth_application::stored_program::ProgramCompilationError::Generator {
                timbre,
                selection,
            } => {
                let kind = match selection & 15 {
                    6 => "Synth PCM",
                    7 => "Drum PCM",
                    8 => "Audio In",
                    _ => "режим OSC1",
                };
                format!("Тембр {}: {kind} ещё переносится", timbre + 1)
            }
            other => format!("Не удалось прочитать программу: {other:?}"),
        })?;
        if !self.modulation_available || !self.tempo_available {
            return Err("Stored programs require native controller and tempo tables".into());
        }
        if !self.filter2_available.load(Ordering::Relaxed) {
            return Err("Stored programs require native Filter2 controller tables".into());
        }
        for t in &program.stored.timbres {
            t.controls
                .modulation
                .validate_with_clock(true)
                .map_err(|_| "Stored program modulation is unsupported".to_string())?;
        }
        self.commands
            .try_send(Command::Program(Box::new(program)))
            .map_err(|e| e.to_string())
    }
    pub fn stop(&self) -> Result<(), String> {
        self.commands
            .try_send(Command::Stop)
            .map_err(|e| e.to_string())
    }
    pub fn note(&self, note: u8, on: bool) -> Result<(), String> {
        self.note_velocity(note, if on { 100 } else { 0 })
    }
    pub fn note_velocity(&self, note: u8, velocity: u8) -> Result<(), String> {
        if note > 127 || velocity > 127 {
            return Err("Invalid MIDI note".into());
        }
        self.commands
            .try_send(Command::Note(0, note, velocity))
            .map_err(|e| e.to_string())
    }
    pub fn filter(&self, coefficients: FilterCoefficients) -> Result<(), String> {
        self.timbre_filter(0, coefficients)
    }
    pub fn timbre_shaper(
        &self,
        timbre: u8,
        mode: u8,
        position: u8,
        depth: u8,
    ) -> Result<(), String> {
        use radias_synth_application::shaper::{ShaperMode, ShaperProgram};
        use radias_synth_domain::{controller_shaper::ShaperControl, waveshaper::ShaperPosition};
        if timbre >= 4 || depth > 127 {
            return Err("Shaper timbre/depth out of range".into());
        }
        let mode = ShaperMode::from_panel(mode).ok_or("Shaper mode out of range")?;
        let position = match position {
            0 => ShaperPosition::PreFilter,
            1 => ShaperPosition::PreAmp,
            _ => return Err("Shaper position out of range".into()),
        };
        let shaper = ShaperProgram {
            mode,
            position,
            control: ShaperControl {
                depth,
                ..Default::default()
            },
        };
        self.commands
            .try_send(Command::Shaper(timbre, shaper))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_filter_routing(
        &self,
        timbre: u8,
        route: u8,
        second: radias_synth_domain::filter_routing::Filter2Coefficients,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        use radias_synth_domain::filter_routing::FilterRouting;
        let routing = match route {
            0 => None,
            1 => Some(FilterRouting::Serial),
            2 => Some(FilterRouting::Parallel),
            3 => Some(FilterRouting::Individual),
            _ => return Err("Invalid native filter route".into()),
        };
        self.commands
            .try_send(Command::FilterRouting(timbre, routing, second))
            .map_err(|e| e.to_string())
    }
    pub fn configure_comb(
        &self,
        tables: radias_synth_domain::controller_comb::CombControlTables,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::CombTables(Box::new(tables)))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_comb(
        &self,
        timbre: u8,
        route: u8,
        program: radias_synth_application::comb::CombProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        use radias_synth_domain::filter_routing::FilterRouting;
        let route = match route {
            0 => None,
            1 => Some(FilterRouting::Serial),
            2 => Some(FilterRouting::Parallel),
            3 => Some(FilterRouting::Individual),
            _ => return Err("Invalid native filter route".into()),
        };
        self.commands
            .try_send(Command::Comb(timbre, route, program))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_filter(
        &self,
        timbre: u8,
        coefficients: FilterCoefficients,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::Filter(timbre, coefficients))
            .map_err(|e| e.to_string())
    }
    pub fn adsr(&self, values: [u8; 4]) -> Result<(), String> {
        self.timbre_adsr(0, values)
    }
    pub fn timbre_adsr(&self, timbre: u8, values: [u8; 4]) -> Result<(), String> {
        validate_timbre(timbre)?;
        if values.iter().any(|&v| v > 127) {
            return Err("Invalid envelope value".into());
        }
        self.commands
            .try_send(Command::Envelope(timbre, values))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_modulation(&self, timbre: u8, program: ModulationProgram) -> Result<(), String> {
        if !self.modulation_available {
            return Err("Live modulation tables were not configured".into());
        }
        validate_timbre(timbre)?;
        program
            .validate_with_clock(self.tempo_available)
            .map_err(|_| "Tempo clock tables were not configured")?;
        for route in program.routes {
            if ![0, 1, 2, 3, 4, 5, 6, 7, 8].contains(&(route.source & 15))
                || ![
                    0, 1, 2, 3, 4, 5, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 34, 35,
                    36, 37, 38, 39,
                ]
                .contains(&(route.destination.index() as u8))
            {
                return Err("Native live route destination/source is not yet compiled".into());
            }
            if route.destination.index() == 15 && !self.portamento_available.load(Ordering::Relaxed)
            {
                return Err("Portamento modulation requires its controller tables".into());
            }
        }
        self.commands
            .try_send(Command::Modulation(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn waveform(&self, index: usize) -> Result<(), String> {
        self.timbre_waveform(0, index)
    }
    pub fn timbre_amplifier_level(&self, timbre: u8, level: u8) -> Result<(), String> {
        validate_timbre(timbre)?;
        if level > 127 {
            return Err("Amplifier level outside original 7-bit range".into());
        }
        self.commands
            .try_send(Command::AmplifierLevel(timbre, level))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_amplifier_program(
        &self,
        timbre: u8,
        program: AmplifierProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if program.level > 127
            || program.program_volume > 127
            || program.midi_volume.is_some_and(|v| v > 127)
            || program.envelope.adsr.iter().any(|&v| v > 127)
            || program.envelope.velocity_level_sensitivity > 127
            || program.envelope.velocity_time_sensitivity > 127
            || program.envelope.key_tracking > 127
        {
            return Err("Invalid native EG2/amplifier program input".into());
        }
        self.commands
            .try_send(Command::AmplifierProgram(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn configure_pan(
        &self,
        tables: radias_synth_domain::controller_pan::PanTables,
        weights: radias_synth_domain::control_slew::SlewWeights,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::PanTables(Box::new(tables), weights))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_pan(
        &self,
        timbre: u8,
        pan: radias_synth_domain::controller_pan::PanControl,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::Pan(timbre, pan))
            .map_err(|e| e.to_string())
    }
    pub fn controller_filter_tables(
        &self,
        tables: radias_synth_domain::controller_filter::ControllerFilterTables,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::FilterTables(Box::new(tables)))
            .map_err(|e| e.to_string())
    }
    pub fn configure_mixer(
        &self,
        scales: radias_synth_domain::controller_mixer::MixerScales,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::MixerScales(Box::new(scales)))
            .map_err(|e| e.to_string())
    }
    pub fn configure_filter2(
        &self,
        tables: radias_synth_domain::controller_filter2::Filter2ControlTables,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::Filter2Tables(Box::new(tables)))
            .map_err(|e| e.to_string())?;
        self.filter2_available.store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn timbre_filter2_program(
        &self,
        timbre: u8,
        program: radias_synth_application::filter2::Filter2Program,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if !self.filter2_available.load(Ordering::Relaxed) {
            return Err("Native Filter2 controller tables have not been configured".into());
        }
        if program.route & 0x30 == 0x30 {
            return Err("Comb requires the Comb program compiler".into());
        }
        self.commands
            .try_send(Command::Filter2Program(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn configure_secondary(
        &self,
        table: radias_synth_domain::controller_secondary::FineTuneTable,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::SecondaryTable(Box::new(table)))
            .map_err(|e| e.to_string())
    }
    pub fn configure_note_pitch(
        &self,
        tables: radias_synth_domain::note_pitch::NotePitchTables,
        scale: radias_synth_domain::note_pitch::ScaleContext,
        master_tune: i32,
    ) -> Result<(), String> {
        if !self.modulation_available {
            return Err("Native pitch control requires controller/modulation tables".into());
        }
        self.commands
            .try_send(Command::NotePitchTables(
                Box::new(tables),
                scale,
                master_tune,
            ))
            .map_err(|e| e.to_string())
    }
    pub fn configure_portamento(
        &self,
        tables: radias_synth_application::portamento::PortamentoTables,
    ) -> Result<(), String> {
        if !self.modulation_available {
            return Err("Portamento requires controller/modulation tables".into());
        }
        self.commands
            .try_send(Command::PortamentoTables(Box::new(tables)))
            .map_err(|e| e.to_string())?;
        self.portamento_available.store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn timbre_portamento(
        &self,
        timbre: u8,
        program: radias_synth_domain::portamento::PortamentoProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if program.time > 127 || program.curve > 15 {
            return Err("Invalid portamento program".into());
        }
        self.commands
            .try_send(Command::Portamento(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_voice_mode(
        &self,
        timbre: u8,
        mode: radias_synth_domain::mono_notes::VoiceMode,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if !mode.polyphonic && !self.modulation_available {
            return Err("Mono requires native controller/modulation tables".into());
        }
        self.commands
            .try_send(Command::VoiceMode(timbre, mode))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_pitch(
        &self,
        timbre: u8,
        program: radias_synth_domain::note_pitch::PitchProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if [
            program.transpose,
            program.fine_tune,
            program.vibrato_intensity,
            program.bend_range,
        ]
        .iter()
        .any(|v| *v > 127)
        {
            return Err("Invalid pitch program data byte".into());
        }
        self.commands
            .try_send(Command::Pitch(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_secondary(&self, timbre: u8, program: SecondaryProgram) -> Result<(), String> {
        validate_timbre(timbre)?;
        if program.selection & !0x33 != 0
            || program.pitch.semitone > 127
            || program.pitch.fine_tune > 127
        {
            return Err("Unsupported native OSC2 input".into());
        }
        self.commands
            .try_send(Command::Secondary(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_primary_control(
        &self,
        timbre: u8,
        program: radias_synth_application::primary::PrimaryProgram,
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        if (program.selection & !0x33 != 0 && !matches!(program.selection, 4 | 5))
            || program.control.control1 > 127
            || program.control.control2 > 127
        {
            return Err("Unsupported native OSC1 CONTROL 1/2 input".into());
        }
        self.commands
            .try_send(Command::Primary(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn configure_noise(
        &self,
        pitch: PitchTable,
        noise: radias_synth_domain::noise_control::NoisePitchTable,
        counters: radias_synth_domain::controller_noise::FormantCounterSeeds,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::NoiseTables(Box::new(
                radias_synth_application::noise::NoiseTables {
                    pitch,
                    noise,
                    counters,
                },
            )))
            .map_err(|e| e.to_string())
    }

    pub fn configure_physical_frames(
        &self,
        seeds: [radias_synth_domain::noise::NoiseFrameSeeds; 2],
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::PhysicalFrames(Box::new(seeds)))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_mixer(&self, timbre: u8, program: MixerProgram) -> Result<(), String> {
        validate_timbre(timbre)?;
        if program.levels.iter().any(|&v| v > 127) {
            return Err("Invalid native mixer level".into());
        }
        self.commands
            .try_send(Command::Mixer(timbre, program))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_auxiliary(
        &self,
        timbre: u8,
        programs: [ModEnvelopeProgram; 2],
    ) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::Auxiliary(timbre, programs))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_dynamic_filter(&self, timbre: u8, filter: DynamicFilter) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::DynamicFilter(timbre, filter))
            .map_err(|e| e.to_string())
    }
    pub fn tempo(&self, tenths_bpm: u16) -> Result<(), String> {
        if !self.tempo_available {
            return Err("Tempo clock tables were not configured".into());
        }
        self.commands
            .try_send(Command::Tempo(tenths_bpm))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_waveform(&self, timbre: u8, index: usize) -> Result<(), String> {
        validate_timbre(timbre)?;
        self.commands
            .try_send(Command::Waveform(timbre, index))
            .map_err(|e| e.to_string())
    }
    pub fn timbre_note(&self, timbre: u8, note: u8, velocity: u8) -> Result<(), String> {
        validate_timbre(timbre)?;
        if note > 127 || velocity > 127 {
            return Err("Invalid MIDI note".into());
        }
        self.commands
            .try_send(Command::Note(timbre, note, velocity))
            .map_err(|e| e.to_string())
    }
    pub fn timbre(&self, timbre: u8, enabled: bool, channel: u8) -> Result<(), String> {
        validate_timbre(timbre)?;
        if channel > 15 {
            return Err("Invalid MIDI channel".into());
        }
        self.commands
            .try_send(Command::Timbre(timbre, enabled, channel))
            .map_err(|e| e.to_string())
    }
    pub fn gain(&self, gain: f32) {
        if gain.is_finite() {
            self.counters
                .gain
                .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }
    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            callbacks: self.counters.callbacks.load(Ordering::Relaxed),
            native_frames: self.counters.native_frames.load(Ordering::Relaxed),
            audible_frames: self.counters.audible_frames.load(Ordering::Relaxed),
            worst_render_ns: self.counters.worst_render_ns.load(Ordering::Relaxed),
            deadline_misses: self.counters.deadline_misses.load(Ordering::Relaxed),
            failed: self.counters.failed.load(Ordering::Relaxed),
            device: self.device.clone(),
            sample_rate: self.rate,
            active_voices: self.counters.active_voices.load(Ordering::Relaxed),
            held_voices: self.counters.held_voices.load(Ordering::Relaxed),
            output_peak: f32::from_bits(self.counters.output_peak.load(Ordering::Relaxed)),
        }
    }
}

fn validate_timbre(timbre: u8) -> Result<(), String> {
    if timbre as usize >= TIMBRE_COUNT {
        Err("Invalid timbre".into())
    } else {
        Ok(())
    }
}

fn stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut generator: Generator,
    counters: Arc<Counters>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let errors = counters.clone();
    let mut current = StereoFrame::default();
    let mut next = StereoFrame::default();
    let mut phase = 0u64;
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| {
                let start = Instant::now();
                generator.service();
                let gain = f32::from_bits(counters.gain.load(Ordering::Relaxed));
                let mut native = 0u64;
                let mut audible = 0u64;
                let mut output_peak = 0.0f32;
                for frame in output.chunks_mut(channels) {
                    let pair = if rate == 48_000 {
                        native += 1;
                        let s = generator.sample();
                        [s.left.0 as f64, s.right.0 as f64]
                    } else {
                        let fraction = phase as f64 / rate as f64;
                        let pair = [
                            current.left.0 as f64
                                + (next.left.0 as f64 - current.left.0 as f64) * fraction,
                            current.right.0 as f64
                                + (next.right.0 as f64 - current.right.0 as f64) * fraction,
                        ];
                        phase += 48_000;
                        while phase >= rate as u64 {
                            phase -= rate as u64;
                            current = next;
                            next = generator.sample();
                            native += 1;
                        }
                        pair
                    };
                    let left = (pair[0] / 2147483648.0 * gain as f64) as f32;
                    let right = (pair[1] / 2147483648.0 * gain as f64) as f32;
                    output_peak = output_peak.max(left.abs()).max(right.abs());
                    if left != 0.0 || right != 0.0 {
                        audible += 1;
                    }
                    for (i, value) in frame.iter_mut().enumerate() {
                        let sample = if channels == 1 {
                            (left + right) * 0.5
                        } else if i == 0 {
                            left
                        } else if i == 1 {
                            right
                        } else {
                            0.0
                        };
                        *value = T::from_sample(sample);
                    }
                }
                let elapsed = start.elapsed().as_nanos() as u64;
                counters
                    .active_voices
                    .store(generator.pool.active_count() as u32, Ordering::Relaxed);
                counters
                    .held_voices
                    .store(generator.pool.held_count() as u32, Ordering::Relaxed);
                for timbre in 0..TIMBRE_COUNT {
                    counters.sustain_flags[timbre].store(
                        generator.pool.sustain_state(timbre as u8).unwrap().flags as u32,
                        Ordering::Relaxed,
                    );
                    let note = (0..radias_synth_domain::voice_allocation::VOICE_COUNT)
                        .filter_map(|slot| generator.pool.active_voice(slot))
                        .find(|voice| voice.held && voice.timbre as usize == timbre)
                        .map_or(0, |voice| voice.note as u32 + 1);
                    counters.held_notes[timbre].store(note, Ordering::Relaxed);
                }
                counters.callbacks.fetch_add(1, Ordering::Relaxed);
                counters
                    .output_peak
                    .store(output_peak.to_bits(), Ordering::Relaxed);
                counters.native_frames.fetch_add(native, Ordering::Relaxed);
                counters
                    .audible_frames
                    .fetch_add(audible, Ordering::Relaxed);
                counters
                    .worst_render_ns
                    .fetch_max(elapsed, Ordering::Relaxed);
                if elapsed as f64 > output.len() as f64 / channels as f64 / rate as f64 * 1e9 {
                    counters.deadline_misses.fetch_add(1, Ordering::Relaxed);
                }
            },
            move |_| {
                errors.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}
