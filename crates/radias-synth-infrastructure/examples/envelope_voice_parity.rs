//! Original controller event times are the comparison fixture. ADSR states and
//! amplifier targets are generated independently; recorded targets are asserts.
use radias_synth_application::{
    VoiceRenderer,
    lfo::LfoParameters,
    shared_lfo::{EffectLfoController, EffectLfoParameters, SharedTimbreLfo},
};
use radias_synth_domain::{
    Sample,
    amp_envelope::{AmpEnvelope, AmpEnvelopeParameters},
    amplifier_control::AmplifierControl,
    pan::StereoFrame,
};
use radias_synth_infrastructure::{
    firmware::{MasterTables, amplifier_tables, envelope_curves, envelope_timing_tables},
    prepared::PreparedVoice,
    wav,
};
use serde_json::Value;
use std::{fs, path::PathBuf};

struct ModulatedVoice {
    controller: radias_synth_application::modulation::VoiceModulation,
    events: Vec<Value>,
    index: usize,
    seed: u16,
    pitch_code: u16,
    ticks: usize,
    matrices: usize,
    pitch_commits: usize,
    tempo: Option<TempoPair>,
    clock_pulses: usize,
    initial_matrix: usize,
    shared: [Option<SharedTimbreLfo>; 4],
    global: Option<EffectLfoController>,
    tempo_tables: radias_synth_domain::lfo_tempo::LfoTempoTables,
    shared_ticks: usize,
    global_ticks: usize,
    before_order: u64,
    envelope_levels: Option<[u16; 3]>,
    amplitude_tables: radias_synth_domain::amplifier_control::AmplifierTables,
    sources: [i32; 16],
    initialization_events: bool,
    published_lfo: Option<[i16; 2]>,
    committed_pitch: u16,
    secondary_relative: Option<i16>,
    secondary_sync: bool,
    note_pitch: Option<NativePitchFixture>,
    native_pitch_compilations: usize,
    mono: Option<MonoFixture>,
    sustain: Option<SustainFixture>,
}
struct SustainFixture {
    state: radias_synth_domain::sustain::SustainState,
    program: radias_synth_domain::sustain::SustainProgram,
    poly: bool,
    claim: radias_synth_domain::voice_allocation::VoiceClaim,
    release_ready: bool,
    deferrals: usize,
    cc_inputs: usize,
    release_assertions: usize,
}
struct MonoFixture {
    mode: radias_synth_domain::mono_notes::VoiceMode,
    notes: radias_synth_domain::mono_notes::MonoNotes,
    decision: Option<radias_synth_domain::mono_notes::MonoDecision>,
    inherit_timbre: bool,
    decisions: usize,
    last_action: u8,
}
struct NativePitchFixture {
    controller: radias_synth_application::note_pitch::NotePitchController,
    scale: radias_synth_domain::note_pitch::ScaleContext,
    tables: radias_synth_domain::note_pitch::NotePitchTables,
    portamento: Option<radias_synth_application::portamento::VoicePortamento>,
    portamento_tables: radias_synth_application::portamento::PortamentoTables,
    portamento_switch: bool,
    portamento_ticks: usize,
    portamento_nonzero_steps: usize,
    portamento_notes: usize,
    portamento_rate_compilations: usize,
}
struct TempoPair {
    states: [radias_synth_domain::lfo_tempo::LfoTempoState; 2],
    tables: radias_synth_domain::lfo_tempo::LfoTempoTables,
}
fn tempo_state(
    event: &Value,
) -> Result<radias_synth_domain::lfo_tempo::LfoTempoState, Box<dyn std::error::Error>> {
    Ok(radias_synth_domain::lfo_tempo::LfoTempoState {
        phase: field(event, "tempo", 0)?,
        previous_increment: field(event, "tempo", 1)?,
        reference_phase: field(event, "tempo", 2)?,
        clock_count: field(event, "tempo", 3)? as u16,
        observed_clock_count: field(event, "tempo", 4)? as u16,
        correction_active: field(event, "tempo", 5)? as u8,
        correction_hold: field(event, "tempo", 6)? as u8,
        division: field(event, "tempo", 7)? as u8,
    })
}
fn field(event: &Value, key: &str, index: usize) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(event[key][index]
        .as_u64()
        .ok_or("Controller field absent")? as u32)
}
fn lfo_state(
    event: &Value,
) -> Result<radias_synth_domain::lfo::LfoState, Box<dyn std::error::Error>> {
    Ok(radias_synth_domain::lfo::LfoState {
        phase: field(event, "state", 0)?,
        previous_random: field(event, "state", 1)? as i16,
        random: field(event, "state", 2)? as i16,
        half_cycle: field(event, "state", 3)? as u8,
    })
}
fn shared_parameters(event: &Value) -> Result<LfoParameters, Box<dyn std::error::Error>> {
    Ok(LfoParameters {
        waveform: field(event, "parameters", 0)? as u8,
        shape: field(event, "parameters", 1)? as u8,
        frequency: field(event, "parameters", 2)? as u8,
        phase_sync: field(event, "parameters", 3)? as u8,
        frequency_offset: 0,
        frequency_modulation: 0,
    })
}
fn effect_parameters(event: &Value) -> Result<EffectLfoParameters, Box<dyn std::error::Error>> {
    Ok(EffectLfoParameters {
        mode: field(event, "parameters", 0)? as u8,
        frequency: field(event, "parameters", 1)? as u8,
        phase_sync: field(event, "parameters", 2)? as u8,
        beat: field(event, "parameters", 3)? as u8,
        alternate_phase: field(event, "parameters", 4)? as u8,
    })
}
fn assert_shared_state(
    state: radias_synth_domain::lfo::LfoState,
    tempo: radias_synth_domain::lfo_tempo::LfoTempoState,
    event: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if state != lfo_state(event)? || tempo != tempo_state(event)? {
        return Err(format!("Shared LFO state differs: {state:?}/{tempo:?} != {event}").into());
    }
    Ok(())
}
impl ModulatedVoice {
    fn load(
        path: &std::path::Path,
        tempo_tables: &radias_synth_domain::lfo_tempo::LfoTempoTables,
        amplitude_tables: &radias_synth_domain::amplifier_control::AmplifierTables,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(None);
        }
        let events: Vec<Value> = fs::read_to_string(path)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        let initial = events
            .iter()
            .find(|e| e["kind"] == "matrix")
            .ok_or("Initial LFO state absent")?;
        let initial_matrix = events
            .iter()
            .position(|event| event["kind"] == "matrix")
            .unwrap();
        let state = |i: usize| -> Result<_, Box<dyn std::error::Error>> {
            let e = &initial["lfo"][i];
            Ok(radias_synth_domain::lfo::LfoState {
                phase: field(e, "state", 0)?,
                previous_random: field(e, "state", 1)? as i16,
                random: field(e, "state", 2)? as i16,
                half_cycle: field(e, "state", 3)? as u8,
            })
        };
        let parameters = |i: usize| -> Result<_, Box<dyn std::error::Error>> {
            let e = &initial["lfo"][i];
            Ok(radias_synth_application::lfo::LfoParameters {
                waveform: field(e, "parameters", 0)? as u8,
                shape: field(e, "parameters", 1)? as u8,
                frequency: field(e, "parameters", 2)? as u8,
                phase_sync: field(e, "parameters", 3)? as u8,
                frequency_offset: field(e, "parameters", 4)? as i8,
                frequency_modulation: field(e, "parameters", 5)? as i16,
            })
        };
        let mut routes = [radias_synth_application::modulation::PatchRoute {
            source: 0,
            destination: radias_synth_domain::modulation::ModulationDestination::new(0).unwrap(),
            intensity: 64,
        }; 6];
        let mut manual_offsets = [0; 6];
        for i in 0..6 {
            let p = &initial["patches"][i];
            let value = |n: usize| -> Result<u32, Box<dyn std::error::Error>> {
                Ok(p[n].as_u64().ok_or("Patch input absent")? as u32)
            };
            routes[i] = radias_synth_application::modulation::PatchRoute {
                source: value(0)? as u8,
                destination: radias_synth_domain::modulation::ModulationDestination::new(
                    value(1)? as u8
                )
                .ok_or("Patch destination invalid")?,
                intensity: value(2)? as u8,
            };
            manual_offsets[i] = value(3)? as i8;
            let native_midi_source = [6, 7].contains(&routes[i].source)
                && path
                    .file_name()
                    .and_then(|p| p.to_str())
                    .is_some_and(|p| p.starts_with("live-note-pitch-"));
            if routes[i].intensity != 64
                && ![0, 1, 2, 3, 4].contains(&routes[i].source)
                && !native_midi_source
            {
                return Err("Audio fixture requires a source not computed here".into());
            }
            if value(4)? != 0 {
                return Err("Initial feedback depth not zero".into());
            }
        }
        let lfo_parameters = [parameters(0)?, parameters(1)?];
        let tempo_divisions =
            core::array::from_fn(|i| initial["lfo"][i]["division"].as_u64().unwrap_or(8) as u8);
        let tempo = if lfo_parameters.iter().any(|p| p.phase_sync & 128 != 0) {
            let mut states = [
                tempo_state(&initial["lfo"][0])?,
                tempo_state(&initial["lfo"][1])?,
            ];
            let setting = radias_synth_domain::lfo_tempo::TempoSetting::clamped(
                initial["tempo_setting"]
                    .as_u64()
                    .ok_or("Initial BPM absent")? as u32,
            );
            if setting.clock_rate() as u64
                != initial["clock_rate"]
                    .as_u64()
                    .ok_or("Original clock rate absent")?
            {
                return Err("Original BPM/rate store differs".into());
            }
            for i in 0..2 {
                let compiled = tempo_tables
                    .compile_increment((tempo_divisions[i] & 31) as i32, 0, setting.clock_rate())
                    .1;
                if compiled != states[i].previous_increment {
                    return Err("Initial compiled tempo rate differs".into());
                }
                states[i].previous_increment = compiled;
            }
            Some(TempoPair {
                states,
                tables: *tempo_tables,
            })
        } else {
            None
        };
        let initialization_events = events.iter().any(|e| e["kind"] == "lfo_note");
        let published_lfo = events
            .iter()
            .any(|e| e["kind"] == "lfo_publish")
            .then_some([0; 2]);
        let initial_seed = if initialization_events {
            events
                .iter()
                .find(|e| e["seed"].as_u64().is_some())
                .unwrap()
        } else {
            initial
        };
        let result = Self {
            controller: radias_synth_application::modulation::VoiceModulation {
                pair: radias_synth_application::lfo::LfoPairController {
                    states: [state(0)?, state(1)?],
                    parameters: lfo_parameters,
                },
                patches: radias_synth_application::modulation::VirtualPatchController {
                    routes,
                    manual_offsets,
                    targets: Default::default(),
                },
                base_pitch_q16: field(initial, "pitch", 0)? as i32,
                vibrato_depth: field(initial, "pitch", 1)? as i32,
                tempo_divisions,
            },
            index: 0,
            seed: initial_seed["seed"]
                .as_u64()
                .ok_or("Initial random seed absent")? as u16,
            pitch_code: 0,
            ticks: 0,
            matrices: 0,
            pitch_commits: 0,
            events,
            tempo,
            clock_pulses: 0,
            initial_matrix,
            shared: core::array::from_fn(|_| None),
            global: None,
            tempo_tables: *tempo_tables,
            shared_ticks: 0,
            global_ticks: 0,
            before_order: u64::MAX,
            envelope_levels: None,
            amplitude_tables: *amplitude_tables,
            sources: [0; 16],
            initialization_events,
            published_lfo,
            committed_pitch: 0,
            secondary_relative: None,
            secondary_sync: false,
            note_pitch: None,
            native_pitch_compilations: 0,
            mono: None,
            sustain: None,
        };
        Ok(Some(result))
    }
    fn matrix(
        &mut self,
        lfo: &radias_synth_domain::lfo::LfoTables,
        tables: &radias_synth_domain::modulation::ModulationTables,
    ) {
        if let Some(values) = self.published_lfo {
            self.controller
                .service_published(tables, self.sources, values);
        } else {
            self.controller.service(lfo, tables, self.sources);
        }
        self.pitch_code = self.pitch(lfo);
    }
    fn pitch(&self, tables: &radias_synth_domain::lfo::LfoTables) -> u16 {
        self.published_lfo.map_or_else(
            || self.controller.pitch_code(tables),
            |values| self.controller.pitch_code_published(values),
        )
    }
    fn advance(
        &mut self,
        frame: usize,
        lfo: &radias_synth_domain::lfo::LfoTables,
        tables: &radias_synth_domain::modulation::ModulationTables,
        pitch: &radias_synth_domain::pitch::PitchTable,
        bandwidth: &radias_synth_domain::bandlimit::BandwidthTable,
        renderer: &mut VoiceRenderer,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(event) = self.events.get(self.index) {
            if event["frame"].as_u64().ok_or("Modulation frame absent")? > frame as u64 {
                break;
            }
            if event["frame"].as_u64() == Some(frame as u64)
                && event["order"].as_u64().unwrap_or(0) >= self.before_order
            {
                break;
            }
            match event["kind"]
                .as_str()
                .ok_or("Modulation event kind absent")?
            {
                "sustain_cc" => {
                    let sustain = self
                        .sustain
                        .as_mut()
                        .ok_or("Native sustain context absent")?;
                    sustain.state.receive(
                        event["channel"].as_u64().ok_or("Sustain channel absent")? as u8,
                        event["event"].as_u64().ok_or("Sustain CC input absent")? as u32,
                    );
                    if sustain.state.flags & 15 == 0 {
                        if sustain.poly {
                            sustain.release_ready |=
                                radias_synth_domain::sustain::SustainState::release_pending_poly(
                                    &mut sustain.claim,
                                );
                        } else if sustain.state.release_pending_mono() {
                            sustain.release_ready |=
                                radias_synth_domain::sustain::SustainState::release_mono_claim(
                                    &mut sustain.claim,
                                );
                        }
                    }
                    sustain.cc_inputs += 1;
                }
                "sustain_note_off" => {
                    let sustain = self
                        .sustain
                        .as_mut()
                        .ok_or("Native sustain context absent")?;
                    if event["poly"].as_bool() != Some(sustain.poly) {
                        return Err("Stored sustain voice mode differs".into());
                    }
                    sustain.claim.note_flags =
                        event["note"].as_u64().ok_or("Sustain note input absent")? as u8 | 128;
                    sustain.release_ready = if sustain.poly {
                        sustain
                            .state
                            .poly_note_off(sustain.program, 0, &mut sustain.claim)
                    } else {
                        let released = sustain.state.mono_note_off(sustain.program, 0);
                        if released {
                            radias_synth_domain::sustain::SustainState::release_mono_claim(
                                &mut sustain.claim,
                            );
                        }
                        released
                    };
                    if !sustain.release_ready {
                        sustain.deferrals += 1;
                    }
                }
                "mono_event" => {
                    let mono = self.mono.as_mut().ok_or("Native Mono context absent")?;
                    let mode = radias_synth_domain::mono_notes::VoiceMode::from_raw(
                        event["mode"].as_u64().ok_or("Mono mode absent")? as u8,
                    );
                    if mode != mono.mode {
                        return Err("Stored Mono mode differs".into());
                    }
                    let input = event["event"].as_u64().ok_or("Mono MIDI input absent")? as u32;
                    let decision = if event["off"].as_bool().ok_or("Mono direction absent")? {
                        mono.notes.note_off(mono.mode, input)
                    } else {
                        mono.notes.note_on(mono.mode, input)
                    };
                    mono.inherit_timbre =
                        decision.action == radias_synth_domain::mono_notes::MonoAction::Allocate;
                    mono.decision = Some(decision);
                }
                "mono_decision" => {
                    use radias_synth_domain::mono_notes::MonoAction;
                    let mono = self.mono.as_mut().ok_or("Native Mono context absent")?;
                    let decision = mono.decision.take().ok_or("Mono event absent")?;
                    let action = match decision.action {
                        MonoAction::Ignore => 0,
                        MonoAction::Allocate => 1,
                        MonoAction::Legato => 2,
                        MonoAction::Retrigger => 3,
                        MonoAction::Release => 4,
                    };
                    if event["action"].as_u64() != Some(action)
                        || event["selected"].as_u64() != Some(decision.event as u64)
                        || event["velocity"].as_u64() != Some(mono.notes.velocity as u64)
                        || (0..6).any(|i| {
                            event["entries"][i].as_u64() != Some(mono.notes.entries[i] as u64)
                        })
                    {
                        return Err(format!("Native Mono decision/queue differs: {event}").into());
                    }
                    mono.decisions += 1;
                    mono.last_action = action as u8;
                }
                "clock_one" | "clock_four" => {
                    if self.initialization_events || self.index >= self.initial_matrix {
                        let four = event["kind"] == "clock_four";
                        if self.index >= self.initial_matrix
                            && let Some(tempo) = &mut self.tempo
                        {
                            for (i, state) in tempo.states.iter_mut().enumerate() {
                                state.phase = self.controller.pair.states[i].phase;
                                if four {
                                    state.clock_pulse_four();
                                } else {
                                    state.clock_pulse_one();
                                }
                            }
                        }
                        for shared in self.shared.iter_mut().flatten() {
                            shared.pulse(four);
                        }
                        if let Some(global) = &mut self.global {
                            global.tempo.phase = global.state.phase;
                            if four {
                                global.tempo.clock_pulse_four();
                            } else {
                                global.tempo.clock_pulse_one();
                            }
                        }
                        self.clock_pulses += 1;
                    }
                }
                "shared_tick" | "global_lfo_tick" => {
                    if self.initialization_events || self.index >= self.initial_matrix {
                        if event["seed"].as_u64() != Some(self.seed as u64) {
                            return Err(format!(
                                "Shared/global PRNG differs before frame {frame}: {} != {}",
                                self.seed, event["seed"]
                            )
                            .into());
                        }
                        if event["kind"] == "global_lfo_tick" {
                            let e = &event["lfo"][0];
                            if self.global.is_none() {
                                self.global = Some(EffectLfoController {
                                    state: lfo_state(e)?,
                                    tempo: tempo_state(e)?,
                                });
                            }
                            let global = self.global.as_mut().unwrap();
                            assert_shared_state(global.state, global.tempo, e)?;
                            global.tick(
                                effect_parameters(e)?,
                                lfo,
                                &self.tempo_tables,
                                &mut self.seed,
                            );
                            self.global_ticks += 1;
                        } else {
                            let timbre =
                                event["timbre"].as_u64().ok_or("Shared timbre absent")? as usize;
                            if timbre >= 4 {
                                return Err("Shared timbre out of range".into());
                            }
                            if self.shared[timbre].is_none() {
                                let mut shared = SharedTimbreLfo::default();
                                for i in 0..2 {
                                    shared.synthesis.states[i] = lfo_state(&event["lfo"][i])?;
                                    shared.tempo[i] = tempo_state(&event["lfo"][i])?;
                                    shared.effects[i] = EffectLfoController {
                                        state: lfo_state(&event["lfo"][i + 2])?,
                                        tempo: tempo_state(&event["lfo"][i + 2])?,
                                    };
                                }
                                self.shared[timbre] = Some(shared);
                            }
                            let shared = self.shared[timbre].as_mut().unwrap();
                            let mut divisions = [0; 2];
                            for (i, division) in divisions.iter_mut().enumerate() {
                                assert_shared_state(
                                    shared.synthesis.states[i],
                                    shared.tempo[i],
                                    &event["lfo"][i],
                                )?;
                                assert_shared_state(
                                    shared.effects[i].state,
                                    shared.effects[i].tempo,
                                    &event["lfo"][i + 2],
                                )?;
                                shared.synthesis.parameters[i] =
                                    shared_parameters(&event["lfo"][i])?;
                                *division = event["lfo"][i]["division"]
                                    .as_u64()
                                    .ok_or("Shared division absent")?
                                    as u8;
                            }
                            shared.tick(
                                event["enabled"].as_bool().ok_or("Shared enable absent")?,
                                divisions,
                                [
                                    effect_parameters(&event["lfo"][2])?,
                                    effect_parameters(&event["lfo"][3])?,
                                ],
                                lfo,
                                &self.tempo_tables,
                                &mut self.seed,
                            );
                            self.shared_ticks += 1;
                        }
                    }
                }
                "lfo_note" => {
                    let family = event["family"].as_u64().ok_or("LFO family absent")? as usize;
                    let timbre = event["timbre"].as_u64().ok_or("LFO timbre absent")? as usize;
                    let shared = self.shared[timbre]
                        .as_ref()
                        .ok_or("Shared LFO not initialized")?;
                    let sync = event["phase_sync"]
                        .as_u64()
                        .ok_or("Note phase sync absent")? as u8;
                    self.controller.pair.states[family].initialize_note(
                        sync,
                        shared.synthesis.states[family],
                        &mut self.seed,
                    );
                    if let Some(tempo) = &mut self.tempo {
                        tempo.states[family].initialize_note(
                            sync,
                            event["division"].as_u64().ok_or("Note division absent")? as u8,
                            shared.tempo[family],
                        );
                    }
                }
                "lfo_publish" => {
                    let family = event["family"]
                        .as_u64()
                        .ok_or("Published LFO family absent")?
                        as usize;
                    self.published_lfo
                        .as_mut()
                        .ok_or("LFO publication not enabled")?[family] =
                        self.controller.pair.values(lfo)[family];
                }
                "noise_note" => {
                    if event["seed"].as_u64() != Some(self.seed as u64) {
                        return Err(
                            format!("Noise constructor seed differs at frame {frame}").into()
                        );
                    }
                    radias_synth_domain::lfo::LfoState::initialize_oscillator_random(
                        &mut self.seed,
                    );
                }
                "base_note" => {
                    if let Some(pitch) = &self.note_pitch {
                        let base = pitch.controller.base(
                            radias_synth_application::note_pitch::MidiPitch {
                                bend: event["midi_bend"].as_u64().ok_or("Bend input absent")?
                                    as i16,
                                wheel: 0,
                            },
                            0,
                            false,
                        );
                        if event["note"].as_u64() != Some(pitch.controller.note.wrapped as u64)
                            || event["assigned_note"].as_u64()
                                != Some(pitch.controller.assigned_note_q16 as u32 as u64)
                            || event["scale_offset"].as_u64()
                                != Some(pitch.controller.note.scale_q16 as u32 as u64)
                            || event["tuning_offset"].as_u64()
                                != Some(pitch.controller.tuning_q16 as u32 as u64)
                            || event["manual_offset"].as_u64() != Some(0)
                        {
                            return Err("Native stored note/tuning inputs differ".into());
                        }
                        self.controller.base_pitch_q16 = base;
                        self.native_pitch_compilations += 1;
                    } else {
                        self.controller.base_pitch_q16 =
                            (event["note"].as_u64().ok_or("MIDI note absent")? as i32) << 16;
                    }
                    if event["expected_base"].as_u64()
                        != Some(self.controller.base_pitch_q16 as u32 as u64)
                    {
                        return Err("Fixture contains unported transposition or portamento".into());
                    }
                    self.pitch_code = self.pitch(lfo);
                }
                "note_init" => {
                    if let Some(pitch) = &mut self.note_pitch {
                        if !pitch.controller.initialize_note(
                            event["midi_note"].as_u64().ok_or("MIDI input absent")? as u8,
                            pitch.scale,
                            &pitch.tables,
                            &mut self.seed,
                        ) || event["expected_note"].as_u64()
                            != Some(pitch.controller.note.wrapped as u64)
                            || event["expected_clamped"].as_u64()
                                != Some(pitch.controller.note.clamped_q8 as u64)
                            || event["expected_scale"].as_u64()
                                != Some(pitch.controller.note.scale_q16 as u32 as u64)
                        {
                            return Err("Native note initializer differs".into());
                        }
                        self.native_pitch_compilations += 1;
                    }
                }
                "tuning_init" => {
                    if let Some(pitch) = &mut self.note_pitch {
                        if event["master_tune"].as_u64() != Some(0)
                            || event["tuning_virtual_patch"].as_u64() != Some(0)
                            || event["tuning_manual_offset"].as_u64() != Some(0)
                        {
                            return Err("Unqualified tuning context".into());
                        }
                        pitch.controller.compile_tuning(&pitch.tables, 0, 0, 0);
                        if event["expected_tuning"].as_u64()
                            != Some(pitch.controller.tuning_q16 as u32 as u64)
                        {
                            return Err("Native tuning initializer differs".into());
                        }
                        self.native_pitch_compilations += 1;
                    }
                }
                "note_assign" => {
                    if let Some(pitch) = &mut self.note_pitch {
                        let offset = pitch.portamento.map_or(0, |port| port.state.current_q16);
                        if event["portamento_offset"].as_u64() != Some(offset as u32 as u64) {
                            return Err("Portamento is outside this scene".into());
                        }
                        pitch.controller.assign_note(offset);
                        if event["expected_assigned"].as_u64()
                            != Some(pitch.controller.assigned_note_q16 as u32 as u64)
                        {
                            return Err("Native note assignment differs".into());
                        }
                        self.native_pitch_compilations += 1;
                    }
                }
                "port_switch" => {
                    if event["timbre"].as_u64() == Some(0)
                        && let Some(pitch) = &mut self.note_pitch
                    {
                        pitch.portamento_switch =
                            event["value"].as_u64().ok_or("Switch input absent")? & 64 != 0;
                    }
                }
                "port_rate" | "port_note" | "port_tick" | "port_state" => {
                    let pitch = self
                        .note_pitch
                        .as_mut()
                        .ok_or("Native pitch context absent")?;
                    let port = pitch
                        .portamento
                        .as_mut()
                        .ok_or("Native portamento context absent")?;
                    if event["time"].as_u64() != Some(port.program.time as u64)
                        || event["curve"].as_u64() != Some(port.program.curve as u64)
                        || event["switch_required"].as_bool() != Some(port.program.switch_required)
                        || event["switch"].as_bool() != Some(pitch.portamento_switch)
                        || event["manual_offset"].as_u64() != Some(0)
                        || event["modulation"].as_u64()
                            != Some(self.controller.patches.targets.controls[13] as u16 as u64)
                    {
                        return Err(format!("Portamento program/input mismatch: {event}").into());
                    }
                    port.modulation = self.controller.patches.targets.controls[13];
                    match event["kind"].as_str().unwrap() {
                        "port_rate" => {
                            port.compile_rate(
                                &pitch.portamento_tables.rates,
                                pitch.portamento_switch,
                            );
                            pitch.portamento_rate_compilations += 1;
                            if event["expected_state"][1].as_u64() != Some(port.state.rate as u64) {
                                return Err("Native portamento rate differs".into());
                            }
                        }
                        "port_note" => {
                            let previous = pitch.controller.assigned_note_q16;
                            if event["previous_voice"].as_u64() != Some(previous as u32 as u64) {
                                return Err("Native previous pitch differs".into());
                            }
                            // This scene declares the controller boot shared pitch60;
                            // subsequent shared state must equal the generated prior
                            // assigned voice pitch. The inheritance flag is accepted
                            // controller context; allocation/mode policy is separate.
                            let shared = if pitch.portamento_notes == 0 {
                                60 << 16
                            } else {
                                previous
                            };
                            if event["previous_timbre"].as_u64() != Some(shared as u32 as u64) {
                                return Err("Native shared prior pitch differs".into());
                            }
                            let inherit = if let Some(mono) = &self.mono {
                                if event["inherit_timbre"].as_bool() != Some(mono.inherit_timbre) {
                                    return Err("Native Mono inheritance differs".into());
                                }
                                mono.inherit_timbre
                            } else {
                                event["inherit_timbre"]
                                    .as_bool()
                                    .ok_or("Inheritance input absent")?
                            };
                            port.note_on(
                                pitch.controller.note.wrapped,
                                previous,
                                inherit.then_some(shared),
                                &pitch.portamento_tables.rates,
                                pitch.portamento_switch,
                            );
                            pitch.portamento_notes += 1;
                        }
                        "port_tick" => {
                            port.tick(&pitch.portamento_tables.curves);
                            pitch.portamento_ticks += 1;
                            if port.state.current_q16 != 0 {
                                pitch.portamento_nonzero_steps += 1;
                            }
                        }
                        "port_state" => {
                            let actual = [
                                port.state.phase,
                                port.state.rate,
                                port.state.start_q16 as u32,
                                port.state.current_q16 as u32,
                            ];
                            if actual
                                != core::array::from_fn::<_, 4, _>(|i| {
                                    event["expected_state"][i].as_u64().unwrap() as u32
                                })
                            {
                                return Err(format!(
                                    "Native portamento state differs: {actual:?} vs {event}"
                                )
                                .into());
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                "lfo_tick" => {
                    if (self.shared_ticks != 0 || self.global_ticks != 0)
                        && event["seed"].as_u64() != Some(self.seed as u64)
                    {
                        return Err(format!("Private PRNG differs at frame {frame}").into());
                    }
                    for i in 0..2 {
                        let state = self.controller.pair.states[i];
                        let e = &event["lfo"][i];
                        if [
                            state.phase,
                            state.previous_random as u16 as u32,
                            state.random as u16 as u32,
                            state.half_cycle as u32,
                        ] != [
                            field(e, "state", 0)?,
                            field(e, "state", 1)?,
                            field(e, "state", 2)?,
                            field(e, "state", 3)?,
                        ] {
                            return Err(format!("LFO state mismatch at frame {frame}").into());
                        }
                    }
                    if let Some(tempo) = &mut self.tempo {
                        for i in 0..2 {
                            if tempo.states[i] != tempo_state(&event["lfo"][i])? {
                                return Err(format!(
                                    "Tempo state mismatch at frame {frame}: {:?} != {:?}",
                                    tempo.states[i],
                                    tempo_state(&event["lfo"][i])?
                                )
                                .into());
                            }
                        }
                        self.controller.pair.tick_with_tempo(
                            lfo,
                            &tempo.tables,
                            self.controller.tempo_divisions,
                            &mut tempo.states,
                            &mut self.seed,
                        );
                    } else {
                        self.controller
                            .pair
                            .tick(lfo, &mut self.seed)
                            .map_err(|_| "Clock data absent for tempo LFO")?;
                    }
                    self.ticks += 1;
                }
                "matrix" => {
                    if event["seed"].as_u64() != Some(self.seed as u64)
                        && self.initialization_events
                    {
                        return Err(format!("Matrix seed differs at frame {frame}").into());
                    }
                    if let Some(levels) = self.envelope_levels {
                        for (i, level) in levels.iter().enumerate() {
                            if *level as u32 != field(event, "envelope_levels", i)? {
                                return Err(format!(
                                    "EG{} level differs at frame {frame}: {} != {}",
                                    i + 1,
                                    level,
                                    event["envelope_levels"][i]
                                )
                                .into());
                            }
                        }
                        let signals = radias_synth_domain::modulation::ControllerSources {
                            envelope_levels: levels,
                            envelope_velocity_sensitivity: [
                                field(event, "envelope_velocity_sensitivity", 0)? as u8,
                                field(event, "envelope_velocity_sensitivity", 1)? as u8,
                                field(event, "envelope_velocity_sensitivity", 2)? as u8,
                            ],
                            lfo: [0; 2],
                            velocity: event["velocity"].as_u64().ok_or("Matrix velocity absent")?
                                as u8,
                            bend: if self.note_pitch.is_some() {
                                event["midi_bend"].as_u64().ok_or("Bend source absent")? as i16
                            } else {
                                0
                            },
                            wheel: if self.note_pitch.is_some() {
                                event["wheel"].as_u64().ok_or("Wheel source absent")? as u8
                            } else {
                                0
                            },
                            relative_pitch: if let Some(pitch) = &self.note_pitch {
                                (pitch.controller.assigned_note_q16.wrapping_sub(60 << 16) >> 8)
                                    as i16
                            } else {
                                ((self.controller.base_pitch_q16 >> 16) - 60) as i16 * 256
                            },
                            auxiliary: 0,
                        }
                        .normalized(&self.amplitude_tables);
                        self.sources[..10].copy_from_slice(&signals);
                    }
                    let values = self
                        .published_lfo
                        .unwrap_or_else(|| self.controller.pair.values(lfo));
                    if [values[0] as u16 as u32, values[1] as u16 as u32]
                        != [
                            field(event, "lfo_values", 0)?,
                            field(event, "lfo_values", 1)?,
                        ]
                    {
                        return Err(format!("LFO waveform mismatch at frame {frame}: values {values:?}, states {:?}, seed {} != original values {:?}, states {:?}, seed {:?}",self.controller.pair.states,self.seed,event["lfo_values"],event["lfo"],event["seed"]).into());
                    }
                    self.matrix(lfo, tables);
                    self.matrices += 1;
                }
                "pitch" => {
                    if let Some(pitch) = &self.note_pitch {
                        self.controller.vibrato_depth = pitch.controller.program.vibrato_depth(
                            event["wheel"].as_u64().ok_or("Wheel input absent")? as u8,
                            &pitch.tables.vibrato,
                        );
                        if self.controller.base_pitch_q16 != field(event, "pitch", 0)? as i32
                            || self.controller.vibrato_depth != field(event, "pitch", 1)? as i32
                        {
                            return Err(
                                format!("Native base/vibrato differs at frame {frame}").into()
                            );
                        }
                        self.native_pitch_compilations += 1;
                    } else {
                        self.controller.base_pitch_q16 = field(event, "pitch", 0)? as i32;
                        self.controller.vibrato_depth = field(event, "pitch", 1)? as i32;
                    }
                    self.pitch_code = self.pitch(lfo);
                    if self.pitch_code as u32 != field(event, "pitch", 4)? {
                        return Err(
                            format!("Computed controller pitch mismatch at frame {frame}").into(),
                        );
                    }
                }
                "pitch_commit" => {
                    if event["pc"].as_u64() == Some(0xd534) {
                        self.committed_pitch = 0;
                    }
                    if event["pc"].as_u64() != Some(0xd534) {
                        if self.pitch_code as u64
                            != event["expected_code"]
                                .as_u64()
                                .ok_or("Pitch commit absent")?
                        {
                            return Err(
                                format!("Computed DSP pitch mismatch at frame {frame}").into()
                            );
                        }
                        let code = radias_synth_domain::pitch::PitchCode::new(self.pitch_code)
                            .ok_or("Pitch out of range")?;
                        let increment = pitch.increment(code);
                        renderer.set_pitch(increment, bandwidth.coefficient(increment));
                        self.committed_pitch = self.pitch_code;
                        self.pitch_commits += 1;
                    }
                }
                _ => return Err("Unknown modulation event".into()),
            }
            self.index += 1;
        }
        Ok(())
    }
}
fn parameters(event: &Value) -> Result<AmpEnvelopeParameters, Box<dyn std::error::Error>> {
    let p = event["parameters"]
        .as_array()
        .ok_or("Envelope parameters absent")?;
    let value = |i: usize| -> Result<u8, Box<dyn std::error::Error>> {
        Ok(u8::try_from(
            p.get(i)
                .and_then(Value::as_u64)
                .ok_or("Envelope parameter absent")?,
        )?)
    };
    Ok(AmpEnvelopeParameters {
        attack: value(0)?,
        decay: value(1)?,
        sustain: value(2)?,
        release: value(3)?,
        curve: value(4)?,
        velocity_sensitivity: value(5)?,
        key_tracking: value(6)?,
        velocity: value(7)?,
        note: value(8)?,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().ok_or("Repository path required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let curves = envelope_curves(&system)?;
    let timing = envelope_timing_tables(&system)?;
    let gain = amplifier_tables(&system)?;
    let lfo = radias_synth_infrastructure::firmware::lfo_tables(&system)?;
    let tempo_tables = radias_synth_infrastructure::firmware::lfo_tempo_tables(&system)?;
    let modulation_tables = radias_synth_infrastructure::firmware::modulation_tables(&system)?;
    let filter_tables = radias_synth_infrastructure::firmware::controller_filter_tables(&system)?;
    let pan_tables = radias_synth_infrastructure::firmware::pan_tables(&system)?;
    let mixer_scales = radias_synth_infrastructure::firmware::mixer_scales(&system)?;
    let fine_tune = radias_synth_infrastructure::firmware::fine_tune_table(&system)?;
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&source)?;
    let waveform = tables.waveform()?;
    let pitch = tables.pitch()?;
    let bandwidth = tables.bandwidth()?;
    let native_rates = args.iter().any(|arg| arg == "--native-rates");
    let rate_tables = radias_synth_infrastructure::firmware::amplifier_rate_table(&system)?;
    for name in args
        .iter()
        .skip(1)
        .filter(|arg| arg.as_str() != "--native-rates")
    {
        let output = root.join("runs/native-clone");
        let raw = fs::read(output.join(format!("{name}-voice-va-inputs.bin")))?;
        let mut plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let mut events: Vec<Value> =
            fs::read_to_string(output.join(format!("{name}-native-envelope-events.jsonl")))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        for event in &mut events {
            event["controller"] = "amp".into();
        }
        if native_rates {
            for line in
                fs::read_to_string(output.join(format!("{name}-amplifier-packet-events.jsonl")))?
                    .lines()
            {
                let mut event: Value = serde_json::from_str(line)?;
                event["controller"] = "amp_delivery".into();
                events.push(event);
            }
        }
        let auxiliary_path = output.join(format!("{name}-native-aux-events.jsonl"));
        let auxiliary_enabled = auxiliary_path.exists();
        let pan_path = output.join(format!("{name}-native-pan-events.jsonl"));
        let pan_enabled = pan_path.exists();
        let mixer_path = output.join(format!("{name}-native-mixer-events.jsonl"));
        let mixer_enabled = mixer_path.exists();
        let secondary_path = output.join(format!("{name}-native-secondary-events.jsonl"));
        let secondary_enabled = secondary_path.exists();
        let shaper_path = output.join(format!("{name}-native-shaper-controller-events.jsonl"));
        let shaper_enabled = shaper_path.exists();
        let primary_path = output.join(format!("{name}-native-primary-control-events.jsonl"));
        let primary_enabled = primary_path.exists()
            && matches!(
                plan.parameters.primary,
                radias_synth_domain::primary_oscillator::PrimaryParameters::Ramp(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Pulse(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Triangle(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Sine(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Cross(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::CrossTriangle(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::CrossSine(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Unison(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::UnisonCarrier(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::Vpm(_)
                    | radias_synth_domain::primary_oscillator::PrimaryParameters::VpmCarrier(_)
            );
        if primary_enabled {
            use radias_synth_domain::primary_oscillator::PrimaryParameters;
            let (selection, increment) = match plan.parameters.primary {
                PrimaryParameters::Ramp(p) => (0, p.increment),
                PrimaryParameters::Pulse(p) => (1, p.increment),
                PrimaryParameters::Triangle(p) => (2, p.increment),
                PrimaryParameters::Sine(p) => (3, p.increment),
                PrimaryParameters::Noise(_) | PrimaryParameters::Formant(_) => {
                    return Err("Noise/Formant requires its dedicated controller compiler".into());
                }
                PrimaryParameters::Cross(p) => {
                    (if p.blend == i16::MIN { 17 } else { 16 }, p.increment)
                }
                PrimaryParameters::CrossTriangle(p) => (18, p.carrier.increment),
                PrimaryParameters::CrossSine(p) => (19, p.increment),
                PrimaryParameters::Unison(p) => (32, p.increments[0]),
                PrimaryParameters::UnisonCarrier(p) => (
                    match p.waveform {
                        radias_synth_domain::unison::UnisonWaveform::Pulse { .. } => 33,
                        radias_synth_domain::unison::UnisonWaveform::Triangle => 34,
                        radias_synth_domain::unison::UnisonWaveform::Sine { .. } => 35,
                        _ => return Err("Unexpected Unison carrier".into()),
                    },
                    p.parameters.increments[0],
                ),
                PrimaryParameters::Vpm(p) => {
                    (if p.blend == i16::MIN { 49 } else { 48 }, p.increment)
                }
                PrimaryParameters::VpmCarrier(p) => (
                    match p.carrier {
                        radias_synth_domain::primary_oscillator::PrimaryVpmCarrier::Triangle(_) => {
                            50
                        }
                        radias_synth_domain::primary_oscillator::PrimaryVpmCarrier::Sine {
                            ..
                        } => 51,
                    },
                    p.modulator.increment,
                ),
            };
            plan.parameters.primary = radias_synth_application::primary::PrimaryProgram {
                selection,
                ..Default::default()
            }
            .compile_waveform(increment, bandwidth.coefficient(increment))
            .unwrap();
        }
        if auxiliary_enabled {
            for (path, kind) in [
                (auxiliary_path, "aux"),
                (
                    output.join(format!("{name}-native-filter-events.jsonl")),
                    "filter",
                ),
            ] {
                for line in fs::read_to_string(path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = kind.into();
                    events.push(event);
                }
            }
            if pan_enabled {
                for line in fs::read_to_string(pan_path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = "pan".into();
                    events.push(event);
                }
            }
            if mixer_enabled {
                for line in fs::read_to_string(mixer_path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = "mixer".into();
                    events.push(event);
                }
            }
            if secondary_enabled {
                for line in fs::read_to_string(secondary_path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = "secondary".into();
                    events.push(event);
                }
            }
            if shaper_enabled {
                for line in fs::read_to_string(shaper_path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = "shaper".into();
                    events.push(event);
                }
            }
            if primary_enabled {
                for line in fs::read_to_string(primary_path)?.lines() {
                    let mut event: Value = serde_json::from_str(line)?;
                    event["controller"] = "primary".into();
                    events.push(event);
                }
            }
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        if native_rates && !auxiliary_enabled {
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        let original = fs::read(output.join(format!("{name}-original-complete-mix.wav")))?;
        if original.len() < 44 || &original[36..40] != b"data" {
            return Err("Original bus WAV format differs".into());
        }
        let length = (original.len() - 44) / 32;
        let mut frames = vec![[Sample(0); 8]; length];
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        if auxiliary_enabled {
            renderer.control_slew(plan.control_slew, (plan.reference_start_frame & 3) as u8);
        }
        if shaper_enabled {
            renderer.set_shaper_immediate(plan.parameters.shaper);
        }
        let mut modulation = ModulatedVoice::load(
            &output.join(format!("{name}-native-modulation-events.jsonl")),
            &tempo_tables,
            &gain,
        )?;
        if let Some(controller) = &mut modulation {
            if name.starts_with("live-note-pitch-") {
                let label = name
                    .strip_prefix("live-")
                    .unwrap()
                    .strip_suffix("-reference")
                    .unwrap();
                let stored = radias_synth_domain::program::Program::from_bytes(&fs::read(
                    output.join(format!("{label}.program.bin")),
                )?)
                .map_err(|_| "Stored program missing")?;
                let controls = radias_synth_application::program::TimbreControls::from_timbre(
                    stored.timbre(0).unwrap(),
                )
                .map_err(|_| "Invalid route")?;
                let program = controls.pitch;
                if controller
                    .events
                    .iter()
                    .any(|event| event["kind"] == "sustain_cc")
                {
                    controller.sustain = Some(SustainFixture {
                        state: Default::default(),
                        program: controls.sustain,
                        poly: controls.voice_mode.polyphonic,
                        claim: radias_synth_domain::voice_allocation::VoiceClaim {
                            note_flags: 128,
                            ..Default::default()
                        },
                        release_ready: false,
                        deferrals: 0,
                        cc_inputs: 0,
                        release_assertions: 0,
                    });
                }
                if controller
                    .events
                    .iter()
                    .any(|event| event["kind"] == "mono_event")
                {
                    controller.mono = Some(MonoFixture {
                        mode: controls.voice_mode,
                        notes: Default::default(),
                        decision: None,
                        inherit_timbre: true,
                        decisions: 0,
                        last_action: 0,
                    });
                }
                controller.note_pitch = Some(NativePitchFixture {
                    controller: radias_synth_application::note_pitch::NotePitchController::new(
                        program,
                    ),
                    scale: radias_synth_domain::note_pitch::ScaleContext {
                        selection: stored.bytes()[0x13],
                        ..Default::default()
                    },
                    tables: radias_synth_infrastructure::firmware::note_pitch_tables(&system)?,
                    portamento: label.contains("portamento-").then(|| {
                        radias_synth_application::portamento::VoicePortamento::new(
                            controls.portamento,
                        )
                    }),
                    portamento_tables: radias_synth_application::portamento::PortamentoTables {
                        rates: radias_synth_infrastructure::firmware::portamento_rates(&system)?,
                        curves: radias_synth_infrastructure::firmware::portamento_curves(&system)?,
                    },
                    portamento_switch: false,
                    portamento_ticks: 0,
                    portamento_nonzero_steps: 0,
                    portamento_notes: 0,
                    portamento_rate_compilations: 0,
                });
            }
            controller.matrix(&lfo, &modulation_tables);
            if secondary_enabled {
                controller.secondary_relative = Some(0);
                controller.secondary_sync =
                    if events.iter().any(|event| event["kind"] == "secondary_sync") {
                        false
                    } else {
                        plan.parameters.secondary_modulation.sync
                    };
            }
        }
        let mut secondary_relative = 0i16;
        let mut secondary_compilations = 0usize;
        let mut secondary_commits = 0usize;
        let mut secondary_sync_commits = 0usize;
        let mut shaper_target = None;
        let mut shaper_compilations = 0usize;
        let mut shaper_commits = 0usize;
        let mut shaper_target_changes = 0usize;
        let mut prior_shaper_depth = None;
        let mut envelope = AmpEnvelope::default();
        let mut auxiliary = [radias_synth_domain::mod_envelope::ModEnvelope::default(); 2];
        let mut auxiliary_ticks = 0usize;
        let mut cutoff_compilations = 0usize;
        let mut filter_commits = 0usize;
        let mut frequency = 0u32;
        let raw_word = |i: usize| u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap());
        let mut pan_target = 0u16;
        let mut primary_target = 0i16;
        let mut primary_control_compilations = 0usize;
        let mut primary_phase_code = 0u16;
        let mut primary_phase_compilations = 0usize;
        let mut primary_ratio = 0i16;
        let mut primary_ratio_compilations = 0usize;
        let mut primary_ratio_commits = 0usize;
        let mut primary_phase_commits = 0usize;
        let mut primary_control_commits = 0usize;
        let slot_template_path = output.join(format!("{name}-initial-dsp-slot-template.bin"));
        let slot_template = if slot_template_path.exists() {
            Some(fs::read(slot_template_path)?)
        } else {
            None
        };
        let mut pan_compilations = 0usize;
        let mut pan_commits = 0usize;
        let mut mixer_gains = [0i16; 3];
        let mut committed_mixer_gains = [0i16; 3];
        let mut active_mixer_scales = [0u16, 0, 0x3333];
        let mut mixer_compilations = 0usize;
        let mut mixer_commits = 0usize;
        if mixer_enabled {
            renderer.initialize_mixer(plan.parameters.mix);
            let clock_path = output.join(format!("{name}-mixer-clock.jsonl"));
            if clock_path.exists() {
                let clocks: Vec<Value> = fs::read_to_string(clock_path)?
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()?;
                let initial = clocks
                    .iter()
                    .find(|e| e["pc"].as_u64() == Some(0xa262))
                    .ok_or("Original mixer clock absent")?;
                let first_tick = initial["frame"]
                    .as_u64()
                    .ok_or("Mixer clock sample absent")?;
                renderer
                    .mixer_slew_phase(((3 + plan.reference_start_frame - first_tick) & 3) as u8);
            }
        }
        if primary_enabled {
            let clock_path = output.join(format!("{name}-primary-control-clock.jsonl"));
            let initial_phase = if clock_path.exists() {
                let first: Value = serde_json::from_str(
                    fs::read_to_string(clock_path)?
                        .lines()
                        .next()
                        .ok_or("Primary interpolation clock absent")?,
                )?;
                (3 + plan.reference_start_frame + 4
                    - (first["frame"]
                        .as_u64()
                        .ok_or("Primary clock frame absent")?
                        & 3))
                    & 3
            } else {
                3
            };
            renderer.initialize_primary_control(raw_word(8) as i16, initial_phase as u8);
        }
        if pan_enabled {
            renderer.initialize_pan(
                radias_synth_domain::pan::PanSmoother {
                    current: raw_word(129) as i16,
                    target: 0,
                },
                radias_synth_domain::control_slew::SlewWeights {
                    target: raw_word(163) as i16,
                    memory: raw_word(164) as i16,
                },
            );
        }
        let mut filter_target = plan.parameters.filter;
        filter_target.input_gain = raw_word(1 + 54) as i16;
        filter_target.mix = core::array::from_fn(|i| raw_word(1 + 72 + 2 * i) as i16);
        let initial_resonance = ((raw_word(1 + 62) << 16) | raw_word(1 + 63)) as i32;
        let filter_normalization = events
            .iter()
            .find(|e| e["controller"] == "filter" && e["kind"] == "commit")
            .and_then(|e| e["normalization"].as_u64())
            .map(|v| v as i32);
        let mut event_index = 0;
        let mut target = 0;
        let mut state_errors = 0;
        let mut target_errors = 0;
        let mut rate_errors = 0usize;
        let mut rate_requests = 0usize;
        let mut rate_commits = 0usize;
        let mut soft_bindings = 0usize;
        let mut amplifier_delivery =
            radias_synth_domain::amplifier_delivery::AmplifierDelivery::default();
        let mut pending_rate = 0u16;
        if native_rates {
            renderer.set_envelope_rate(0);
        }
        let mut commits = 0;
        let mut ticks = 0;
        let mut current = parameters(
            events
                .iter()
                .find(|e| e["kind"] == "note_on" && e["controller"] == "amp")
                .ok_or("Original note lifecycle absent")?,
        )?;
        for (frame, sample) in frames.iter_mut().enumerate() {
            if !auxiliary_enabled && let Some(controller) = &mut modulation {
                controller.advance(
                    frame,
                    &lfo,
                    &modulation_tables,
                    &pitch,
                    &bandwidth,
                    &mut renderer,
                )?;
            }
            while let Some(event) = events.get(event_index) {
                if event["frame"].as_u64().ok_or("Event sample clock absent")? > frame as u64 {
                    break;
                }
                let kind = event["kind"].as_str().ok_or("Event kind absent")?;
                if auxiliary_enabled && let Some(controller) = &mut modulation {
                    controller.before_order = event["order"]
                        .as_u64()
                        .ok_or("Controller sequence absent")?;
                    controller.envelope_levels = Some([
                        auxiliary[0].envelope.segment.level,
                        envelope.segment.level,
                        auxiliary[1].envelope.segment.level,
                    ]);
                    controller.advance(
                        frame,
                        &lfo,
                        &modulation_tables,
                        &pitch,
                        &bandwidth,
                        &mut renderer,
                    )?;
                }
                if event["controller"] == "aux" {
                    let family =
                        event["family"].as_u64().ok_or("Auxiliary family absent")? as usize;
                    if ![0, 2].contains(&family) {
                        return Err("Unknown auxiliary envelope".into());
                    }
                    let p = parameters(event)?;
                    let envelope = &mut auxiliary[family / 2];
                    match kind {
                        "note_on" => envelope.note_on(
                            p,
                            &curves,
                            &timing,
                            event["initial_phase"]
                                .as_u64()
                                .ok_or("Auxiliary phase absent")?
                                as u32,
                        ),
                        "tick" => {
                            envelope.publish();
                            envelope.tick(p, &curves, &timing);
                            auxiliary_ticks += 1;
                        }
                        "release" => {
                            if let Some(sustain) =
                                modulation.as_mut().and_then(|m| m.sustain.as_mut())
                            {
                                if !sustain.release_ready {
                                    return Err(
                                        "Auxiliary release precedes native sustain decision".into(),
                                    );
                                }
                                sustain.release_assertions += 1;
                            }
                            envelope.release(p, &timing);
                        }
                        _ => return Err("Unknown auxiliary event".into()),
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "primary" {
                    if kind == "primary_ratio_control" {
                        let targets = modulation
                            .as_ref()
                            .ok_or("VPM controller absent")?
                            .controller
                            .patches
                            .targets;
                        if targets.controls[14] as u16 as u32 != field(event, "control", 2)? {
                            return Err(
                                format!("VPM ratio modulation differs at frame {frame}").into()
                            );
                        }
                        primary_ratio = radias_synth_domain::controller_primary::PrimaryControl {
                            control2: field(event, "control", 0)? as u8,
                            control2_manual_offset: field(event, "control", 1)? as i8,
                            control2_modulation: targets.controls[14],
                            ..Default::default()
                        }
                        .vpm_ratio();
                        if Some(primary_ratio as u16 as u64) != event["expected_ratio"].as_u64() {
                            return Err(format!("VPM ratio differs at frame {frame}").into());
                        }
                        primary_ratio_compilations += 1;
                    } else if kind == "primary_ratio_commit" {
                        let committed = if event["pc"].as_u64() == Some(0xd534) {
                            slot_template
                                .as_ref()
                                .map(|t| u32::from_le_bytes(t[32..36].try_into().unwrap()) as i16)
                                .unwrap_or(0)
                        } else {
                            primary_ratio
                        };
                        if Some(committed as u16 as u64) != event["expected_ratio"].as_u64() {
                            return Err(format!(
                                "VPM ratio commit differs at frame {frame}: {committed} vs {event}"
                            )
                            .into());
                        }
                        renderer.set_primary_ratio(committed);
                        primary_ratio_commits += 1;
                    } else if kind == "primary_phase_control" {
                        let targets = modulation
                            .as_ref()
                            .ok_or("Phase controller absent")?
                            .controller
                            .patches
                            .targets;
                        if targets.controls[14] as u16 as u32 != field(event, "control", 2)? {
                            return Err(
                                format!("Unison phase source differs at frame {frame}").into()
                            );
                        }
                        primary_phase_code =
                            radias_synth_domain::controller_primary::PrimaryControl {
                                control2: field(event, "control", 0)? as u8,
                                control2_manual_offset: field(event, "control", 1)? as i8,
                                control2_modulation: targets.controls[14],
                                ..Default::default()
                            }
                            .phase_code();
                        if Some(primary_phase_code as u64) != event["expected_code"].as_u64() {
                            return Err(
                                format!("Unison phase code differs at frame {frame}").into()
                            );
                        }
                        primary_phase_compilations += 1;
                    } else if kind == "primary_phase_commit" {
                        if Some(primary_phase_code as u64) != event["expected_code"].as_u64() {
                            return Err(format!("Unison phase HPI differs at frame {frame}").into());
                        }
                        renderer.initialize_unison_phases(
                            primary_phase_code,
                            event["triangle"].as_bool().ok_or("Phase form absent")?,
                        );
                        primary_phase_commits += 1;
                    } else if kind == "primary_control" {
                        let state = modulation
                            .as_ref()
                            .ok_or("Primary control requires generated LFO")?;
                        let targets = state.controller.patches.targets;
                        let lfo_values = state
                            .published_lfo
                            .unwrap_or_else(|| state.controller.pair.values(&lfo));
                        if targets.controls[0] as u16 as u32 != field(event, "control", 3)?
                            || targets.controls[14] as u16 as u32 != field(event, "control", 4)?
                            || lfo_values[0] as u16 as u32 != field(event, "control", 6)?
                        {
                            return Err(format!(
                                "Primary controller sources differ at frame {frame}: {}",
                                event
                            )
                            .into());
                        }
                        let control = radias_synth_domain::controller_primary::PrimaryControl {
                            control1: field(event, "control", 0)? as u8,
                            control2: field(event, "control", 1)? as u8,
                            control1_manual_offset: field(event, "control", 2)? as i16,
                            control1_modulation: targets.controls[0],
                            control2_modulation: targets.controls[14],
                            control2_manual_offset: field(event, "control", 5)? as i8,
                            lfo1: lfo_values[0],
                        }
                        .compose();
                        if [
                            control.base as u32,
                            control.curved as u32,
                            control.linear as u32,
                        ] != [
                            field(event, "expected_composed", 0)?,
                            field(event, "expected_composed", 1)?,
                            field(event, "expected_composed", 2)?,
                        ] {
                            return Err(
                                format!("Primary composed state differs at frame {frame}").into()
                            );
                        }
                        primary_target = match control.target(
                            event["selection"]
                                .as_u64()
                                .ok_or("Primary selection absent")?
                                as u8,
                        ) {
                            Some(
                                radias_synth_domain::controller_primary::PrimaryTarget::Waveform(v)
                                | radias_synth_domain::controller_primary::PrimaryTarget::Cross(v),
                            ) => v,
                            Some(
                                radias_synth_domain::controller_primary::PrimaryTarget::Unison(v)
                                | radias_synth_domain::controller_primary::PrimaryTarget::Vpm(v),
                            ) => v,
                            _ => return Err("Unqualified primary mode in moving voice".into()),
                        };
                        if primary_target as u16 as u64
                            != event["expected_target"]
                                .as_u64()
                                .ok_or("Primary target absent")?
                        {
                            return Err(format!(
                                "Primary waveform target differs at frame {frame}"
                            )
                            .into());
                        }
                        primary_control_compilations += 1;
                    } else if kind == "primary_control_commit" {
                        let committed = if event["pc"].as_u64() == Some(0xd534) {
                            let offset = if matches!(
                                plan.parameters.primary,
                                radias_synth_domain::primary_oscillator::PrimaryParameters::Sine(_)
                            ) {
                                11
                            } else {
                                6
                            };
                            if let Some(template) = &slot_template {
                                let start = offset as usize * 4;
                                u32::from_le_bytes(template[start..start + 4].try_into().unwrap())
                                    as i16
                            } else if offset == 6 {
                                0
                            } else {
                                return Err("Initial Sine slot template absent".into());
                            }
                        } else {
                            primary_target
                        };
                        if Some(committed as u16 as u64) != event["expected_target"].as_u64() {
                            return Err(format!(
                                "Primary HPI target differs at frame {frame}: {committed} != {}",
                                event
                            )
                            .into());
                        }
                        renderer.set_primary_waveform_control(committed);
                        primary_control_commits += 1;
                    } else {
                        return Err("Unknown primary control event".into());
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "pan" {
                    if kind == "pan_target" {
                        let modulated = modulation
                            .as_ref()
                            .map_or(0, |m| m.controller.patches.targets.controls[10]);
                        if modulated as u16 as u32 != field(event, "control", 2)? {
                            return Err(
                                format!("Pan virtual patch differs at frame {frame}").into()
                            );
                        }
                        let control = radias_synth_domain::controller_pan::PanControl {
                            position: field(event, "control", 0)? as u8,
                            manual_offset: field(event, "control", 1)? as i16,
                            modulation: modulated,
                            timbre_offset: field(event, "control", 3)? as i8,
                            midi_pan: (field(event, "control", 4)? != 0)
                                .then_some(field(event, "control", 5)? as u8),
                        };
                        let composed = control.target();
                        if event["expected_target"].as_u64() != Some(composed as u64) {
                            return Err(format!("Composed SH pan differs at frame {frame}").into());
                        }
                        pan_target = pan_tables.compile(composed);
                        pan_compilations += 1;
                    } else if kind == "pan_commit" {
                        let target = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            pan_target
                        };
                        if event["expected_target"].as_u64() != Some(target as u64) {
                            return Err(format!("DSP pan transfer differs at frame {frame}").into());
                        }
                        renderer.set_pan_target(target as i16);
                        pan_commits += 1;
                    } else {
                        return Err("Unknown pan event".into());
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "mixer" {
                    let family = event["family"].as_u64().ok_or("Mixer family absent")? as usize;
                    if kind == "mixer_scale" {
                        let selection = event["selection"]
                            .as_u64()
                            .ok_or("Mixer selection absent")?
                            as u8;
                        active_mixer_scales[family] = if family == 0 {
                            mixer_scales.primary(selection)
                        } else {
                            mixer_scales.secondary(selection)
                        };
                        if event["expected_scale"].as_u64()
                            != Some(active_mixer_scales[family] as u64)
                        {
                            return Err(format!("Mixer note scale differs at frame {frame}").into());
                        }
                    } else if kind == "mixer_target" {
                        let modulated = modulation
                            .as_ref()
                            .map_or(0, |m| m.controller.patches.targets.controls[1 + family]);
                        if modulated as u16 as u32 != field(event, "control", 2)? {
                            return Err(
                                format!("Mixer virtual patch differs at frame {frame}").into()
                            );
                        }
                        let scale = *active_mixer_scales
                            .get(family)
                            .ok_or("Invalid mixer family")?;
                        if event["expected_scale"].as_u64() != Some(scale as u64) {
                            return Err(format!(
                                "Mixer normalization differs at frame {frame}: {scale} != {}",
                                event["expected_scale"]
                            )
                            .into());
                        }
                        let control = radias_synth_domain::controller_mixer::MixerLevel {
                            level: field(event, "control", 0)? as u8,
                            manual_offset: field(event, "control", 1)? as i16,
                            modulation: modulated,
                            scale,
                        };
                        if event["expected_level"].as_u64() != Some(control.composed() as u64) {
                            return Err(format!(
                                "Mixer level composition differs at frame {frame}"
                            )
                            .into());
                        }
                        mixer_gains[family] = control.gain();
                        mixer_compilations += 1;
                    } else if kind == "mixer_commit" {
                        let target = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            mixer_gains[family]
                        };
                        if event["expected_target"].as_u64() != Some(target as u16 as u64) {
                            return Err(format!(
                                "Mixer DSP target differs at frame {frame}: {} != {}",
                                mixer_gains[family], event["expected_target"]
                            )
                            .into());
                        }
                        committed_mixer_gains[family] = target;
                        renderer.set_mixer(radias_synth_domain::mixer::OscillatorMix {
                            primary_gain: committed_mixer_gains[0],
                            secondary_gain: committed_mixer_gains[1],
                            noise_gain: committed_mixer_gains[2],
                        });
                        mixer_commits += 1;
                    } else {
                        return Err("Unknown mixer event".into());
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "shaper" {
                    if kind == "shaper_compile" {
                        let modulated = modulation
                            .as_ref()
                            .ok_or("Shaper matrix absent")?
                            .controller
                            .patches
                            .targets
                            .controls[8];
                        if field(event, "input", 3)? != u32::from(modulated as u16) {
                            return Err(format!(
                                "Shaper virtual patch differs at frame {frame}: {modulated} != {}",
                                event["input"][3]
                            )
                            .into());
                        }
                        let packed = field(event, "input", 0)? as u8;
                        let mode = radias_synth_application::shaper::ShaperMode::from_allocation(
                            packed & 3,
                            event["type"].as_u64().ok_or("Shaper type absent")? as u8 & 15,
                        )
                        .ok_or("Invalid shaper mode")?;
                        let program = radias_synth_application::shaper::ShaperProgram {
                            mode,
                            position: if packed & 16 != 0 {
                                radias_synth_domain::waveshaper::ShaperPosition::PreAmp
                            } else {
                                radias_synth_domain::waveshaper::ShaperPosition::PreFilter
                            },
                            control: radias_synth_domain::controller_shaper::ShaperControl {
                                depth: field(event, "input", 1)? as u8,
                                manual_offset: field(event, "input", 2)? as i16,
                                modulation: modulated,
                            },
                        };
                        shaper_target =
                            program.parameters_with_pitch(plan.parameters.primary_pitch_code);
                        shaper_compilations += 1;
                    } else if kind == "shaper_commit" {
                        let Some(mut compiled) = shaper_target else {
                            // The original template copy also writes zero
                            // DEPTH while the shaper is Off.
                            if event["pc"].as_u64() != Some(0xd534)
                                || event["expected_target"].as_u64() != Some(0)
                            {
                                return Err("Shaper commit without native compiler".into());
                            }
                            renderer.set_shaper_immediate(None);
                            shaper_commits += 1;
                            event_index += 1;
                            continue;
                        };
                        let depth = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            compiled.coefficients.depth()
                        };
                        if event["expected_target"].as_u64() != Some(u64::from(depth as u16)) {
                            return Err(format!(
                                "Shaper native DEPTH compiler differs at frame {frame}"
                            )
                            .into());
                        }
                        compiled.coefficients.set_depth(depth);
                        if prior_shaper_depth.is_some_and(|old| old != depth) {
                            shaper_target_changes += 1;
                        }
                        prior_shaper_depth = Some(depth);
                        renderer.set_shaper(Some(compiled));
                        shaper_commits += 1;
                    } else {
                        return Err("Unknown shaper controller event".into());
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "secondary" {
                    let controller = modulation.as_mut().ok_or("Secondary matrix absent")?;
                    if kind == "secondary_sync" {
                        // Actor-copy D534 copies the program's stored Sync
                        // flag too; it is not an unconditional flag reset.
                        let sync = event["selection"]
                            .as_u64()
                            .ok_or("Secondary packed selection absent")?
                            as u8
                            & 0x20
                            != 0;
                        if event["expected_sync"].as_u64() != Some(if sync { 0x7fff } else { 0 }) {
                            return Err(format!(
                                "Secondary SYNC allocation differs at frame {frame}"
                            )
                            .into());
                        }
                        controller.secondary_sync = sync;
                        renderer.set_secondary_sync(sync);
                        secondary_sync_commits += 1;
                    } else if kind == "secondary_relative" {
                        let modulated =
                            controller.controller.patches.targets.oscillator_pitch_q16[1];
                        if event["expected_modulation"].as_u64() != Some(modulated as u32 as u64) {
                            return Err(format!(
                                "Secondary virtual patch differs at frame {frame}"
                            )
                            .into());
                        }
                        let pitch = radias_synth_domain::controller_secondary::SecondaryPitch {
                            semitone: field(event, "control", 0)? as u8,
                            fine_tune: field(event, "control", 1)? as u8,
                            semitone_manual_offset: field(event, "control", 2)? as i16,
                            fine_manual_offset: field(event, "control", 3)? as i16,
                            virtual_patch_q16: modulated,
                        };
                        secondary_relative = pitch.relative_code(&fine_tune);
                        if event["expected_relative"].as_u64()
                            != Some(secondary_relative as u16 as u64)
                        {
                            return Err(format!(
                                "Secondary relative pitch differs at frame {frame}"
                            )
                            .into());
                        }
                        secondary_compilations += 1;
                    } else if kind == "secondary_commit" {
                        let relative = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            secondary_relative
                        };
                        if event["expected_relative"].as_u64() != Some(relative as u16 as u64)
                            || event["expected_primary_code"].as_u64()
                                != Some(controller.committed_pitch as u64)
                        {
                            return Err(
                                format!("Secondary HPI input differs at frame {frame}").into()
                            );
                        }
                        controller.secondary_relative = Some(relative);
                        secondary_commits += 1;
                    } else if kind == "secondary_coefficients" {
                        if event["pc"].as_u64() == Some(0xd534) {
                            if ["expected_increment", "expected_edge", "expected_bandwidth"]
                                .iter()
                                .any(|key| event[key].as_u64() != Some(0))
                            {
                                return Err("Secondary reset coefficients were not zero".into());
                            }
                            renderer.clear_secondary_tuning();
                            event_index += 1;
                            continue;
                        }
                        let relative = controller
                            .secondary_relative
                            .ok_or("Secondary HPI delta absent")?;
                        let code = radias_synth_domain::pitch::PitchCode::new(
                            (controller.committed_pitch as i32 + relative as i32).clamp(0, 32767)
                                as u16,
                        )
                        .unwrap();
                        let increment = pitch.increment(code);
                        let edge = radias_synth_domain::bandlimit::edge_coefficient(
                            code,
                            controller.secondary_sync,
                        );
                        let bw = bandwidth.coefficient(increment);
                        if event["expected_increment"].as_u64() != Some(increment.0 as u64)
                            || event["expected_edge"].as_u64() != Some(edge as u16 as u64)
                            || event["expected_bandwidth"].as_u64() != Some(bw as u16 as u64)
                        {
                            return Err(format!("Secondary coefficient compiler differs at frame {frame}: {}/{}/{} != {event}",increment.0,edge,bw).into());
                        }
                        renderer.set_secondary_pitch_mode(
                            code,
                            &pitch,
                            &bandwidth,
                            controller.secondary_sync,
                        );
                    } else {
                        return Err("Unknown secondary event".into());
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "filter" {
                    match kind {
                        "cutoff" => {
                            let p = |i| field(event, "control", i);
                            if auxiliary[0].envelope.segment.level as u32 != p(10)? {
                                return Err(
                                    format!("EG1 cutoff level mismatch at frame {frame}").into()
                                );
                            }
                            let targets = modulation
                                .as_ref()
                                .ok_or("Filter matrix absent")?
                                .controller
                                .patches
                                .targets;
                            if targets.controls[15] as u16 as u32 != p(9)?
                                || targets.controls[16] as u16 as u32 != p(5)?
                                || targets.controls[5] as u16 as u32 != p(14)?
                            {
                                return Err("Filter virtual-patch inputs differ".into());
                            }
                            let control =
                                radias_synth_domain::controller_filter::ControllerFilter {
                                    cutoff: p(0)? as u8,
                                    cutoff_offset: p(1)? as i16,
                                    lfo_offset: p(2)? as i16,
                                    key_tracking: p(3)? as u8,
                                    key_manual_offset: p(4)? as i8,
                                    key_modulation: targets.controls[16],
                                    relative_pitch: p(6)? as i16,
                                    eg1_intensity: p(7)? as u8,
                                    eg1_manual_offset: p(8)? as i8,
                                    eg1_depth_modulation: targets.controls[15],
                                    eg1_level: auxiliary[0].envelope.segment.level,
                                    velocity: p(11)? as u8,
                                    eg1_velocity_sensitivity: p(12)? as u8,
                                    additional_offset: p(13)? as i16,
                                    cutoff_modulation: targets.controls[5],
                                };
                            frequency = control.frequency(&filter_tables, &gain);
                            if Some(frequency as u64) != event["expected_frequency"].as_u64() {
                                return Err(format!(
                                    "Computed cutoff differs at frame {frame}: {frequency} != {}",
                                    event["expected_frequency"]
                                )
                                .into());
                            }
                            cutoff_compilations += 1;
                            if (frame as u64) < plan.reference_start_frame {
                                let compiled = radias_synth_domain::filter_control::compile(
                                    frequency as i32,
                                    initial_resonance,
                                    filter_normalization
                                        .ok_or("Initial filter normalization absent")?,
                                );
                                filter_target.feedback = compiled.feedback;
                                filter_target.integrator_gain = compiled.integrator_gain;
                                filter_target.post_gain = compiled.post_gain;
                                filter_target.post_feedback = compiled.post_feedback;
                                renderer.set_filter(filter_target);
                            }
                        }
                        "commit" => {
                            if event["frequency"].as_u64() != Some(frequency as u64) {
                                return Err(format!("Filter transfer frequency differs at frame {frame}: {frequency} != {}",event["frequency"]).into());
                            }
                            let compiled = radias_synth_domain::filter_control::compile(
                                frequency as i32,
                                event["resonance"].as_u64().ok_or("Resonance absent")? as i32,
                                event["normalization"]
                                    .as_u64()
                                    .ok_or("Filter normalization absent")?
                                    as i32,
                            );
                            if [
                                compiled.feedback as u32,
                                compiled.integrator_gain as u32,
                                compiled.post_gain as u16 as u32,
                                compiled.post_feedback as u16 as u32,
                            ] != [
                                field(event, "expected_coefficients", 0)?,
                                field(event, "expected_coefficients", 1)?,
                                field(event, "expected_coefficients", 2)?,
                                field(event, "expected_coefficients", 3)?,
                            ] {
                                return Err(format!("DSP filter coefficients differ at frame {frame}: {compiled:?} != {}",event["expected_coefficients"]).into());
                            }
                            filter_target.feedback = compiled.feedback;
                            filter_target.integrator_gain = compiled.integrator_gain;
                            filter_target.post_gain = compiled.post_gain;
                            filter_target.post_feedback = compiled.post_feedback;
                            renderer.set_filter(filter_target);
                            filter_commits += 1;
                        }
                        _ => return Err("Unknown filter event".into()),
                    }
                    event_index += 1;
                    continue;
                }
                if event["controller"] == "amp_delivery" {
                    match kind {
                        "packet_request" => {
                            let action =
                                event["action"].as_u64().ok_or("AMP packet action absent")?;
                            let packet = match action {
                                0 | 1 => {
                                    let soft = modulation.as_ref().and_then(|m|m.mono.as_ref()).is_some_and(|m|m.last_action == 3);
                                    if soft != (action == 1) { rate_errors += 1; }
                                    soft_bindings += usize::from(soft);
                                    amplifier_delivery.binding(&rate_tables, event["attack"].as_u64().ok_or("Attack absent")? as u8,
                                        event["attack_modulation"].as_u64().ok_or("Attack modulation absent")? as i16, target, soft)
                                }
                                2 => {
                                    if event["expected_mode_before"].as_u64() != Some(u64::from(amplifier_delivery.mode)) {rate_errors += 1;}
                                    amplifier_delivery.service(&rate_tables,target)
                                }
                                3 => radias_synth_domain::amplifier_delivery::AmplifierDelivery::reset(&rate_tables),
                                _ => return Err("Unknown AMP packet action".into()),
                            };
                            if let radias_synth_domain::amplifier_delivery::AmplifierPacket::RateAndTarget {rate,..}=packet {pending_rate=rate;}
                            rate_requests += 1;
                        }
                        "rate_commit" => {
                            let rate = if event["pc"].as_u64() == Some(0xd534) {
                                0
                            } else {
                                pending_rate
                            };
                            if event["expected_rate"].as_u64() != Some(u64::from(rate)) {
                                rate_errors += 1;
                            }
                            renderer.set_envelope_rate(rate as i16);
                            rate_commits += 1;
                        }
                        _ => return Err("Unknown AMP delivery event".into()),
                    }
                    event_index += 1;
                    continue;
                }
                if kind != "commit" {
                    current = parameters(event)?;
                }
                match kind {
                    "note_on" => envelope.note_on(
                        current,
                        &curves,
                        &timing,
                        event["initial_phase"]
                            .as_u64()
                            .ok_or("Initial phase absent")? as u32,
                    ),
                    "tick" => {
                        envelope.publish();
                        envelope.tick(
                            current,
                            &curves,
                            &timing,
                            event["acknowledged"]
                                .as_bool()
                                .ok_or("Release acknowledgement absent")?,
                        );
                        if envelope.stage
                            == radias_synth_domain::amp_envelope::EnvelopeStage::ReleaseHold
                        {
                            amplifier_delivery.release_zero();
                        }
                        ticks += 1;
                    }
                    "release" => {
                        if let Some(sustain) = modulation.as_mut().and_then(|m| m.sustain.as_mut())
                        {
                            if !sustain.release_ready {
                                return Err(
                                    "Amplifier release precedes native sustain decision".into()
                                );
                            }
                            sustain.release_assertions += 1;
                        }
                        envelope.release(current, &timing);
                    }
                    "amplifier" => {
                        let values = event["control"]
                            .as_array()
                            .ok_or("Amplifier controls absent")?;
                        let p = |i: usize| -> Result<u32, Box<dyn std::error::Error>> {
                            Ok(values
                                .get(i)
                                .and_then(Value::as_u64)
                                .ok_or("Amplifier field absent")?
                                as u32)
                        };
                        if envelope.segment.level as u32 != p(3)? {
                            if state_errors < 3 {
                                eprintln!(
                                    "{name} controller frame {frame}: {} != {}",
                                    envelope.segment.level,
                                    p(3)?
                                );
                            }
                            state_errors += 1;
                        }
                        target = gain.target(AmplifierControl {
                            level: p(0)? as u8,
                            level_offset: p(1)? as i8,
                            source_gain: p(2)? as u16,
                            envelope_level: envelope.segment.level,
                            velocity: p(4)? as u8,
                            velocity_sensitivity: p(5)? as u8,
                            modulation: [
                                p(6)? as i16,
                                modulation.as_ref().map_or(p(7)? as i16, |c| {
                                    c.controller.patches.targets.controls[9]
                                }),
                            ],
                            midi_volume: if p(8)? == 0 { None } else { Some(p(9)? as u8) },
                            program_volume: p(10)? as u8,
                        });
                    }
                    "commit" => {
                        // Initial DSP voice clearing occurs before this active
                        // voice begins; its target is already zero.
                        if frame as u64 >= plan.reference_start_frame {
                            commits += 1;
                            let committed = if event["pc"].as_u64() == Some(0xd534) {
                                0
                            } else {
                                target
                            };
                            if committed as u16 as u64
                                != event["expected_target"]
                                    .as_u64()
                                    .ok_or("Commit target absent")?
                            {
                                if target_errors < 3 {
                                    eprintln!(
                                        "{name} target frame {frame}: {target} != {}",
                                        event["expected_target"]
                                    );
                                }
                                target_errors += 1;
                            }
                            renderer.set_envelope_target(committed);
                        }
                    }
                    _ => return Err("Unrecognized envelope event".into()),
                }
                event_index += 1;
            }
            if auxiliary_enabled && let Some(controller) = &mut modulation {
                controller.before_order = u64::MAX;
                controller.envelope_levels = Some([
                    auxiliary[0].envelope.segment.level,
                    envelope.segment.level,
                    auxiliary[1].envelope.segment.level,
                ]);
                controller.advance(
                    frame,
                    &lfo,
                    &modulation_tables,
                    &pitch,
                    &bandwidth,
                    &mut renderer,
                )?;
            }
            if (frame as u64) >= plan.reference_start_frame
                && (frame as u64) < plan.reference_start_frame + plan.reference_voice_frames as u64
            {
                if frame as u64 == plan.reference_start_frame {
                    renderer.set_envelope_target(target);
                }
                let mut stereo = [StereoFrame::default(); 1];
                renderer.render(&waveform, &plan.events, &mut stereo);
                sample[0] = stereo[0].left;
                sample[1] = stereo[0].right;
            }
        }
        wav::write_buses(
            &output.join(format!(
                "{name}-rust-controller{}.wav",
                if native_rates {
                    "-amp-delivery"
                } else {
                    "-mix"
                }
            )),
            &frames,
        )?;
        let passed = state_errors == 0
            && target_errors == 0
            && rate_errors == 0
            && commits > 10
            && ticks > 10;
        let report = serde_json::json!({"name":name,"frames":length,"state_errors":state_errors,"target_errors":target_errors,
            "controller_ticks":ticks,"amplifier_commits":commits,"computed_adsr":true,"computed_amplifier_targets":true,
            "controller_event_times":"original reference observation; fixed device clock qualification remains separate","passed":passed,
            "computed_modulation":modulation.is_some(),"lfo_ticks":modulation.as_ref().map_or(0,|m|m.ticks),
            "virtual_patch_matrices":modulation.as_ref().map_or(0,|m|m.matrices),"pitch_commits":modulation.as_ref().map_or(0,|m|m.pitch_commits),
            "native_stored_pitch_compilations":modulation.as_ref().map_or(0,|m|m.native_pitch_compilations),
            "native_sustain_cc_inputs":modulation.as_ref().and_then(|m|m.sustain.as_ref()).map_or(0,|s|s.cc_inputs),
            "native_sustain_deferrals":modulation.as_ref().and_then(|m|m.sustain.as_ref()).map_or(0,|s|s.deferrals),
            "native_sustain_release_assertions":modulation.as_ref().and_then(|m|m.sustain.as_ref()).map_or(0,|s|s.release_assertions),
            "original_sustain_gate_outputs_replayed":false,
            "native_mono_decisions":modulation.as_ref().and_then(|m|m.mono.as_ref()).map_or(0,|m|m.decisions),
            "native_mono_queue_priority_and_inheritance_used":modulation.as_ref().is_some_and(|m|m.mono.is_some()),
            "native_portamento_ticks":modulation.as_ref().and_then(|m|m.note_pitch.as_ref()).map_or(0,|p|p.portamento_ticks),
            "native_portamento_nonzero_steps":modulation.as_ref().and_then(|m|m.note_pitch.as_ref()).map_or(0,|p|p.portamento_nonzero_steps),
            "native_portamento_notes":modulation.as_ref().and_then(|m|m.note_pitch.as_ref()).map_or(0,|p|p.portamento_notes),
            "native_portamento_rate_compilations":modulation.as_ref().and_then(|m|m.note_pitch.as_ref()).map_or(0,|p|p.portamento_rate_compilations),
            "original_base_pitch_and_vibrato_depth_replayed":modulation.as_ref().is_none_or(|m|m.note_pitch.is_none())});
        let mut report = report;
        report["native_AMP_rate_state_computed"] = native_rates.into();
        report["AMP_rate_errors"] = rate_errors.into();
        report["AMP_rate_requests"] = rate_requests.into();
        report["AMP_rate_commits"] = rate_commits.into();
        report["native_Mono_soft_binding_decisions"] = soft_bindings.into();
        report["original_AMP_smoothing_rates_replayed"] = (!native_rates).into();
        report["computed_tempo_lfo"] = modulation
            .as_ref()
            .is_some_and(|m| m.tempo.is_some())
            .into();
        report["original_clock_pulse_events_used"] =
            modulation.as_ref().map_or(0, |m| m.clock_pulses).into();
        report["recorded_lfo_or_modulation_targets_used_to_render"] = false.into();
        report["computed_shared_timbre_lfo_ticks"] =
            modulation.as_ref().map_or(0, |m| m.shared_ticks).into();
        report["computed_global_effect_lfo_ticks"] =
            modulation.as_ref().map_or(0, |m| m.global_ticks).into();
        report["shared_lfo_initial_snapshots"] = modulation
            .as_ref()
            .map_or(0, |m| m.shared.iter().flatten().count())
            .into();
        report["recorded_prng_seed_replayed_after_initialization"] = false.into();
        report["computed_auxiliary_envelopes"] = auxiliary_enabled.into();
        report["auxiliary_envelope_ticks"] = auxiliary_ticks.into();
        report["computed_filter_cutoffs"] = cutoff_compilations.into();
        report["computed_filter_coefficient_commits"] = filter_commits.into();
        report["computed_pan_compilations"] = pan_compilations.into();
        report["computed_pan_commits"] = pan_commits.into();
        report["computed_pan_smoothing"] = pan_enabled.into();
        report["computed_mixer_compilations"] = mixer_compilations.into();
        report["computed_mixer_commits"] = mixer_commits.into();
        report["computed_mixer_smoothing"] = mixer_enabled.into();
        report["computed_secondary_pitch_compilations"] = secondary_compilations.into();
        report["computed_secondary_commits"] = secondary_commits.into();
        report["computed_secondary_sync_commits"] = secondary_sync_commits.into();
        report["computed_shaper_modulation_from_native_envelopes"] = shaper_enabled.into();
        report["computed_shaper_compilations"] = shaper_compilations.into();
        report["computed_shaper_commits"] = shaper_commits.into();
        report["computed_shaper_target_changes"] = shaper_target_changes.into();
        report["recorded_shaper_modulation_or_depth_targets_used_to_render"] = false.into();
        report["computed_primary_control_compilations"] = primary_control_compilations.into();
        report["computed_primary_phase_compilations"] = primary_phase_compilations.into();
        report["computed_primary_phase_commits"] = primary_phase_commits.into();
        report["computed_primary_ratio_compilations"] = primary_ratio_compilations.into();
        report["computed_primary_ratio_commits"] = primary_ratio_commits.into();
        report["computed_primary_waveform_initial_coefficients"] = primary_enabled.into();
        report["computed_primary_control_commits"] = primary_control_commits.into();
        fs::write(
            output.join(format!(
                "{name}-envelope-voice{}-parity.json",
                if native_rates { "-amp-delivery" } else { "" }
            )),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Continuous native ADSR/amplifier parity failed".into());
        }
    }
    Ok(())
}
