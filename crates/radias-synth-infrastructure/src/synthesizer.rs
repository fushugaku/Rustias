//! Shared native generator; devices supply commands and sample-clock calls.
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

pub struct NativeProgramLoad {
    pub compiled: crate::stored_program::CompiledProgram,
    pub drums: Option<Box<radias_synth_application::drum_program::CompiledDrumKit>>,
    pub vocoder: Option<Box<radias_synth_application::vocoder::VocoderRenderer>>,
    pub effects: Option<Box<radias_synth_application::effect_audio::EffectAudioRack>>,
}

#[cfg(test)]
mod bus_output_tests {
    use super::*;
    use crate::standalone::StandaloneSynth;

    #[test]
    fn vocoder_source_command_updates_targets_without_resetting_histories() {
        let root = crate::reference_root();
        let raw = std::fs::read(root.join("runs/vocoder-native-synth-program.bin")).unwrap();
        let program = radias_synth_domain::program::Program::from_bytes(&raw).unwrap();
        let mut bytes = *program.vocoder().bytes;
        bytes[40] = 2; // Original EG3 source; no Formant Shift.
        bytes[43] = 127;
        let stored = radias_synth_domain::vocoder_control::VocoderProgram { bytes: &bytes };
        let controls = crate::vocoder_tables::original();
        let mut renderer = radias_synth_application::vocoder::VocoderRenderer::from_program(
            stored,
            Default::default(),
            &controls,
            crate::vocoder_tables::interpolation(),
        )
        .unwrap();
        renderer.processor.state[0x94] = 1234;
        let before = renderer.processor.clone();
        let sources = radias_synth_domain::vocoder_sources::VocoderSources {
            actor: Some(radias_synth_domain::vocoder_sources::VocoderActorSources {
                envelope_outputs: [0, 0, 8192],
                ..Default::default()
            }),
            ..Default::default()
        };
        let expected = stored.frequency_offset(sources.read(bytes[40]), &controls) as u16;
        assert_ne!(expected, before.parameters[0xbf]);
        let mut instrument = StandaloneSynth::new();
        assert!(!instrument.engine.publish_vocoder_sources(sources));
        instrument
            .engine
            .set_vocoder_program(bytes, Box::new(renderer));
        instrument.engine.apply(Command::VocoderSources(sources));
        let after = &instrument.engine.vocoder().unwrap().processor;
        assert_eq!(after.state, before.state);
        assert_eq!(after.parameters[0xc0], before.parameters[0xc0]);
        assert_eq!(after.parameters[0xbf], expected);
    }

    #[test]
    fn disabled_vocoder_keeps_the_original_dry_output_and_its_histories() {
        let mut actual = StandaloneSynth::new();
        let mut reference = StandaloneSynth::new();
        for instrument in [&mut actual, &mut reference] {
            instrument.engine.apply(Command::Note(0, 60, 96));
        }
        let processor = radias_synth_domain::vocoder::Vocoder {
            parameters: [0; 352],
            state: [123; 300],
        };
        actual.engine.set_vocoder(Some(Box::new(
            radias_synth_application::vocoder::VocoderRenderer {
                processor: processor.clone(),
                tables: crate::vocoder_tables::interpolation(),
            },
        )));
        for _ in 0..256 {
            assert_eq!(actual.engine.sample(), reference.engine.sample());
        }
        assert_eq!(actual.engine.vocoder().unwrap().processor, processor);
    }

    #[test]
    fn instrument_vocoder_replaces_its_pair_and_keeps_other_timbres() {
        use radias_synth_domain::vocoder::{InterpolationTables, Vocoder, VocoderFrame};
        let root = crate::reference_root();
        let raw =
            std::fs::read(root.join("runs/native-clone/vocoder-audio-parameters.bin")).unwrap();
        let image = std::fs::read(root.join("firmware/dsp-master-host-stream.bin")).unwrap();
        let origin = u16::from_be_bytes(image[..2].try_into().unwrap()) as usize;
        let source = |address: usize| {
            let at = 2 + 2 * (address - origin);
            u16::from_be_bytes(image[at..at + 2].try_into().unwrap())
        };
        let tables = InterpolationTables {
            scalar_offsets: core::array::from_fn(|i| source(0x494b + i)),
            wide_offset: source(0x4971),
        };
        let mut processor = Vocoder {
            parameters: core::array::from_fn(|i| {
                u16::from_le_bytes(raw[2 * i..2 * i + 2].try_into().unwrap())
            }),
            state: [0; 300],
        };
        processor.parameters[0xb4] = 12;
        processor.parameters[0xb5] = 14;
        let mut actual = StandaloneSynth::new();
        let mut reference = StandaloneSynth::new();
        for instrument in [&mut actual, &mut reference] {
            for timbre in 0..4 {
                assert!(instrument.control(timbre, 71, 1));
                instrument
                    .engine
                    .apply(Command::Note(timbre, 48 + 4 * timbre, 96));
            }
        }
        actual.engine.set_vocoder(Some(Box::new(
            radias_synth_application::vocoder::VocoderRenderer {
                processor: processor.clone(),
                tables: tables.clone(),
            },
        )));
        let mut nonzero_unaffected = [false; 4];
        for index in 0..2048 {
            if index == 1024 {
                for instrument in [&mut actual, &mut reference] {
                    instrument.engine.apply(Command::Note(2, 56, 0));
                }
            }
            let input = StereoFrame {
                left: radias_synth_domain::Sample(index * 104729),
                right: radias_synth_domain::Sample(-index * 65537),
            };
            let dry = reference.engine.sample_buses()[0];
            let mut frame = VocoderFrame::from_buses(dry, input);
            processor.process(&mut frame, true, &tables).unwrap();
            let rendered = frame.buses();
            for timbre in [0, 2, 3] {
                assert_eq!(rendered[timbre], dry[timbre]);
                nonzero_unaffected[timbre] |= dry[timbre] != StereoFrame::default();
            }
            let output = actual.engine.sample_with_input(input, true).unwrap();
            assert_eq!(
                output.left.0,
                radias_synth_domain::fixed::saturate(
                    rendered.iter().map(|f| i64::from(f.left.0)).sum()
                )
            );
            assert_eq!(
                output.right.0,
                radias_synth_domain::fixed::saturate(
                    rendered.iter().map(|f| i64::from(f.right.0)).sum()
                )
            );
            assert_eq!(actual.engine.vocoder().unwrap().processor, processor);
        }
        assert!(nonzero_unaffected[0] && nonzero_unaffected[2] && nonzero_unaffected[3]);
    }

    #[test]
    fn bus_api_keeps_the_existing_sample_clock_and_all_four_timbres() {
        let mut exposed = StandaloneSynth::new();
        let mut previous = StandaloneSynth::new();
        for instrument in [&mut exposed, &mut previous] {
            for timbre in 0..4 {
                assert!(instrument.control(timbre, 71, 1));
                assert!(instrument.control(timbre, 0, i32::from(timbre)));
                assert!(instrument.control(timbre, 7, 40));
                for note in 0..4 {
                    instrument
                        .engine
                        .apply(Command::Note(timbre, 48 + 3 * timbre + note, 96));
                }
            }
        }
        let mut heard = [false; 4];
        let mut heard_slave = false;
        let mut distinct_from_double_slave = false;
        for frame in 0..4096 {
            if frame == 2048 {
                for instrument in [&mut exposed, &mut previous] {
                    for timbre in 0..4 {
                        instrument
                            .engine
                            .apply(Command::Note(timbre, 48 + 3 * timbre, 0));
                    }
                }
            }
            let engine = &mut previous.engine;
            // Unchanged application projection used before the new public API.
            let old_sample = engine.pool.next_sample_with_modulation(
                &engine.table,
                engine.controller_tables.as_ref(),
                engine.modulation_tables.as_ref(),
                |program| &engine.plans[program].events,
            );
            if frame % 3 == 1 {
                assert_eq!(
                    exposed.engine.sample(),
                    old_sample,
                    "mixed API clock at {frame}"
                );
                continue;
            }
            let buses = exposed.engine.sample_buses();
            for (timbre, bus) in buses[0].iter().enumerate() {
                heard[timbre] |= bus.left.0 != 0 || bus.right.0 != 0;
            }
            heard_slave |= buses[1]
                .iter()
                .any(|bus| bus.left.0 != 0 || bus.right.0 != 0);
            let sum = |frames: &[StereoFrame]| {
                let left: i64 = frames.iter().map(|f| i64::from(f.left.0)).sum();
                let right: i64 = frames.iter().map(|f| i64::from(f.right.0)).sum();
                StereoFrame {
                    left: radias_synth_domain::Sample(radias_synth_domain::fixed::saturate(left)),
                    right: radias_synth_domain::Sample(radias_synth_domain::fixed::saturate(right)),
                }
            };
            assert_eq!(sum(&buses[0]), old_sample, "separate buses at {frame}");
            distinct_from_double_slave |= sum(&buses.concat()) != old_sample;
        }
        assert_eq!(heard, [true; 4]);
        assert!(
            heard_slave,
            "workload must reach the second synthesis processor"
        );
        assert!(
            distinct_from_double_slave,
            "guard must detect counting Slave output twice"
        );
        assert_eq!(
            exposed.engine.active_count(),
            previous.engine.active_count()
        );
    }
}
pub enum Command {
    Effects(Box<radias_synth_application::effect_audio::EffectAudioRack>),
    Effect(
        usize,
        Box<radias_synth_domain::effect_audio::EffectAudioSettings>,
    ),
    EffectControllers([f32; 13]),
    Program(Box<NativeProgramLoad>),
    Start,
    Stop,
    Note(u8, u8, u8),
    DrumPad(u8, u8),
    DrumInstrument(
        u8,
        Box<radias_synth_application::drum_program::DrumInstrumentProgram>,
    ),
    Midi(u8, u8, u8),
    Bend(u8, u16),
    Wheel(u8, u8),
    Expression(u8, u8),
    Performance(radias_synth_domain::performance::GlobalPerformance),
    ControllerService(radias_synth_domain::controller_service::ControllerServiceTimer),
    AmplifierDelivery(radias_synth_domain::amplifier_delivery::AmplifierRateTable),
    ConstructorFilterMix(Box<radias_synth_domain::filter_control::FilterMixTable>),
    PitchDelivery(
        Box<[radias_synth_domain::pitch_receiver::PitchReceiverRom; 2]>,
        radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable,
    ),
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
    VocoderSources(radias_synth_domain::vocoder_sources::VocoderSources),
}
pub struct Synthesizer {
    plans: Box<[PreparedVoice]>,
    pub(crate) timbres: [Timbre; TIMBRE_COUNT],
    voice_costs: Option<VoiceCostTables>,
    table: WaveformTable,
    pub(crate) pool: Box<PolyphonicRenderer>,
    vocoder: Option<Box<radias_synth_application::vocoder::VocoderRenderer>>,
    vocoder_program: Option<[u8; radias_synth_domain::vocoder_control::STORED_BYTES]>,
    effects: Option<Box<radias_synth_application::effect_audio::EffectAudioRack>>,
    #[cfg(target_arch = "wasm32")]
    buffer: [[[StereoFrame; TIMBRE_COUNT]; 2]; 128],
    #[cfg(target_arch = "wasm32")]
    position: usize,
    tuning: Option<(PitchTable, BandwidthTable)>,
    controller_tables: Option<ControllerTables>,
    modulation_tables: Option<VoiceModulationTables>,
    midi_pitch: [radias_synth_application::note_pitch::MidiPitch; 16],
    portamento_switches: [bool; 16],
    expression: radias_synth_domain::performance::ExpressionState,
    performance: radias_synth_domain::performance::GlobalPerformance,
    performance_enabled: bool,
    drums: Option<Box<radias_synth_application::drum_program::CompiledDrumKit>>,
    pub(crate) unsupported_drum_notes: u64,
    drum_pads: radias_synth_domain::drum_pad::DrumPadState,
}

#[derive(Clone, Copy)]
pub(crate) struct Timbre {
    parameter_template: Option<(
        u8,
        radias_synth_domain::parameter_template::ParameterTemplate,
    )>,
    receive_flags: u8,
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
    pub(crate) amplifier: AmplifierProgram,
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
#[derive(Clone, Copy)]
struct DrumNoteControls {
    pitch: radias_synth_domain::note_pitch::PitchProgram,
    group: u8,
    filter2: Option<radias_synth_application::filter2::Filter2Program>,
    instrument: u8,
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
            return Err("Controller tables absent".into());
        }
        if tempo_tables.is_some() && modulation_tables.is_none() {
            return Err("Modulation tables absent".into());
        }
        let mut pool = Box::new(PolyphonicRenderer::default());
        if let Some(tables) = tempo_tables {
            pool.enable_tempo_clock(tables, 1200);
        }
        Ok(Self {
            plans: plans.into_boxed_slice(),
            timbres: core::array::from_fn(|i| Timbre {
                parameter_template: None,
                receive_flags: 255,
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
            vocoder: None,
            vocoder_program: None,
            effects: None,
            #[cfg(target_arch = "wasm32")]
            buffer: [[[StereoFrame::default(); TIMBRE_COUNT]; 2]; 128],
            #[cfg(target_arch = "wasm32")]
            position: 128,
            tuning,
            controller_tables,
            modulation_tables,
            midi_pitch: [Default::default(); 16],
            portamento_switches: [false; 16],
            expression: Default::default(),
            performance: Default::default(),
            performance_enabled: false,
            drums: None,
            unsupported_drum_notes: 0,
            drum_pads: Default::default(),
        })
    }
    #[cfg(feature = "web-modular")]
    pub fn set_circuit(
        &mut self,
        timbre: usize,
        prototype: Option<Box<dyn radias_synth_application::VoiceCircuit>>,
    ) {
        self.pool.set_circuit(timbre, prototype);
    }
    pub fn waveform_table(&self) -> &WaveformTable {
        &self.table
    }
    pub fn active_count(&self) -> usize {
        self.pool.active_count()
    }
    pub fn set_drum_gain(&mut self, gain: f64) {
        self.pool.set_drum_gain(gain);
    }
    pub fn steal_oldest_voice(&mut self) -> bool {
        self.pool.steal_oldest_voice()
    }
    pub fn held_count(&self) -> usize {
        self.pool.held_count()
    }
    pub fn set_key_window(&mut self, timbre: u8, window: [u8; 2]) {
        self.timbres[timbre as usize].key_window = window;
    }
    pub fn set_receive_flags(&mut self, timbre: u8, flags: u8) {
        self.timbres[timbre as usize].receive_flags = flags;
    }
    pub fn drum_kit(&mut self, kit: Box<radias_synth_application::drum_program::CompiledDrumKit>) {
        self.pool.stop();
        #[cfg(target_arch = "wasm32")]
        {
            self.buffer
                .fill([[StereoFrame::default(); TIMBRE_COUNT]; 2]);
            self.position = 128;
        }
        self.drum_pads = Default::default();
        self.drums = Some(kit);
        if let (Some(kit), Some(tables)) = (&self.drums, &self.controller_tables) {
            self.pool.edit_program_common(
                radias_synth_domain::program_binding::ProgramCommon {
                    level: kit.program.level,
                    pan: kit.program.pan,
                },
                tables,
            );
        }
    }
    pub fn update_drum_mapping(&mut self, index: usize, note: u8, group: u8) {
        if let Some(kit) = &mut self.drums {
            let mut raw = *kit.kit.bytes();
            raw[36 + index] = note;
            raw[18 + index] = group;
            kit.kit = radias_synth_domain::drum::DrumKit::from_bytes(&raw).unwrap();
        }
    }
    pub fn update_drum_common(&mut self, level: u8, pan: u8, transpose: u8) {
        if let Some(kit) = &mut self.drums {
            kit.program.level = level;
            kit.program.pan = pan;
            kit.program.transpose = transpose;
            if let Some(tables) = &self.controller_tables {
                self.pool.edit_program_common(
                    radias_synth_domain::program_binding::ProgramCommon { level, pan },
                    tables,
                );
            }
        }
    }
    pub fn clear_drums(&mut self) {
        self.pool.stop();
        #[cfg(target_arch = "wasm32")]
        {
            self.buffer
                .fill([[StereoFrame::default(); TIMBRE_COUNT]; 2]);
            self.position = 128;
        }
        self.drum_pads = Default::default();
        self.drums = None;
    }

    pub fn set_performance_enabled(&mut self, enabled: bool) {
        self.performance_enabled = enabled;
    }
    pub fn set_source_gain(&mut self, timbre: u8, gain: u16) {
        self.timbres[timbre as usize].amplifier.source_gain = gain;
        if let Some(tables) = &self.controller_tables {
            self.pool.edit_source_gain(timbre, gain, tables);
        }
    }
    pub fn source_gain(&self, timbre: u8) -> u16 {
        self.timbres[timbre as usize].amplifier.source_gain
    }
    fn update_expression_gains(&mut self) {
        for (index, timbre) in self.timbres.iter_mut().enumerate() {
            let gain = self
                .expression
                .gain(timbre.channel, timbre.receive_flags, self.performance);
            if timbre.amplifier.source_gain != gain {
                timbre.amplifier.source_gain = gain;
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_source_gain(index as u8, gain, tables);
                }
            }
        }
    }
    fn note_on(&mut self, timbre: u8, note: u8, velocity: u8, retrigger: bool) {
        let settings = self.timbres[timbre as usize];
        self.note_on_program(timbre, note, velocity, retrigger, settings, None);
    }
    fn note_on_program(
        &mut self,
        timbre: u8,
        note: u8,
        velocity: u8,
        retrigger: bool,
        settings: Timbre,
        drum: Option<DrumNoteControls>,
    ) {
        if !settings.enabled {
            return;
        }
        if let Some(effects) = &mut self.effects {
            effects.note_on(usize::from(timbre), note, velocity);
        }
        let plan = &self.plans[settings.waveform];
        let synthesis_note = radias_synth_domain::note_pitch::fold_note(
            if drum.is_some() { 60 } else { note as i32 } + settings.pitch.transpose as i32 - 64,
        );
        let pitch_code = synthesis_note as u16 * 256;
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        let template_primary = settings
            .parameter_template
            .filter(|(selection, _)| *selection == settings.primary.selection)
            .and_then(|(_, template)| {
                radias_synth_domain::primary_parameters::decode(&template.words)
            });
        if let Some(primary) = template_primary {
            renderer.select_primary(primary);
        }
        renderer.reset_filter_memory();
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
            if template_primary.is_none()
                && let Some(primary) = settings
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
            uses_program_common: drum.is_some(),
            drum_pitch: drum.map(|d| d.pitch),
            drum_instrument: drum.map(|d| d.instrument),
            drum_filter2: drum.and_then(|d| d.filter2),
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
            if let Some(drum) = drum {
                self.pool
                    .trigger_drum_modulated(voice, cost, settings.modulation, drum.group);
            } else if retrigger {
                self.pool
                    .retrigger_modulated(voice, cost, settings.modulation);
            } else {
                self.pool
                    .trigger_modulated(voice, cost, settings.modulation);
            }
        } else {
            self.pool.trigger(voice, cost);
        };
    }
    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Effects(rack) => {
                self.effects = Some(rack);
            }
            Command::Effect(slot, settings) => {
                if let Some(rack) = &mut self.effects {
                    let _ = rack.configure(slot, *settings);
                }
            }
            Command::EffectControllers(controllers) => {
                if let Some(rack) = &mut self.effects {
                    rack.set_controllers(controllers);
                }
            }
            Command::VocoderSources(sources) => {
                self.publish_vocoder_sources(sources);
            }
            Command::Program(program) => {
                let NativeProgramLoad {
                    compiled: program,
                    drums,
                    vocoder,
                    effects,
                } = *program;
                self.drums = drums;
                self.vocoder_program = vocoder.as_ref().map(|_| program.vocoder);
                self.vocoder = vocoder;
                self.effects = effects;
                self.performance_enabled = true;
                self.pool.stop();
                self.pool.set_tempo(program.stored.tempo_tenths);
                if let Some(tables) = &self.controller_tables {
                    self.pool.edit_program_common(
                        radias_synth_domain::program_binding::ProgramCommon {
                            level: program.stored.drum.level,
                            pan: program.stored.drum.pan,
                        },
                        tables,
                    );
                }
                for timbre in 0..TIMBRE_COUNT {
                    let source = program.stored.timbres[timbre];
                    let c = source.controls;
                    let compiled = program.timbres[timbre];
                    self.timbres[timbre] = Timbre {
                        parameter_template: compiled
                            .parameter_template
                            .map(|template| (c.oscillator_selection, template)),
                        receive_flags: source.receive_flags,
                        key_window: source.key_window,
                        pitch: c.pitch,
                        portamento: c.portamento,
                        waveform: (c.oscillator_selection & 3) as usize,
                        filter: Some(compiled.filter),
                        shaper: c.shaper,
                        comb_program: compiled.comb,
                        filter_routing: Some((compiled.filter_routing, compiled.filter2)),
                        amplifier: c.amplifier(
                            self.expression.gain(
                                source.channel,
                                source.receive_flags,
                                self.performance,
                            ),
                            None,
                            0,
                        ),
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
            Command::DrumPad(instrument, velocity) => {
                if let Some(kit) = &self.drums {
                    let owner = kit.program.timbre.unwrap();
                    let Some(event) = self.drum_pads.input(
                        kit.program,
                        radias_synth_domain::drum_pad::DrumPadInput {
                            instrument,
                            velocity,
                            owning_timbre_enabled: self.timbres[owner as usize].enabled,
                            owning_channel: self.timbres[owner as usize].channel,
                            key: kit.kit.bytes()[36 + instrument as usize],
                        },
                    ) else {
                        return;
                    };
                    self.drum_instrument_event(
                        event.timbre,
                        event.instrument as usize,
                        event.event,
                    );
                }
            }
            Command::DrumInstrument(index, next) => {
                if let (Some(drums), Some(tables), Some(modulation)) = (
                    &mut self.drums,
                    &self.controller_tables,
                    &self.modulation_tables,
                ) {
                    let previous = drums.instruments[index as usize];
                    self.pool.edit_drum_instrument(
                        drums.program.timbre.unwrap(),
                        index,
                        previous,
                        *next,
                        tables,
                        modulation,
                    );
                    if let Some(costs) = &self.voice_costs
                        && let Some(cost) = costs.cost(VoiceCostParameters {
                            primary: next.controls.oscillator_selection,
                            secondary: next.controls.secondary.selection,
                            filter_route: next.controls.filter_route,
                            drive_mode: next.controls.shaper.allocation_mode(),
                            shaper_type: next.controls.shaper.allocation_type(),
                        })
                    {
                        for slot in 0..24 {
                            if self.pool.active_voice(slot).is_some_and(|v| {
                                v.timbre == drums.program.timbre.unwrap()
                                    && v.drum_instrument == Some(index)
                            }) {
                                self.pool.allocator.budget.costs[slot] = cost as u16;
                            }
                        }
                    }
                    drums.instruments[index as usize] = *next;
                }
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
                if let Some(effects) = &mut self.effects {
                    effects.release_notes(None);
                }
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
                        if let Some(effects) = &mut self.effects {
                            effects.set_controller(timbre, 2, (f32::from(raw) - 8192.0) / 8192.0);
                        }
                    }
                }
                if channel == self.performance.channel
                    && let Some(effects) = &mut self.effects
                {
                    effects.set_controller(4, 2, (f32::from(raw) - 8192.0) / 8192.0);
                }
            }
            Command::Wheel(channel, value) => {
                self.midi_pitch[channel as usize].wheel = value;
                for timbre in 0..TIMBRE_COUNT {
                    if self.timbres[timbre].channel == channel {
                        self.pool
                            .set_midi_pitch(timbre as u8, self.midi_pitch[channel as usize]);
                        if let Some(effects) = &mut self.effects {
                            effects.set_controller(timbre, 3, f32::from(value) / 127.0);
                        }
                    }
                }
                if channel == self.performance.channel
                    && let Some(effects) = &mut self.effects
                {
                    effects.set_controller(4, 3, f32::from(value) / 127.0);
                }
            }
            Command::Expression(channel, value) => {
                self.expression.set(channel, value);
                if self.performance_enabled {
                    self.update_expression_gains();
                }
            }
            Command::Performance(global) => {
                self.performance = global;
                self.performance_enabled = true;
                self.update_expression_gains();
            }
            Command::ControllerService(timer) => self.pool.configure_controller_service(timer),
            Command::AmplifierDelivery(rates) => self.pool.configure_amplifier_delivery(rates),
            Command::ConstructorFilterMix(table) => {
                self.pool.configure_constructor_filter_mix(*table)
            }
            Command::PitchDelivery(rom, dispatch) => {
                self.pool.configure_pitch_delivery(*rom, dispatch)
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
            Command::AmplifierProgram(timbre, mut program) => {
                if self.performance_enabled {
                    let t = self.timbres[timbre as usize];
                    program.source_gain =
                        self.expression
                            .gain(t.channel, t.receive_flags, self.performance);
                }
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
            Command::Tempo(tempo) => {
                self.pool.set_tempo(tempo);
                if let Some(effects) = &mut self.effects {
                    effects.set_tempo(tempo);
                }
            }
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
                if self.performance_enabled {
                    self.update_expression_gains();
                }
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
        if velocity == 0
            && let Some(effects) = &mut self.effects
        {
            effects.note_off(usize::from(timbre), note);
        }
        if self
            .drums
            .as_ref()
            .is_some_and(|d| d.program.timbre == Some(timbre))
        {
            let mask = self
                .drums
                .as_ref()
                .unwrap()
                .kit
                .trigger_mask(note, self.drums.as_ref().unwrap().program.transpose);
            for index in 0..16 {
                if mask & (1 << index) == 0 {
                    continue;
                }
                let event = ((0x10 | self.timbres[timbre as usize].channel as u32) << 24)
                    | ((velocity as u32) << 8)
                    | note as u32
                    | if velocity != 0 { 128 } else { 0 };
                self.drum_instrument_event(timbre, index, event);
            }
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
    fn drum_instrument_event(&mut self, timbre: u8, index: usize, event: u32) {
        let note = event as u8 & 127;
        let velocity = ((event >> 8) & 127) as u8;
        let kit = self.drums.as_ref().unwrap();
        let instrument = kit.instruments[index];
        let c = instrument.controls;
        let graph = instrument.graph;
        let common = kit.program;
        let group = kit.kit.exclusive_group(index).unwrap();
        self.pool.begin_note_event((event >> 24) as u8);
        if event as u8 & 128 == 0 {
            self.pool
                .release_drum_note(timbre, note, self.controller_tables.as_ref());
        } else {
            let selection = c.oscillator_selection & 63;
            if selection & 15 >= 6 || (selection & 15 >= 4 && selection & 48 != 0) {
                self.unsupported_drum_notes += 1;
                self.pool.finish_note_event();
                return;
            }
            let owner = self.timbres[timbre as usize];
            let settings = Timbre {
                parameter_template: graph
                    .parameter_template
                    .map(|template| (c.oscillator_selection, template)),
                pitch: c.pitch,
                waveform: if selection & 15 < 4 {
                    (selection & 3) as usize
                } else {
                    0
                },
                filter: Some(graph.filter),
                shaper: c.shaper,
                comb_program: graph.comb,
                filter_routing: Some((graph.filter_routing, graph.filter2)),
                amplifier: c.amplifier(owner.amplifier.source_gain, Some(common.level), 0),
                pan: radias_synth_domain::controller_pan::PanControl {
                    position: c.pan,
                    midi_pan: Some(common.pan),
                    ..Default::default()
                },
                mixer: c.mixer,
                secondary: c.secondary,
                primary: c.primary(),
                modulation: c.modulation,
                auxiliary: [c.envelope[0], c.envelope[2]],
                dynamic_filter: Some(graph.dynamic_filter),
                ..owner
            };
            self.note_on_program(
                timbre,
                note,
                velocity,
                false,
                settings,
                Some(DrumNoteControls {
                    pitch: c.pitch,
                    group,
                    filter2: graph.dynamic_filter2,
                    instrument: index as u8,
                }),
            );
        }
        self.pool.finish_note_event();
    }
    /// Advance the instrument once, retaining each processor's four stereo
    /// pairs for the effects input. Master already includes Slave ingress;
    /// summing both processors would count the Slave voices twice.
    /// `sample` and this method share one clock and one WASM block cursor.
    pub fn sample_buses(&mut self) -> [[StereoFrame; TIMBRE_COUNT]; 2] {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pool.next_buses_with_modulation(
                &self.table,
                self.controller_tables.as_ref(),
                self.modulation_tables.as_ref(),
                |program| &self.plans[program].events,
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            if self.position == 128 {
                for sample in &mut self.buffer {
                    *sample = self.pool.next_buses_with_modulation(
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

    /// Install compiled data, without a firmware interpreter or audio-thread
    /// allocation. Native RDL vocoder compilation belongs to the patch loader.
    pub fn set_vocoder(
        &mut self,
        renderer: Option<Box<radias_synth_application::vocoder::VocoderRenderer>>,
    ) {
        self.vocoder = renderer;
        self.vocoder_program = None;
    }
    pub fn set_vocoder_program(
        &mut self,
        program: [u8; radias_synth_domain::vocoder_control::STORED_BYTES],
        renderer: Box<radias_synth_application::vocoder::VocoderRenderer>,
    ) {
        self.vocoder_program = Some(program);
        self.vocoder = Some(renderer);
    }
    pub fn publish_vocoder_sources(
        &mut self,
        sources: radias_synth_domain::vocoder_sources::VocoderSources,
    ) -> bool {
        let (Some(renderer), Some(bytes)) = (&mut self.vocoder, &self.vocoder_program) else {
            return false;
        };
        renderer.publish_sources(
            radias_synth_domain::vocoder_control::VocoderProgram { bytes },
            sources,
            &crate::vocoder_tables::original(),
        );
        true
    }
    pub fn vocoder(&self) -> Option<&radias_synth_application::vocoder::VocoderRenderer> {
        self.vocoder.as_deref()
    }
    /// Advance voices and their selected vocoder pair once. Master already
    /// contains Slave ingress; no second Slave mix is added here.
    pub fn sample_with_input(
        &mut self,
        input: StereoFrame,
        interpolate_vocoder: bool,
    ) -> Result<StereoFrame, radias_synth_domain::vocoder::VocoderError> {
        self.sample_with_sources([input, StereoFrame::default()], interpolate_vocoder)
    }
    pub fn sample_with_sources(
        &mut self,
        inputs: [StereoFrame; 2],
        interpolate_vocoder: bool,
    ) -> Result<StereoFrame, radias_synth_domain::vocoder::VocoderError> {
        let mut frame = radias_synth_domain::vocoder::VocoderFrame::from_sources(
            [StereoFrame::default(); TIMBRE_COUNT],
            inputs,
        );
        self.sample_frame(&mut frame, interpolate_vocoder)?;
        let buses = frame.buses();
        if let Some(effects) = &mut self.effects {
            return Ok(effects.process(buses));
        }
        let (left, right) = buses.iter().fold((0i64, 0i64), |(left, right), bus| {
            (left + i64::from(bus.left.0), right + i64::from(bus.right.0))
        });
        Ok(StereoFrame {
            left: radias_synth_domain::Sample(radias_synth_domain::fixed::saturate(left)),
            right: radias_synth_domain::Sample(radias_synth_domain::fixed::saturate(right)),
        })
    }

    /// Advance both voice processors once and publish the eight Master mix
    /// words at the original A333 frame offset. Preserve all caller-supplied
    /// input, auxiliary and transport words for the subsequent processing path.
    pub fn sample_frame(
        &mut self,
        frame: &mut radias_synth_domain::vocoder::VocoderFrame,
        interpolate_vocoder: bool,
    ) -> Result<(), radias_synth_domain::vocoder::VocoderError> {
        let buses = self.sample_buses()[0];
        for (timbre, bus) in buses.iter().enumerate() {
            frame.samples[4 + 2 * timbre] = bus.left.0;
            frame.samples[5 + 2 * timbre] = bus.right.0;
        }
        if let Some(vocoder) = &mut self.vocoder
            && vocoder.processor.parameters[0] != 0
        {
            vocoder
                .processor
                .process(frame, interpolate_vocoder, &vocoder.tables)?;
        }
        Ok(())
    }

    /// Process a real working-buffer frame before TX conversion. Invalid frame
    /// indexes are rejected before advancing any voice or vocoder history.
    pub fn sample_exchange_frame(
        &mut self,
        exchange: &mut radias_synth_application::dsp_audio_exchange::DspAudioExchange,
        index: usize,
        interpolate_vocoder: bool,
    ) -> Result<(), radias_synth_application::vocoder::VocoderBlockError> {
        use radias_synth_application::vocoder::VocoderBlockError;
        use radias_synth_domain::vocoder::VocoderError;
        let mut frame = exchange
            .vocoder_frame(index)
            .map_err(|_| VocoderBlockError::Sample {
                frame: index,
                error: VocoderError::FrameRoute,
            })?;
        self.sample_frame(&mut frame, interpolate_vocoder)
            .map_err(|error| VocoderBlockError::Sample {
                frame: index,
                error,
            })?;
        exchange
            .replace_vocoder_frame(index, &frame)
            .map_err(|_| VocoderBlockError::Sample {
                frame: index,
                error: VocoderError::FrameRoute,
            })?;
        Ok(())
    }
    /// Existing audition projection, before the unfinished FXD03 stage.
    /// A configured vocoder receives digital-zero external input here.
    pub fn sample(&mut self) -> StereoFrame {
        self.sample_with_input(StereoFrame::default(), true)
            .expect("Invalid compiled vocoder routes")
    }
}
