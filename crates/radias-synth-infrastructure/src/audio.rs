//! Direct device and deterministic recording adapters for the same native engine.
use crate::prepared::PreparedVoice;
use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use radias_synth_application::amplifier::{AmplifierProgram, ControllerTables};
use radias_synth_application::mixer::MixerProgram;
use radias_synth_application::modulation::{ModulationProgram, VoiceModulationTables};
use radias_synth_application::polyphony::{ActiveVoice, TIMBRE_COUNT};
use radias_synth_application::secondary::SecondaryProgram;
use radias_synth_application::voice_envelopes::{DynamicFilter, ModEnvelopeProgram};
use radias_synth_domain::{
    bandlimit::BandwidthTable, filter::FilterCoefficients, pan::StereoFrame, pitch::PitchTable,
    voice_allocation::VoiceCostTables, waveform::WaveformTable,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};

use crate::synthesizer::{Command, NativeProgramLoad};
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
    source_gains: [AtomicU32; TIMBRE_COUNT],
    unsupported_drum_notes: AtomicU64,
    actor_pitch_codes: [AtomicU32; 24],
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
    _stream: Option<cpal::Stream>,
}

/// Deterministic Q31 recording through the production command and note factory.
/// No audio device or firmware interpreter is opened. Controls are serviced once
/// per submitted block, as in the device callback. State advances only for the
/// requested samples, so events never act on a precomputed future state.
pub struct NativeOfflineRenderer {
    player: NativePlayer,
    generator: Generator,
}
impl NativeOfflineRenderer {
    pub fn with_clock(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
        voice_costs: Option<VoiceCostTables>,
        modulation_tables: Option<VoiceModulationTables>,
        tempo_tables: Option<radias_synth_domain::lfo_tempo::LfoTempoTables>,
    ) -> Result<Self, String> {
        let (mut player, generator) = NativePlayer::prepare_generator(
            plans,
            table,
            tuning,
            controller_tables,
            voice_costs,
            modulation_tables,
            tempo_tables,
        )?;
        player.device = "Native Q31 recording".into();
        player.rate = 48_000;
        player.gain(1.0);
        Ok(Self { player, generator })
    }
    /// Uses every existing validated desktop/MIDI control without duplicating it.
    pub fn controls(&self) -> &NativePlayer {
        &self.player
    }
    /// Read-only comparison state; observing it never advances or replaces DSP data.
    pub fn active_voice(&self, slot: usize) -> Option<&ActiveVoice> {
        if slot >= radias_synth_domain::voice_allocation::VOICE_COUNT {
            return None;
        }
        self.generator.pool.active_voice(slot)
    }
    pub fn amplifier_delivery_state(&self) -> (usize, bool) {
        self.generator.pool.amplifier_delivery_state()
    }
    pub fn controller_service_state(
        &self,
    ) -> Option<(
        radias_synth_domain::controller_service::ControllerServiceTimer,
        u64,
    )> {
        self.generator.pool.controller_service_state()
    }
    /// Raw dry output, before the desktop monitoring gain and device conversion.
    pub fn render_into(&mut self, output: &mut [StereoFrame]) {
        self.generator.service();
        let mut audible = 0;
        let mut peak = 0.0f32;
        for frame in output.iter_mut() {
            *frame = self.generator.sample();
            audible += u64::from(frame.left.0 != 0 || frame.right.0 != 0);
            peak = peak.max(frame.left.0.unsigned_abs() as f32 / 2_147_483_648.0);
            peak = peak.max(frame.right.0.unsigned_abs() as f32 / 2_147_483_648.0);
        }
        let counters = &self.player.counters;
        self.generator.publish_voice_counters(counters);
        counters.callbacks.fetch_add(1, Ordering::Relaxed);
        counters
            .native_frames
            .fetch_add(output.len() as u64, Ordering::Relaxed);
        counters
            .audible_frames
            .fetch_add(audible, Ordering::Relaxed);
        counters
            .output_peak
            .store(peak.to_bits(), Ordering::Relaxed);
    }
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
            0xb0 if message[1] == 11 => Command::Expression(message[0] & 15, message[2]),
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
    synth: crate::synthesizer::Synthesizer,
    commands: Receiver<Command>,
}
impl std::ops::Deref for Generator {
    type Target = crate::synthesizer::Synthesizer;
    fn deref(&self) -> &Self::Target {
        &self.synth
    }
}
impl std::ops::DerefMut for Generator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.synth
    }
}
impl Generator {
    fn service(&mut self) {
        for _ in 0..64 {
            let Ok(command) = self.commands.try_recv() else {
                break;
            };
            self.synth.apply(command);
        }
    }
    fn sample(&mut self) -> StereoFrame {
        self.synth.sample()
    }
    fn publish_voice_counters(&self, counters: &Counters) {
        if self.pool.amplifier_delivery_state().1 {
            counters.failed.store(true, Ordering::Relaxed);
        }
        counters
            .active_voices
            .store(self.pool.active_count() as u32, Ordering::Relaxed);
        counters
            .held_voices
            .store(self.pool.held_count() as u32, Ordering::Relaxed);
        for timbre in 0..TIMBRE_COUNT {
            counters.source_gains[timbre].store(
                self.timbres[timbre].amplifier.source_gain as u32,
                Ordering::Relaxed,
            );
            counters.sustain_flags[timbre].store(
                self.pool.sustain_state(timbre as u8).unwrap().flags as u32,
                Ordering::Relaxed,
            );
            let note = (0..radias_synth_domain::voice_allocation::VOICE_COUNT)
                .filter_map(|slot| self.pool.active_voice(slot))
                .find(|voice| voice.held && voice.timbre as usize == timbre)
                .map_or(0, |voice| voice.note as u32 + 1);
            counters.held_notes[timbre].store(note, Ordering::Relaxed);
        }
        for slot in 0..24 {
            counters.actor_pitch_codes[slot].store(
                self.pool
                    .active_voice(slot)
                    .map_or(u32::MAX, |v| v.renderer.primary_pitch_code() as u32),
                Ordering::Relaxed,
            );
        }
        counters
            .unsupported_drum_notes
            .store(self.unsupported_drum_notes, Ordering::Relaxed);
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
    pub fn source_gains(&self) -> [u16; TIMBRE_COUNT] {
        core::array::from_fn(|i| self.counters.source_gains[i].load(Ordering::Relaxed) as u16)
    }
    pub fn unsupported_drum_notes(&self) -> u64 {
        self.counters.unsupported_drum_notes.load(Ordering::Relaxed)
    }
    pub fn actor_pitch_codes(&self) -> [u32; 24] {
        core::array::from_fn(|i| self.counters.actor_pitch_codes[i].load(Ordering::Relaxed))
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
        let (mut player, generator) = Self::prepare_generator(
            plans,
            table,
            tuning,
            controller_tables,
            voice_costs,
            modulation_tables,
            tempo_tables,
        )?;
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
        let counters = player.counters.clone();
        let stream = match format {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config, generator, counters),
            cpal::SampleFormat::F64 => stream::<f64>(&device, &config, generator, counters),
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config, generator, counters),
            cpal::SampleFormat::I32 => stream::<i32>(&device, &config, generator, counters),
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config, generator, counters),
            _ => return Err(format!("Unsupported audio output format: {format:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        player.device = name;
        player.rate = config.sample_rate;
        player._stream = Some(stream);
        Ok(player)
    }
    fn prepare_generator(
        plans: Vec<PreparedVoice>,
        table: WaveformTable,
        tuning: Option<(PitchTable, BandwidthTable)>,
        controller_tables: Option<ControllerTables>,
        voice_costs: Option<VoiceCostTables>,
        modulation_tables: Option<VoiceModulationTables>,
        tempo_tables: Option<radias_synth_domain::lfo_tempo::LfoTempoTables>,
    ) -> Result<(Self, Generator), String> {
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
        // Startup queues ROM adapters and four complete timbre settings before
        // CoreAudio necessarily invokes its first callback. Keep that burst
        // intact; service still handles at most 64 commands per callback.
        let (tx, rx) = mpsc::sync_channel(256);
        let counters = Arc::new(Counters::default());
        counters.gain.store(0.2f32.to_bits(), Ordering::Relaxed);
        let generator = Generator {
            synth: crate::synthesizer::Synthesizer::new(
                plans,
                table,
                tuning,
                controller_tables,
                voice_costs,
                modulation_tables,
                tempo_tables,
            )?,
            commands: rx,
        };
        Ok((
            Self {
                commands: tx,
                counters,
                device: String::new(),
                rate: 48_000,
                modulation_available,
                tempo_available,
                portamento_available: AtomicBool::new(false),
                filter2_available: AtomicBool::new(false),
                _stream: None,
            },
            generator,
        ))
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
        self.load_program_with_drums(program, None)
    }
    pub fn load_program_with_drums(
        &self,
        program: crate::stored_program::CompiledProgram,
        drums: Option<radias_synth_application::drum_program::CompiledDrumKit>,
    ) -> Result<(), String> {
        if program.stored.drum.timbre.is_some() && drums.is_none() {
            return Err("Stored drum program requires its compiled drum kit".into());
        }
        if let Some(drums) = &drums
            && drums.program != program.stored.drum
        {
            return Err("Drum owner/program context differs".into());
        }
        if let Some(drums) = &drums {
            for instrument in &drums.instruments {
                instrument
                    .controls
                    .modulation
                    .validate_with_clock(true)
                    .map_err(|_| "Drum instrument modulation is unsupported".to_owned())?;
            }
        }
        program
            .validate_native_generators_except(drums.as_ref().and_then(|d| d.program.timbre))
            .map_err(|e| match e {
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
            .try_send(Command::Program(Box::new(NativeProgramLoad {
                compiled: program,
                drums: drums.map(Box::new),
            })))
            .map_err(|e| e.to_string())
    }
    pub fn configure_performance(
        &self,
        global: radias_synth_domain::performance::GlobalPerformance,
    ) -> Result<(), String> {
        if global.channel > 15 {
            return Err("Invalid Global MIDI channel".into());
        }
        self.commands
            .try_send(Command::Performance(global))
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
    pub fn drum_pad(&self, instrument: u8, velocity: u8) -> Result<(), String> {
        if instrument >= 16 || velocity > 127 {
            return Err("Invalid drum pad input".into());
        }
        self.commands
            .try_send(Command::DrumPad(instrument, velocity))
            .map_err(|e| e.to_string())
    }
    pub fn edit_drum_instrument(
        &self,
        index: u8,
        program: radias_synth_application::drum_program::DrumInstrumentProgram,
    ) -> Result<(), String> {
        if index >= 16 {
            return Err("Invalid drum instrument".into());
        }
        let selection = program.controls.oscillator_selection & 63;
        if selection & 15 >= 6 || (selection & 15 >= 4 && selection & 48 != 0) {
            return Err("Drum instrument generator is not yet available".into());
        }
        program
            .controls
            .modulation
            .validate_with_clock(true)
            .map_err(|_| "Drum modulation is not yet available".to_owned())?;
        self.commands
            .try_send(Command::DrumInstrument(index, Box::new(program)))
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
    pub fn configure_controller_service(
        &self,
        timer: radias_synth_domain::controller_service::ControllerServiceTimer,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::ControllerService(timer))
            .map_err(|e| e.to_string())
    }
    pub fn configure_amplifier_delivery(
        &self,
        rates: radias_synth_domain::amplifier_delivery::AmplifierRateTable,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::AmplifierDelivery(rates))
            .map_err(|e| e.to_string())
    }
    pub fn configure_constructor_filter_mix(
        &self,
        table: radias_synth_domain::filter_control::FilterMixTable,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::ConstructorFilterMix(Box::new(table)))
            .map_err(|e| e.to_string())
    }
    pub fn configure_pitch_delivery(
        &self,
        rom: [radias_synth_domain::pitch_receiver::PitchReceiverRom; 2],
        dispatch: radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable,
    ) -> Result<(), String> {
        self.commands
            .try_send(Command::PitchDelivery(Box::new(rom), dispatch))
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
                    counters.source_gains[timbre].store(
                        generator.timbres[timbre].amplifier.source_gain as u32,
                        Ordering::Relaxed,
                    );
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
                for slot in 0..24 {
                    counters.actor_pitch_codes[slot].store(
                        generator
                            .pool
                            .active_voice(slot)
                            .map_or(u32::MAX, |v| v.renderer.primary_pitch_code() as u32),
                        Ordering::Relaxed,
                    );
                }
                counters
                    .unsupported_drum_notes
                    .store(generator.unsupported_drum_notes, Ordering::Relaxed);
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
