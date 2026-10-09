#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("The device gate requires desktop-io");
}

#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::amplifier::{AmplifierProgram, ControllerTables};
    use radias_synth_application::modulation::{ModulationProgram, VoiceModulationTables};
    use radias_synth_application::voice_envelopes::{DynamicFilter, ModEnvelopeProgram};
    use radias_synth_infrastructure::{
        audio::NativePlayer,
        firmware::{
            MasterTables, amplifier_tables, envelope_curves, envelope_timing_tables,
            voice_cost_tables,
        },
        prepared::{ControlMap, PreparedVoice},
    };
    use std::{
        fs,
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let tempo_sync = std::env::args().any(|argument| argument == "--tempo");
    let noise = std::env::args().any(|argument| argument == "--noise");
    let amp_key = std::env::args().any(|argument| argument == "--amp-key");
    let amp_program = amp_key || std::env::args().any(|argument| argument == "--amp-program");
    let pitch_controls = std::env::args().any(|argument| argument == "--pitch");
    let portamento = std::env::args().any(|argument| argument == "--portamento");
    let mono = std::env::args().any(|argument| argument == "--mono");
    let sustain = std::env::args().any(|argument| argument == "--sustain");
    let note_groups = std::env::args().any(|argument| argument == "--note-groups");
    let voice_groups = std::env::args().any(|argument| argument == "--voice-groups");
    let comb = std::env::args().any(|argument| argument == "--comb");
    let filter2_controller = std::env::args().any(|argument| argument == "--filter2-controller");
    let waveshapers = std::env::args().any(|argument| argument == "--waveshapers");
    let shaper = waveshapers || std::env::args().any(|argument| argument == "--shaper");
    let dual_filter = filter2_controller
        || comb
        || shaper
        || std::env::args().any(|argument| argument == "--dual-filter");
    let auxiliary = voice_groups
        || note_groups
        || sustain
        || mono
        || portamento
        || pitch_controls
        || amp_program
        || noise
        || dual_filter
        || std::env::args().any(|argument| argument == "--auxiliary");
    let secondary =
        amp_program || noise || std::env::args().any(|argument| argument == "--secondary");
    let cross = std::env::args().any(|argument| argument == "--cross");
    let unison = std::env::args().any(|argument| argument == "--unison");
    let vpm = std::env::args().any(|argument| argument == "--vpm");
    let primary =
        cross || unison || vpm || std::env::args().any(|argument| argument == "--primary");
    let modulated = secondary
        || auxiliary
        || tempo_sync
        || std::env::args().any(|argument| argument == "--modulation");
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&master)?;
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let plans = ["saw", "pulse", "triangle", "sine"]
        .map(|name| {
            PreparedVoice::from_program_json(
                &fs::read(root.join(format!("assets/native-va/{name}.json")))
                    .map_err(|e| e.to_string())?,
            )
        })
        .into_iter()
        .collect::<Result<Vec<_>, String>>()?;
    let player = NativePlayer::with_clock(
        plans,
        tables.waveform()?,
        Some((tables.pitch()?, tables.bandwidth()?)),
        Some(ControllerTables {
            curves: envelope_curves(&sys)?,
            timing: envelope_timing_tables(&sys)?,
            amplifier: amplifier_tables(&sys)?,
        }),
        Some(voice_cost_tables(&sys)?),
        if modulated {
            Some(VoiceModulationTables {
                lfo: radias_synth_infrastructure::firmware::lfo_tables(&sys)?,
                matrix: radias_synth_infrastructure::firmware::modulation_tables(&sys)?,
                pitch: tables.pitch()?,
                bandwidth: tables.bandwidth()?,
            })
        } else {
            None
        },
        if tempo_sync {
            Some(radias_synth_infrastructure::firmware::lfo_tempo_tables(
                &sys,
            )?)
        } else {
            None
        },
    )?;
    player.gain(0.08);
    if pitch_controls || portamento || voice_groups {
        player.configure_note_pitch(
            radias_synth_infrastructure::firmware::note_pitch_tables(&sys)?,
            Default::default(),
            0,
        )?;
    }
    if voice_groups {
        player.configure_voice_groups(
            radias_synth_infrastructure::firmware::voice_group_tables(&sys)?,
        )?;
    }
    if portamento {
        player.configure_portamento(radias_synth_application::portamento::PortamentoTables {
            rates: radias_synth_infrastructure::firmware::portamento_rates(&sys)?,
            curves: radias_synth_infrastructure::firmware::portamento_curves(&sys)?,
        })?;
    }
    if noise {
        player.configure_noise(
            tables.pitch()?,
            tables.noise_pitch()?,
            radias_synth_infrastructure::firmware::formant_counter_seeds(&sys)?,
        )?;
    }
    if comb {
        player.configure_comb(radias_synth_infrastructure::firmware::comb_control_tables(
            &sys,
        )?)?;
    }
    if filter2_controller {
        player.configure_filter2(
            radias_synth_infrastructure::firmware::filter2_control_tables(&sys)?,
        )?;
    }
    if secondary {
        player.configure_mixer(radias_synth_infrastructure::firmware::mixer_scales(&sys)?)?;
        player.configure_secondary(radias_synth_infrastructure::firmware::fine_tune_table(
            &sys,
        )?)?;
    }
    let controls = if auxiliary {
        Some(ControlMap::from_system(&sys)?)
    } else {
        None
    };
    let base =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?
            .parameters
            .filter;
    if auxiliary {
        player.controller_filter_tables(
            radias_synth_infrastructure::firmware::controller_filter_tables(&sys)?,
        )?;
        player.configure_pan(
            radias_synth_infrastructure::firmware::pan_tables(&sys)?,
            radias_synth_domain::control_slew::SlewWeights {
                target: tables.word(0x4026)? as i16,
                memory: tables.word(0x4027)? as i16,
            },
        )?;
    }
    let input = player.input();
    for timbre in 0..4 {
        player.timbre(timbre, true, 0)?;
        if portamento {
            player.timbre_portamento(
                timbre,
                radias_synth_domain::portamento::PortamentoProgram {
                    time: [32, 80, 96, 127][timbre as usize],
                    curve: [0, 3, 8, 15][timbre as usize],
                    switch_required: true,
                },
            )?;
        }
        if pitch_controls {
            player.timbre_pitch(
                timbre,
                radias_synth_domain::note_pitch::PitchProgram {
                    transpose: 52 + timbre * 8,
                    fine_tune: 32 + timbre * 20,
                    vibrato_intensity: 80 + timbre * 8,
                    bend_range: 66 + timbre * 2,
                    ..Default::default()
                },
            )?;
        }
        player.timbre_waveform(timbre, timbre as usize)?;
        if amp_program {
            player.timbre_amplifier_program(
                timbre,
                radias_synth_application::amplifier::AmplifierProgram {
                    key_tracking: if amp_key { 48 + timbre * 12 } else { 64 },
                    envelope: ModEnvelopeProgram {
                        adsr: [0, 6, 100, 10],
                        curve: timbre,
                        velocity_level_sensitivity: if timbre & 1 == 0 { 48 } else { 80 },
                        velocity_time_sensitivity: if timbre & 1 == 0 { 80 } else { 48 },
                        key_tracking: 48 + timbre * 12,
                    },
                    ..Default::default()
                },
            )?;
        }
        if secondary {
            player.timbre_secondary(
                timbre,
                radias_synth_application::secondary::SecondaryProgram {
                    selection: timbre,
                    pitch: radias_synth_domain::controller_secondary::SecondaryPitch {
                        semitone: 52 + timbre * 7,
                        fine_tune: 32 + timbre * 20,
                        ..Default::default()
                    },
                },
            )?;
            player.timbre_mixer(
                timbre,
                radias_synth_application::mixer::MixerProgram {
                    selections: [timbre, timbre],
                    levels: [0, 100, 0],
                    ..Default::default()
                },
            )?;
        }
        if modulated {
            let mut program = ModulationProgram::default();
            for lfo in &mut program.lfo {
                lfo.frequency = 45 + timbre * 4;
                lfo.phase_sync = if tempo_sync { 0xc0 } else { 0x40 };
                lfo.waveform = 2;
            }
            program.tempo_divisions = [8 + timbre, 13];
            program.routes[0].source = if amp_program {
                1
            } else if auxiliary {
                if timbre % 2 == 0 { 2 } else { 0 }
            } else {
                3 + timbre % 2
            };
            program.routes[0].destination =
                radias_synth_domain::modulation::ModulationDestination::new(if timbre % 2 == 0 {
                    0
                } else {
                    if auxiliary {
                        if timbre == 3 { 12 } else { 7 }
                    } else {
                        11
                    }
                })
                .unwrap();
            program.routes[0].intensity = 80;
            if secondary {
                program.routes[1].source = 3;
                program.routes[1].destination =
                    radias_synth_domain::modulation::ModulationDestination::new(1).unwrap();
                program.routes[1].intensity = 72;
            }
            if waveshapers {
                program.routes[2].source = 3;
                program.routes[2].destination =
                    radias_synth_domain::modulation::ModulationDestination::new(10).unwrap();
                program.routes[2].intensity = 80;
            }
            if comb {
                for (index, destination) in [9, 19, 20, 21].into_iter().enumerate() {
                    program.routes[index + 1].source = 3;
                    program.routes[index + 1].destination =
                        radias_synth_domain::modulation::ModulationDestination::new(destination)
                            .unwrap();
                    program.routes[index + 1].intensity = 66 + index as u8;
                }
            }
            if noise {
                for (index, destination) in [2, 16, 5].into_iter().enumerate() {
                    program.routes[index + 1].source = 3;
                    program.routes[index + 1].destination =
                        radias_synth_domain::modulation::ModulationDestination::new(destination)
                            .unwrap();
                    program.routes[index + 1].intensity = 66 + index as u8;
                }
            }
            if portamento {
                program.routes[4].source = 3;
                program.routes[4].destination =
                    radias_synth_domain::modulation::ModulationDestination::new(15).unwrap();
                program.routes[4].intensity = 72;
            }
            player.timbre_modulation(timbre, program)?;
        }
        if let Some(controls) = &controls {
            player.timbre_pan(
                timbre,
                radias_synth_domain::controller_pan::PanControl {
                    position: 20 + timbre * 28,
                    ..Default::default()
                },
            )?;
            player.timbre_auxiliary(
                timbre,
                [ModEnvelopeProgram {
                    adsr: [8, 10, 80, 10],
                    ..Default::default()
                }; 2],
            )?;
            let filter = controls.filter(base, 70, 48)?;
            player.timbre_filter(timbre, filter)?;
            player.timbre_dynamic_filter(
                timbre,
                DynamicFilter {
                    input: radias_synth_domain::controller_filter::ControllerFilter {
                        cutoff: 70,
                        eg1_intensity: 100,
                        key_tracking: 64,
                        ..Default::default()
                    },
                    resonance: controls.resonances[48],
                    normalization: controls.normalization,
                    base: filter,
                },
            )?;
        }
    }
    if portamento {
        input.midi(&[0xb0, 65, 127])?;
    }
    for note in [60, 64, 67] {
        input.midi(&[0x90, note, 100])?;
    }
    thread::sleep(Duration::from_millis(400));
    let chord = player.status();
    if chord.active_voices != 12 || chord.held_voices != 12 {
        return Err(format!("Four-timbre chord: {chord:?}").into());
    }
    input.midi(&[0x80, 60, 0])?;
    thread::sleep(Duration::from_millis(250));
    let release = player.status();
    if release.active_voices != 8 || release.held_voices != 8 {
        return Err(format!("Independent note release: {release:?}").into());
    }
    player.stop()?;
    thread::sleep(Duration::from_millis(40));
    for note in 48..54 {
        input.midi(&[0x90, note, 80])?;
    }
    thread::sleep(Duration::from_millis(600));
    let full = player.status();
    if full.active_voices != 24 || full.held_voices != 24 {
        return Err(format!("24-voice pressure: {full:?}").into());
    }
    if tempo_sync {
        player.tempo(600)?;
    }
    input.midi(&[0x90, 54, 80])?;
    thread::sleep(Duration::from_millis(600));
    let steal = player.status();
    if steal.active_voices != 24 || steal.held_voices != 24 {
        return Err(format!("Voice replacement: {steal:?}").into());
    }
    if tempo_sync {
        player.tempo(2400)?;
    }
    let mut live_level_silence = false;
    let mut live_level_restore = false;
    let mut live_mixer_quiet = false;
    let mut live_mixer_restore = false;
    let mut muted_mixer_peak = 0.0;
    let mut live_primary_forms = 0;
    let mut live_dual_filter_combinations = 0;
    let mut live_comb_combinations = 0;
    let mut live_shaper_combinations = 0;
    let mut live_noise_combinations = 0;
    let mut live_amplifier_program_combinations = 0;
    let mut mixer_noise_peak = 0.0;
    let mut live_midi_pitch_combinations = 0;
    let mut live_portamento_combinations = 0;
    if portamento {
        for curve in 0..16 {
            for timbre in 0..4 {
                player.timbre_portamento(
                    timbre,
                    radias_synth_domain::portamento::PortamentoProgram {
                        time: 24 + (curve * 6).min(100),
                        curve,
                        switch_required: true,
                    },
                )?;
            }
            for value in [0, 127] {
                input.midi(&[0xb0, 65, value])?;
                thread::sleep(Duration::from_millis(80));
                let edited = player.status();
                if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                    return Err(format!("Portamento{curve}/CC65{value}: {edited:?}").into());
                }
                live_portamento_combinations += 1;
            }
        }
    }
    if pitch_controls {
        for bend in [0u16, 8192, 16320, 16383] {
            input.midi(&[0xe0, (bend & 127) as u8, (bend >> 7) as u8])?;
            for wheel in [0, 32, 127] {
                input.midi(&[0xb0, 1, wheel])?;
                thread::sleep(Duration::from_millis(80));
                let edited = player.status();
                if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                    return Err(format!("Live bend{bend}/wheel{wheel}: {edited:?}").into());
                }
                live_midi_pitch_combinations += 1;
            }
        }
        input.midi(&[0xe0, 0, 64])?;
        input.midi(&[0xb0, 1, 0])?;
    }
    if amp_program {
        for (level_sensitivity, time_sensitivity) in [(32, 96), (96, 32), (0, 127), (127, 0)] {
            for curve in 0..=4 {
                for timbre in 0..4 {
                    player.timbre_amplifier_program(
                        timbre,
                        radias_synth_application::amplifier::AmplifierProgram {
                            key_tracking: if amp_key {
                                32 + timbre * 20 + curve * 2
                            } else {
                                64
                            },
                            envelope: ModEnvelopeProgram {
                                adsr: [0, 6, 80 + curve * 8, 10],
                                curve,
                                velocity_level_sensitivity: level_sensitivity,
                                velocity_time_sensitivity: time_sensitivity,
                                key_tracking: 40 + timbre * 16,
                            },
                            level: 80 + timbre * 10,
                            ..Default::default()
                        },
                    )?;
                }
                thread::sleep(Duration::from_millis(100));
                let edited = player.status();
                if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                    return Err(format!(
                        "Full EG2{curve}/{level_sensitivity}/{time_sensitivity}:{edited:?}"
                    )
                    .into());
                }
                live_amplifier_program_combinations += 1;
            }
        }
    }
    if noise {
        for selection in [4, 5] {
            for (control1, control2) in [(0, 0), (20, 40), (80, 100), (127, 127)] {
                for timbre in 0..4 {
                    player.timbre_primary_control(
                        timbre,
                        radias_synth_application::primary::PrimaryProgram {
                            selection,
                            control: radias_synth_domain::controller_primary::PrimaryControl {
                                control1,
                                control2,
                                ..Default::default()
                            },
                        },
                    )?;
                    player.timbre_mixer(
                        timbre,
                        radias_synth_application::mixer::MixerProgram {
                            selections: [selection, timbre],
                            levels: [100, 0, 0],
                            ..Default::default()
                        },
                    )?;
                }
                thread::sleep(Duration::from_millis(140));
                let edited = player.status();
                if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                    return Err(format!(
                        "Noise/Formant{selection}/{control1}/{control2}: {edited:?}"
                    )
                    .into());
                }
                live_noise_combinations += 1;
            }
        }
        for timbre in 0..4 {
            player.timbre_mixer(
                timbre,
                radias_synth_application::mixer::MixerProgram {
                    selections: [5, timbre],
                    levels: [0, 0, 100],
                    ..Default::default()
                },
            )?;
        }
        thread::sleep(Duration::from_millis(200));
        mixer_noise_peak = player.status().output_peak;
        if mixer_noise_peak < 1e-8 {
            return Err("Native mixer-only noise is silent".into());
        }
    }
    if secondary {
        for timbre in 0..4 {
            player.timbre_mixer(
                timbre,
                radias_synth_application::mixer::MixerProgram {
                    selections: [timbre, timbre],
                    levels: [0; 3],
                    ..Default::default()
                },
            )?;
        }
        thread::sleep(Duration::from_millis(200));
        let muted = player.status();
        thread::sleep(Duration::from_millis(100));
        let muted_after = player.status();
        // Fixed-point feedback retains a rounding floor after the oscillator
        // inputs reach zero. Test the output level, not exact sample zero.
        muted_mixer_peak = muted.output_peak.max(muted_after.output_peak);
        live_mixer_quiet = muted_mixer_peak < 1e-6 && muted_after.held_voices == 24;
        for timbre in 0..4 {
            player.timbre_secondary(
                timbre,
                radias_synth_application::secondary::SecondaryProgram {
                    selection: timbre | ((timbre + 1) & 3) << 4,
                    pitch: radias_synth_domain::controller_secondary::SecondaryPitch {
                        semitone: 71 - timbre * 2,
                        fine_tune: 110 - timbre * 15,
                        ..Default::default()
                    },
                },
            )?;
            player.timbre_mixer(
                timbre,
                radias_synth_application::mixer::MixerProgram {
                    selections: [timbre, timbre | ((timbre + 1) & 3) << 4],
                    levels: [80, 100, 0],
                    ..Default::default()
                },
            )?;
        }
        thread::sleep(Duration::from_millis(200));
        live_mixer_restore = player.status().output_peak > 1e-4;
        if !live_mixer_quiet || !live_mixer_restore {
            return Err(format!("Live mixer gate: quiet={live_mixer_quiet}, restore={live_mixer_restore}, before={muted:?}, after={muted_after:?}, restored={:?}", player.status()).into());
        }
        for form in 0..4 {
            for timbre in 0..4 {
                player.timbre_waveform(timbre, form)?;
            }
            thread::sleep(Duration::from_millis(60));
            let edited = player.status();
            if edited.held_voices != 24 || edited.output_peak < 1e-4 {
                return Err(format!("Held primary waveform edit {form}: {edited:?}").into());
            }
            live_primary_forms += 1;
        }
        if primary {
            for mode in if vpm {
                &[0u8, 16, 32, 48][..]
            } else if unison {
                &[0u8, 16, 32][..]
            } else if cross {
                &[0u8, 16][..]
            } else {
                &[0u8][..]
            } {
                for form in 0..4 {
                    for (control1, control2) in [(20, 0), (100, 100)] {
                        for timbre in 0..4 {
                            player.timbre_waveform(timbre, form)?;
                            player.timbre_primary_control(
                                timbre,
                                radias_synth_application::primary::PrimaryProgram {
                                    selection: form as u8 | mode,
                                    control:
                                        radias_synth_domain::controller_primary::PrimaryControl {
                                            control1,
                                            control2,
                                            ..Default::default()
                                        },
                                },
                            )?;
                        }
                        thread::sleep(Duration::from_millis(100));
                        let edited = player.status();
                        if edited.held_voices != 24 || edited.output_peak < 1e-4 {
                            return Err(format!(
                                "Held primary control edit {form}/{control1}/{control2}: {edited:?}"
                            )
                            .into());
                        }
                    }
                }
            }
        }
    }
    if dual_filter {
        use radias_synth_domain::filter_routing::{Filter2Coefficients, Filter2Output};
        for route in 1..=3 {
            for output in [
                Filter2Output::LowPass,
                Filter2Output::HighPass,
                Filter2Output::BandPass,
            ] {
                for (cutoff, resonance) in [(40, 20), (96, 80)] {
                    let c = controls
                        .as_ref()
                        .ok_or("Filter controls absent")?
                        .filter(base, cutoff, resonance)?;
                    for timbre in 0..4 {
                        player.timbre_filter_routing(
                            timbre,
                            route,
                            Filter2Coefficients {
                                input_gain: c.input_gain,
                                feedback: c.feedback,
                                integrator_gain: c.integrator_gain,
                                output,
                            },
                        )?;
                        if filter2_controller {
                            let output_code = match output {
                                Filter2Output::LowPass => 0,
                                Filter2Output::HighPass => 16,
                                Filter2Output::BandPass => 32,
                                Filter2Output::Comb => unreachable!(),
                            };
                            let defaults = radias_synth_application::comb::CombProgram::default();
                            let linked = cutoff == 96;
                            player.timbre_filter2_program(timbre, radias_synth_application::filter2::Filter2Program {
                                route: route | output_code | if linked {128} else {0},
                                controls: radias_synth_application::comb::CombProgram {
                                    cutoff: radias_synth_domain::controller_comb::CombCutoffControl {
                                        cutoff, linked_cutoff: cutoff-10, link: linked,
                                        eg1_intensity:72,linked_eg1_intensity:75,
                                        ..defaults.cutoff
                                    },
                                    resonance: radias_synth_domain::controller_comb::CombResonanceControl {
                                        resonance,linked_resonance:resonance-10,link:linked,
                                        ..defaults.resonance
                                    },
                                    key_tracking:77,linked_key_tracking:72,
                                    ..defaults
                                },
                                normalization: controls.as_ref().unwrap().normalization,
                            })?;
                        }
                    }
                    thread::sleep(Duration::from_millis(120));
                    let edited = player.status();
                    if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                        return Err(format!(
                            "Dual route{route}/{output:?}/{cutoff}/{resonance}: {edited:?}"
                        )
                        .into());
                    }
                    live_dual_filter_combinations += 1;
                }
            }
        }
        if comb {
            for route in 1..=3 {
                for (cutoff, resonance, link) in [
                    (40, 0, false),
                    (120, 100, false),
                    (88, 48, true),
                    (50, 90, true),
                ] {
                    for timbre in 0..4 {
                        player.timbre_comb(
                            timbre,
                            route,
                            radias_synth_application::comb::CombProgram {
                                cutoff: radias_synth_domain::controller_comb::CombCutoffControl {
                                    cutoff,
                                    linked_cutoff: cutoff,
                                    link,
                                    eg1_intensity: 70,
                                    linked_eg1_intensity: 75,
                                    ..Default::default()
                                },
                                resonance:
                                    radias_synth_domain::controller_comb::CombResonanceControl {
                                        resonance,
                                        linked_resonance: resonance,
                                        link,
                                        ..Default::default()
                                    },
                                key_tracking: 80,
                                linked_key_tracking: 72,
                                ..Default::default()
                            },
                        )?;
                    }
                    thread::sleep(Duration::from_millis(160));
                    let state = player.status();
                    if state.held_voices != 24 || state.failed || state.output_peak < 1e-8 {
                        return Err(format!("Comb route{route}/cutoff{cutoff}/resonance{resonance}/link{link}: {state:?}").into());
                    }
                    live_comb_combinations += 1;
                }
            }
        }
        let c = controls.as_ref().unwrap().filter(base, 127, 0)?;
        for timbre in 0..4 {
            player.timbre_filter_routing(
                timbre,
                0,
                Filter2Coefficients {
                    input_gain: c.input_gain,
                    feedback: c.feedback,
                    integrator_gain: c.integrator_gain,
                    output: Filter2Output::LowPass,
                },
            )?;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if shaper {
        use radias_synth_domain::filter_routing::{Filter2Coefficients, Filter2Output};
        let c = controls.as_ref().unwrap().filter(base, 90, 48)?;
        for route in 0..=3 {
            for timbre in 0..4 {
                player.timbre_filter_routing(
                    timbre,
                    route,
                    Filter2Coefficients {
                        input_gain: c.input_gain,
                        feedback: c.feedback,
                        integrator_gain: c.integrator_gain,
                        output: Filter2Output::LowPass,
                    },
                )?;
            }
            for mode in 1..=if waveshapers { 12 } else { 2 } {
                for position in 0..=1 {
                    for depth in [0, 64, 127] {
                        for timbre in 0..4 {
                            player.timbre_shaper(timbre, mode, position, depth)?;
                        }
                        thread::sleep(Duration::from_millis(80));
                        let edited = player.status();
                        if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
                            return Err(format!("Shaper route{route}/mode{mode}/position{position}/depth{depth}:{edited:?}").into());
                        }
                        live_shaper_combinations += 1;
                    }
                }
            }
        }
        for timbre in 0..4 {
            player.timbre_filter_routing(
                timbre,
                0,
                Filter2Coefficients {
                    input_gain: c.input_gain,
                    feedback: c.feedback,
                    integrator_gain: c.integrator_gain,
                    output: Filter2Output::LowPass,
                },
            )?;
            player.timbre_shaper(timbre, 1, 1, 40)?;
        }
    }
    if auxiliary {
        for timbre in 0..4 {
            player.timbre_amplifier_level(timbre, 0)?;
        }
        thread::sleep(Duration::from_millis(200));
        let muted = player.status();
        thread::sleep(Duration::from_millis(100));
        let muted_after = player.status();
        live_level_silence =
            muted.audible_frames == muted_after.audible_frames && muted_after.held_voices == 24;
        for timbre in 0..4 {
            player.timbre_amplifier_level(timbre, 100)?;
        }
        thread::sleep(Duration::from_millis(100));
        live_level_restore = player.status().audible_frames > muted_after.audible_frames;
        if !live_level_silence || !live_level_restore {
            return Err("Live AMP level did not mute/restore held native voices".into());
        }
    }
    for note in 48..55 {
        input.midi(&[0x80, note, 0])?;
    }
    thread::sleep(Duration::from_millis(400));
    let final_state = player.status();
    for timbre in 0..4 {
        player.timbre(timbre, true, timbre / 2)?;
    }
    input.midi(&[0x90, 60, 100])?;
    input.midi(&[0x91, 60, 100])?;
    thread::sleep(Duration::from_millis(200));
    let channels = player.status();
    input.midi(&[0xb0, 123, 0])?;
    thread::sleep(Duration::from_millis(250));
    let channel_release_started = Instant::now();
    // The stored EG2 timing/velocity/key settings change the release duration.
    // Wait for the expected released voices; retain the exact count/held gate.
    if amp_program {
        while player.status().active_voices != 2
            && channel_release_started.elapsed() < Duration::from_secs(3)
        {
            thread::sleep(Duration::from_millis(20));
        }
    }
    let all_notes_off = player.status();
    input.midi(&[0xb1, 120, 0])?;
    thread::sleep(Duration::from_millis(40));
    let all_sound_off = player.status();
    let mut mono_scenes = Vec::new();
    if mono {
        use radias_synth_domain::mono_notes::{NotePriority, VoiceMode};
        for multi_trigger in [false, true] {
            for priority in [
                NotePriority::Last,
                NotePriority::Lowest,
                NotePriority::Highest,
            ] {
                player.stop()?;
                thread::sleep(Duration::from_millis(40));
                for timbre in 0..4 {
                    player.timbre(timbre, true, timbre)?;
                    player.timbre_voice_mode(
                        timbre,
                        VoiceMode {
                            polyphonic: false,
                            multi_trigger,
                            priority,
                        },
                    )?;
                    player.timbre_amplifier_program(timbre, AmplifierProgram::default())?;
                }
                thread::sleep(Duration::from_millis(40));
                for timbre in 0..4 {
                    input.midi(&[0x90 | timbre, 48 + timbre, 63])?;
                }
                thread::sleep(Duration::from_millis(160));
                for timbre in 0..4 {
                    input.midi(&[0x90 | timbre, 60 + timbre, 100])?;
                }
                thread::sleep(Duration::from_millis(160));
                let note_on = player.status();
                let actual = player.held_notes();
                let expected = core::array::from_fn(|t| {
                    Some(
                        (if priority == NotePriority::Lowest {
                            48
                        } else {
                            60
                        }) + t as u8,
                    )
                });
                if actual != expected
                    || note_on.active_voices != 4
                    || note_on.held_voices != 4
                    || note_on.output_peak < 1e-8
                {
                    return Err(format!(
                        "Live Mono selection {priority:?}/{multi_trigger}: {actual:?}/{note_on:?}"
                    )
                    .into());
                }
                for timbre in 0..4 {
                    input.midi(&[0x80 | timbre, 60 + timbre, 0])?;
                }
                thread::sleep(Duration::from_millis(160));
                let returned = player.held_notes();
                if returned != core::array::from_fn(|t| Some(48 + t as u8))
                    || player.status().held_voices != 4
                {
                    return Err("Live Mono failed to return to held note".into());
                }
                for timbre in 0..4 {
                    input.midi(&[0x80 | timbre, 48 + timbre, 0])?;
                }
                thread::sleep(Duration::from_millis(400));
                if player.status().held_voices != 0 || player.held_notes() != [None; 4] {
                    return Err("Live Mono final note release failed".into());
                }
                mono_scenes.push(serde_json::json!({"priority":format!("{priority:?}"),"multi_trigger":multi_trigger,"notes":actual,"returned":returned,"four_render_voices":true,"output_peak":note_on.output_peak}));
            }
        }
        player.stop()?;
        thread::sleep(Duration::from_millis(40));
    }
    let mut sustain_scenes = Vec::new();
    if sustain {
        for mode_raw in [128u8, 0, 64] {
            for enabled in [true, false] {
                player.stop()?;
                thread::sleep(Duration::from_millis(40));
                for timbre in 0..4 {
                    player.timbre(timbre, true, 0)?;
                    player.timbre_voice_mode(
                        timbre,
                        radias_synth_domain::mono_notes::VoiceMode::from_raw(mode_raw),
                    )?;
                    player.timbre_sustain_program(
                        timbre,
                        radias_synth_domain::sustain::SustainProgram { enabled },
                    )?;
                    player.timbre_amplifier_program(timbre, AmplifierProgram::default())?;
                }
                thread::sleep(Duration::from_millis(40));
                input.midi(&[0xb0, 64, 64])?;
                input.midi(&[0x90, 60, 100])?;
                thread::sleep(Duration::from_millis(160));
                input.midi(&[0x80, 60, 0])?;
                thread::sleep(Duration::from_millis(400));
                let held = player.status();
                let flags = player.sustain_flags();
                if enabled {
                    let held_count = if mode_raw & 128 != 0 { 0 } else { 4 };
                    if held.active_voices != 4
                        || held.held_voices != held_count
                        || held.output_peak < 1e-8
                    {
                        return Err(
                            format!("Sustain mode{mode_raw}/enabled{enabled}: {held:?}").into()
                        );
                    }
                    let expected = if mode_raw & 128 != 0 { 2 } else { 130 };
                    if flags != [expected; 4] {
                        return Err(format!("Sustain flags differ {flags:?}").into());
                    }
                } else if held.active_voices != 0 {
                    return Err("Disabled damper still held a note".into());
                }
                input.midi(&[0xb0, 64, 63])?;
                thread::sleep(Duration::from_millis(400));
                if player.status().active_voices != 0 || player.sustain_flags() != [0; 4] {
                    return Err("CC64 value63 failed to release damper".into());
                }
                sustain_scenes.push(serde_json::json!({"mode":mode_raw,"enabled":enabled,"active_during_pedal":held.active_voices,"held_flags_during_pedal":held.held_voices,"sustain_flags":flags,"pedal_output_peak":held.output_peak,"release_at_value63":true}));
            }
        }
        player.stop()?;
        thread::sleep(Duration::from_millis(40));
    }
    let mut note_group_scenes = Vec::new();
    if note_groups {
        for channel in [0u8, 1] {
            for pedal in [false, true] {
                player.stop()?;
                thread::sleep(Duration::from_millis(40));
                for timbre in 0..4 {
                    player.timbre(timbre, true, channel)?;
                    player.timbre_voice_mode(
                        timbre,
                        radias_synth_domain::mono_notes::VoiceMode::from_raw(128),
                    )?;
                    player.timbre_sustain_program(timbre, Default::default())?;
                    player.timbre_amplifier_program(timbre, AmplifierProgram::default())?;
                }
                thread::sleep(Duration::from_millis(40));
                input.midi(&[0xb0 | channel, 64, if pedal { 127 } else { 0 }])?;
                input.midi(&[0x90 | channel, 60, 63])?;
                thread::sleep(Duration::from_millis(80));
                input.midi(&[0x90 | channel, 60, 100])?;
                thread::sleep(Duration::from_millis(160));
                let both = player.status();
                if both.active_voices != 8 || both.held_voices != 8 || both.output_peak < 1e-8 {
                    return Err(format!("Repeated C4 channel{channel}: {both:?}").into());
                }
                input.midi(&[0x80 | channel, 60, 0])?;
                thread::sleep(Duration::from_millis(400));
                let first = player.status();
                let expected_active = if pedal { 8 } else { 4 };
                if first.active_voices != expected_active
                    || first.held_voices != 4
                    || first.output_peak < 1e-8
                {
                    return Err(format!(
                        "First group release channel{channel}/pedal{pedal}: {first:?}"
                    )
                    .into());
                }
                input.midi(&[0x80 | channel, 60, 0])?;
                thread::sleep(Duration::from_millis(400));
                let second = player.status();
                if second.held_voices != 0 || second.active_voices != if pedal { 8 } else { 0 } {
                    return Err(format!(
                        "Second group release channel{channel}/pedal{pedal}: {second:?}"
                    )
                    .into());
                }
                input.midi(&[0xb0 | channel, 64, 0])?;
                thread::sleep(Duration::from_millis(400));
                if player.status().active_voices != 0 {
                    return Err("Repeated note groups remained after pedal release".into());
                }
                input.midi(&[0x90 | channel, 60, 63])?;
                input.midi(&[0x90 | channel, 60, 100])?;
                thread::sleep(Duration::from_millis(160));
                input.midi(&[0xb0 | channel, 123, 0])?;
                thread::sleep(Duration::from_millis(400));
                if player.status().active_voices != 0 || player.status().held_voices != 0 {
                    return Err("All Notes Off missed repeated note groups".into());
                }
                note_group_scenes.push(serde_json::json!({"channel":channel,"pedal":pedal,"both_active":both.active_voices,
                    "after_first_active":first.active_voices,"after_first_held":first.held_voices,"after_second_active":second.active_voices,
                    "after_second_held":second.held_voices,"all_notes_off_clears_repeated_groups":true,"output_peak":first.output_peak}));
            }
        }
        player.stop()?;
        thread::sleep(Duration::from_millis(40));
    }
    let mut voice_group_scenes = Vec::new();
    let mut voice_group_pressure_scenes = Vec::new();
    let mut voice_group_edit_scenes = Vec::new();
    if voice_groups {
        for mode in [128u8, 0, 64] {
            for count in [2u8, 4, 8] {
                player.stop()?;
                thread::sleep(Duration::from_millis(40));
                for timbre in 0..4 {
                    player.timbre(timbre, timbre == 0, 0)?;
                }
                player.timbre_voice_mode(
                    0,
                    radias_synth_domain::mono_notes::VoiceMode::from_raw(mode),
                )?;
                player.timbre_amplifier_program(0, AmplifierProgram::default())?;
                player.timbre_pan(0, Default::default())?;
                let group = radias_synth_domain::voice_group::VoiceGroupProgram {
                    raw: 128 | (count - 2),
                    detune: 64,
                    spread: 120,
                };
                player.timbre_voice_group(0, group)?;
                input.midi(&[0x90, 60, 100])?;
                thread::sleep(Duration::from_millis(200));
                let first = player.status();
                if first.active_voices != count as u32
                    || first.held_voices != count as u32
                    || first.output_peak < 1e-8
                {
                    return Err(format!("Unison{count}/mode{mode}: {first:?}").into());
                }
                input.midi(&[0x90, 72, 100])?;
                thread::sleep(Duration::from_millis(200));
                let overlap = player.status();
                let expected = if mode == 128 {
                    count as u32 * 2
                } else {
                    count as u32
                };
                if overlap.held_voices != expected || overlap.active_voices != expected {
                    return Err(format!("Unison overlap{count}/mode{mode}: {overlap:?}").into());
                }
                for amount in [0u8, 127] {
                    player.timbre_voice_group(
                        0,
                        radias_synth_domain::voice_group::VoiceGroupProgram {
                            detune: amount,
                            spread: amount,
                            ..group
                        },
                    )?;
                    thread::sleep(Duration::from_millis(100));
                    if player.status().held_voices != expected || player.status().output_peak < 1e-8
                    {
                        return Err("Live Unison detune/spread lost actors or audio".into());
                    }
                }
                input.midi(&[0x80, 72, 0])?;
                thread::sleep(Duration::from_millis(400));
                let returned = player.status();
                if returned.held_voices != count as u32 || returned.active_voices != count as u32 {
                    return Err(format!("Unison return{count}/mode{mode}: {returned:?}").into());
                }
                input.midi(&[0x80, 60, 0])?;
                thread::sleep(Duration::from_millis(400));
                if player.status().active_voices != 0 {
                    return Err("Unison final release left actors".into());
                }
                voice_group_scenes.push(serde_json::json!({"mode":mode,"count":count,"first_active":first.active_voices,"overlap_active":overlap.active_voices,"return_active":returned.active_voices,"output_peak":first.output_peak,"live_detune_spread_preserves_actor_count":true}));
            }
        }
        for count in [5u8, 7] {
            player.stop()?;
            thread::sleep(Duration::from_millis(40));
            player
                .timbre_voice_mode(0, radias_synth_domain::mono_notes::VoiceMode::from_raw(128))?;
            player.timbre_voice_group(
                0,
                radias_synth_domain::voice_group::VoiceGroupProgram {
                    raw: 128 | (count - 2),
                    detune: 64,
                    spread: 90,
                },
            )?;
            let mut peaks = Vec::new();
            for note in 48..55 {
                input.midi(&[0x90, note, 100])?;
                thread::sleep(Duration::from_millis(100));
                let state = player.status();
                let expected = ((note - 47) as u32 * count as u32).min(24);
                if state.active_voices != expected
                    || state.held_voices != expected
                    || state.output_peak < 1e-8
                {
                    return Err(format!("Unison pressure{count} note{note}: {state:?}").into());
                }
                peaks.push(state.output_peak);
            }
            let spread_edit = radias_synth_domain::voice_group::VoiceGroupProgram {
                raw: 128 | (count - 2),
                detune: 64,
                spread: 127,
            };
            player.timbre_voice_group(0, spread_edit)?;
            thread::sleep(Duration::from_millis(100));
            if player.status().held_voices != 24 || player.status().output_peak < 1e-8 {
                return Err("Live Spread edit destroyed a partially stolen group".into());
            }
            let group4 = radias_synth_domain::voice_group::VoiceGroupProgram {
                raw: 130,
                detune: 64,
                spread: 127,
            };
            player.timbre_voice_group(0, group4)?;
            thread::sleep(Duration::from_millis(100));
            if player.status().active_voices != 0 || player.status().held_voices != 0 {
                return Err("Live group count edit did not retire the timbre".into());
            }
            input.midi(&[0x90, 64, 100])?;
            thread::sleep(Duration::from_millis(100));
            if player.status().held_voices != 4 {
                return Err("Edited Unison count did not apply to next note".into());
            }
            player.timbre_voice_group(
                0,
                radias_synth_domain::voice_group::VoiceGroupProgram { raw: 2, ..group4 },
            )?;
            thread::sleep(Duration::from_millis(100));
            if player.status().active_voices != 0 || player.status().held_voices != 0 {
                return Err("Unison disable did not retire the timbre".into());
            }
            input.midi(&[0x90, 64, 100])?;
            thread::sleep(Duration::from_millis(100));
            if player.status().held_voices != 1 {
                return Err("Disabled Unison did not produce a single actor".into());
            }
            player.timbre_voice_group(0, group4)?;
            thread::sleep(Duration::from_millis(100));
            if player.status().active_voices != 0 || player.status().held_voices != 0 {
                return Err("Unison enable did not retire the single actor".into());
            }
            input.midi(&[0x90, 64, 100])?;
            thread::sleep(Duration::from_millis(100));
            if player.status().held_voices != 4 {
                return Err("Enabled Unison count was not retained".into());
            }
            input.midi(&[0x80, 64, 0])?;
            voice_group_edit_scenes.push(serde_json::json!({"initial_voices_per_note":count,"pressure_held":24,
                "Spread_edit_preserves_groups":true,"count_disable_enable_each_retires_held_actors":true,
                "subsequent_note_counts":[4,1,4]}));
            for note in 48..55 {
                input.midi(&[0x80, note, 0])?;
            }
            thread::sleep(Duration::from_millis(400));
            if player.status().active_voices != 0 || player.status().held_voices != 0 {
                return Err("Partially stolen Unison group did not release".into());
            }
            voice_group_pressure_scenes
                .push(serde_json::json!({"voices_per_note":count,"note_ons":7,
                "maximum_held":24,"all_final_notes_released":true,"output_peaks":peaks}));
        }
        player.stop()?;
        thread::sleep(Duration::from_millis(40));
    }
    let completed = player.status();
    let passed = final_state.active_voices == 0
        && final_state.held_voices == 0
        && !final_state.failed
        && !all_sound_off.failed
        && final_state.audible_frames > 1000
        && completed.deadline_misses == 0
        && !completed.failed
        && channels.active_voices == 4
        && all_notes_off.active_voices == 2
        && all_notes_off.held_voices == 2
        && all_sound_off.active_voices == 0;
    let mut report = serde_json::json!({"passed":passed,"device":final_state.device,"sample_rate":final_state.sample_rate,"native_internal_tempo_sync":tempo_sync,"tempo_changes_under_24_voice_load":tempo_sync,
        "native_EG1_filter_connected":auxiliary,"native_EG3_virtual_patch_connected":auxiliary,
        "native_pan_and_pan_virtual_patch_connected":auxiliary,
        "live_amplifier_level_silence":live_level_silence,"live_amplifier_level_restore":live_level_restore,
        "native_secondary_pitch_and_modulation_connected":secondary,
        "live_secondary_only_audio":secondary && chord.audible_frames > 1000,
        "live_secondary_changes_under_24_voice_load":secondary,
        "live_mixer_zero_input_below_minus_120_dbfs":live_mixer_quiet,"muted_mixer_output_peak":muted_mixer_peak,
        "live_mixer_restore":live_mixer_restore,"secondary_only_chord_output_peak":chord.output_peak,
        "live_primary_forms_under_24_voice_load":live_primary_forms,
        "live_primary_controls_under_24_voice_load":primary,
        "live_primary_control_waveforms":if primary {4} else {0},
        "live_cross_control_waveforms":if cross || unison || vpm {4} else {0},
        "live_cross_controls_under_24_voice_load":cross || unison || vpm,
        "native_primary_coefficients_compiled":true,
        "four_timbre_chord_voices":chord.active_voices,"after_one_note_release":release.active_voices,
        "pressure_voices":full.active_voices,"after_stealing":steal.active_voices,"after_all_note_off":final_state.active_voices,
        "audible_frames":all_sound_off.audible_frames,"callbacks":all_sound_off.callbacks,"worst_callback_ms":all_sound_off.worst_render_ns as f64/1e6,
        "midi_channel_voices":channels.active_voices,"after_channel_all_notes_off":all_notes_off.active_voices,"after_channel_all_sound_off":all_sound_off.active_voices,
        "deadline_misses":all_sound_off.deadline_misses,"device_failed":final_state.failed||all_sound_off.failed,"cpu_emulation_in_renderer":false,
        "live_modulation_connected":modulated,"native_device_modulation_clock":modulated,"original_hpi_latency_matched":false,"complete_radias_engine":false});
    report["live_unison_control_waveforms"] = (if unison || vpm { 4 } else { 0 }).into();
    report["native_noise_formant_controls_connected"] = noise.into();
    report["native_full_amplifier_program_connected"] = amp_program.into();
    report["native_stored_pitch_and_midi_bend_wheel_connected"] = pitch_controls.into();
    report["native_portamento_time_curves_CC65_and_VP_connected"] = portamento.into();
    report["native_live_mono_last_low_high_single_multi_trigger"] = mono.into();
    report["mono_scenes"] = mono_scenes.into();
    report["sustain_scenes"] = sustain_scenes.into();
    report["note_group_scenes"] = note_group_scenes.into();
    report["voice_group_scenes"] = voice_group_scenes.into();
    report["voice_group_pressure_scenes"] = voice_group_pressure_scenes.into();
    report["voice_group_edit_scenes"] = voice_group_edit_scenes.into();
    report["native_live_Unison_Detune_Spread_count_enable_handlers_connected"] =
        voice_groups.into();
    report["native_surviving_group_repair_under_poly_pressure"] = voice_groups.into();
    report["native_instrument_Unison_groups_connected"] = voice_groups.into();
    report["native_live_repeated_note_age_tag_groups"] = note_groups.into();
    report["native_live_sustain_CC64_Poly_Mono_and_program_enable"] = sustain.into();
    report["final_callback_deadline_misses"] = completed.deadline_misses.into();
    report["final_worst_callback_ms"] = (completed.worst_render_ns as f64 / 1e6).into();
    report["live_portamento_curve_switch_combinations_under_24_render_slots"] =
        live_portamento_combinations.into();
    report["live_midi_pitch_combinations_under_24_render_slots"] =
        live_midi_pitch_combinations.into();
    report["channel_release_wait_ms_after_initial_250ms"] =
        (channel_release_started.elapsed().as_millis() as u64).into();
    report["live_amplifier_program_combinations_under_24_render_slots"] =
        live_amplifier_program_combinations.into();
    report["native_EG2_virtual_patch_source_uses_level_sensitivity"] = amp_program.into();
    report["native_AMP_key_tracking_under_24_held_render_slots"] = amp_key.into();
    report["live_noise_formant_combinations_under_24_render_slots"] =
        live_noise_combinations.into();
    report["mixer_noise_only_output_peak"] = mixer_noise_peak.into();
    report["physical_frame_banks_connected"] = noise.into();
    report["live_unison_controls_under_24_held_render_slots"] = (unison || vpm).into();
    report["original_unison_allocation_budget_qualified"] = false.into();
    report["live_vpm_control_waveforms"] = (if vpm { 4 } else { 0 }).into();
    report["live_vpm_controls_under_24_held_render_slots"] = vpm.into();
    report["original_primary_mode_change_allocation_budget_qualified"] = false.into();
    report["live_dual_filter_route_type_control_combinations"] =
        live_dual_filter_combinations.into();
    report["live_dual_filter_under_24_held_render_slots"] = dual_filter.into();
    report["native_regular_Filter2_controller_with_EG_key_and_LINK"] = filter2_controller.into();
    report["live_comb_under_24_held_render_slots"] = comb.into();
    report["live_comb_route_target_combinations"] = live_comb_combinations.into();
    report["comb_controller_targets_from_original_used"] = false.into();
    report["native_comb_controller_compilation_used"] = comb.into();
    report["native_comb_link_and_four_virtual_patch_destinations_connected"] = comb.into();
    report["original_dual_filter_mode_change_allocation_budget_qualified"] = false.into();
    report["live_shaper_route_position_depth_combinations"] = live_shaper_combinations.into();
    report["live_drive_and_hard_clip_under_24_held_render_slots"] = shaper.into();
    report["live_all_waveshapers_under_24_held_render_slots"] = waveshapers.into();
    report["live_lfo_virtual_patch_shaper_depth_connected"] = waveshapers.into();
    report["native_shaper_target_and_feedback_computed"] = shaper.into();
    report["original_shaper_live_mode_change_lifecycle_qualified"] = false.into();
    fs::write(
        root.join(if amp_key {
            "runs/native-clone/amplifier-key-device.json"
        } else if voice_groups {
            "runs/native-clone/voice-groups-device.json"
        } else if note_groups {
            "runs/native-clone/note-groups-device.json"
        } else if sustain {
            "runs/native-clone/sustain-device.json"
        } else if mono {
            "runs/native-clone/mono-device.json"
        } else if portamento {
            "runs/native-clone/portamento-device.json"
        } else if pitch_controls {
            "runs/native-clone/note-pitch-device.json"
        } else if amp_program {
            "runs/native-clone/amplifier-program-device.json"
        } else if noise {
            "runs/native-clone/noise-device.json"
        } else if filter2_controller {
            "runs/native-clone/filter2-controller-device.json"
        } else if comb {
            "runs/native-clone/comb-device.json"
        } else if waveshapers {
            "runs/native-clone/waveshapers-device.json"
        } else if shaper {
            "runs/native-clone/drive-clip-device.json"
        } else if dual_filter {
            "runs/native-clone/dual-filter-device.json"
        } else if vpm {
            "runs/native-clone/vpm-control-device.json"
        } else if unison {
            "runs/native-clone/unison-control-device.json"
        } else if cross {
            "runs/native-clone/cross-control-device.json"
        } else if primary {
            "runs/native-clone/primary-control-device.json"
        } else if secondary {
            "runs/native-clone/secondary-device.json"
        } else if auxiliary {
            "runs/native-clone/aux-envelope-device.json"
        } else if tempo_sync {
            "runs/native-clone/tempo-device.json"
        } else if modulated {
            "runs/native-clone/modulation-device.json"
        } else {
            "runs/native-clone/polyphony-device.json"
        }),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native polyphony device gate failed".into());
    }
    Ok(())
}
