#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("Stored program device gate requires desktop-io");
}
#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::{
        amplifier::ControllerTables, modulation::VoiceModulationTables,
        portamento::PortamentoTables,
    };
    use radias_synth_domain::{control_slew::SlewWeights, program::Program};
    use radias_synth_infrastructure::{
        audio::NativePlayer,
        firmware::{self, MasterTables},
        prepared::{ControlMap, PreparedVoice},
        rdl,
        stored_program::compile_program,
    };
    use std::{fs, path::PathBuf, thread, time::Duration};
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let parameter_templates = std::env::args().any(|a| a == "--parameter-templates");
    let shaper_queue = std::env::args().any(|a| a == "--shaper-queue");
    let comb_queue = shaper_queue || std::env::args().any(|a| a == "--comb-queue");
    let filter2_queue = comb_queue || std::env::args().any(|a| a == "--filter2-queue");
    let noise_queue = filter2_queue || std::env::args().any(|a| a == "--noise-queue");
    let scalar_queue = noise_queue || std::env::args().any(|a| a == "--scalar-queue");
    let pitch_queue = scalar_queue || std::env::args().any(|a| a == "--pitch-queue");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&master)?;
    let plans = ["saw", "pulse", "triangle", "sine"]
        .map(|name| {
            PreparedVoice::from_program_json(
                &fs::read(root.join(format!("assets/native-va/{name}.json"))).unwrap(),
            )
            .unwrap()
        })
        .into_iter()
        .collect::<Vec<_>>();
    let base = plans[0].parameters.filter;
    let player = NativePlayer::with_clock(
        plans,
        tables.waveform()?,
        Some((tables.pitch()?, tables.bandwidth()?)),
        Some(ControllerTables {
            curves: firmware::envelope_curves(&sys)?,
            timing: firmware::envelope_timing_tables(&sys)?,
            amplifier: firmware::amplifier_tables(&sys)?,
        }),
        Some(firmware::voice_cost_tables(&sys)?),
        Some(VoiceModulationTables {
            lfo: firmware::lfo_tables(&sys)?,
            matrix: firmware::modulation_tables(&sys)?,
            pitch: tables.pitch()?,
            bandwidth: tables.bandwidth()?,
        }),
        Some(firmware::lfo_tempo_tables(&sys)?),
    )?;
    player.configure_constructor_filter_mix(tables.filter_mix()?)?;
    player.gain(0.05);
    if pitch_queue {
        let slave = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
        player.configure_controller_service(firmware::controller_service_timer(&sys)?)?;
        player.configure_amplifier_delivery(firmware::amplifier_rate_table(&sys)?)?;
        player.configure_pitch_delivery(
            [
                tables.pitch_receiver_rom()?,
                MasterTables::from_host_stream(&slave)?.pitch_receiver_rom()?,
            ],
            firmware::primary_pitch_sender_table(&sys)?,
        )?;
    }
    player.configure_note_pitch(firmware::note_pitch_tables(&sys)?, Default::default(), 0)?;
    player.configure_portamento(PortamentoTables {
        rates: firmware::portamento_rates(&sys)?,
        curves: firmware::portamento_curves(&sys)?,
    })?;
    player.configure_voice_groups(firmware::voice_group_tables(&sys)?)?;
    player.configure_noise(
        tables.pitch()?,
        tables.noise_pitch()?,
        firmware::formant_counter_seeds(&sys)?,
    )?;
    player.configure_comb(firmware::comb_control_tables(&sys)?)?;
    player.configure_mixer(firmware::mixer_scales(&sys)?)?;
    player.configure_secondary(firmware::fine_tune_table(&sys)?)?;
    player.controller_filter_tables(firmware::controller_filter_tables(&sys)?)?;
    player.configure_filter2(firmware::filter2_control_tables(&sys)?)?;
    player.configure_pan(
        firmware::pan_tables(&sys)?,
        SlewWeights {
            target: tables.word(0x4026)? as i16,
            memory: tables.word(0x4027)? as i16,
        },
    )?;
    let map = ControlMap::from_system(&sys)?;
    let mix = tables.filter_mix()?;
    let raw = fs::read(out.join("stored-four-timbres.program.bin"))?;
    let program = Program::from_bytes(&raw).map_err(|_| "Invalid stored fixture")?;
    let templates = firmware::parameter_template_tables(&sys, tables.filter_mix()?)?;
    let compile = |program: &Program,
                   global,
                   map: &ControlMap,
                   mix: &radias_synth_domain::filter_control::FilterMixTable,
                   base|
     -> Result<_, String> {
        let compiled = compile_program(program, global, map, mix, base)?;
        if parameter_templates {
            compiled
                .with_parameter_templates(program, &templates)
                .map_err(|e| format!("{e:?}"))
        } else {
            Ok(compiled)
        }
    };
    let compiled = compile(&program, 0, &map, &mix, base)?;
    let input = player.input();
    let mut cases = Vec::new();
    for note in [47u8, 48, 60, 64, 67, 72, 73] {
        player.load_program(compiled)?;
        thread::sleep(Duration::from_millis(60));
        input.midi(&[0x90, note, 100])?;
        thread::sleep(Duration::from_millis(100));
        let status = player.status();
        let expected = compiled
            .stored
            .timbres
            .iter()
            .filter(|t| t.accepts(0, note))
            .count() as u32;
        if status.active_voices != expected
            || status.held_voices != expected
            || (expected != 0 && status.output_peak < 1e-8)
        {
            return Err(
                format!("Stored split note{note}: expected{expected}, actual{status:?}").into(),
            );
        }
        cases.push(serde_json::json!({"note":note,"active_voices":status.active_voices,"output_peak":status.output_peak}));
        input.midi(&[0x80, note, 0])?;
        thread::sleep(Duration::from_millis(350));
        if player.status().active_voices != 0 {
            return Err("Stored program release left actors".into());
        }
    }
    player.load_program(compiled)?;
    thread::sleep(Duration::from_millis(60));
    input.midi(&[0x90, 60, 100])?;
    thread::sleep(Duration::from_millis(100));
    if player.status().held_voices != 3 {
        return Err("Stored four-timbre layer fixture failed".into());
    }
    let bank = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let unavailable = compile(&bank[0], 0, &map, &mix, base)?;
    if player.load_program(unavailable).is_ok() {
        return Err("Unavailable PCM generator was silently substituted".into());
    }
    thread::sleep(Duration::from_millis(60));
    if player.status().held_voices != 3 {
        return Err("Rejected program destroyed current actors".into());
    }
    player.load_program(compiled)?;
    thread::sleep(Duration::from_millis(100));
    if player.status().active_voices != 0 || player.status().held_voices != 0 {
        return Err("Atomic stored program load did not retire old actors".into());
    }
    let mut pressure = None;
    if pitch_queue {
        let mut raw = fs::read(out.join("drum-common-center.program.bin"))?;
        raw[24] = 0;
        raw[80] = 128;
        let owner: [u8; 228] = raw[48..276].try_into()?;
        for timbre in 1..4 {
            raw[48 + 228 * timbre..276 + 228 * timbre].copy_from_slice(&owner);
        }
        player.load_program(compile(
            &Program::from_bytes(&raw).map_err(|_| "Invalid pressure fixture")?,
            0,
            &map,
            &mix,
            base,
        )?)?;
        thread::sleep(Duration::from_millis(60));
        for timbre in 0..4 {
            player.timbre(timbre, true, timbre)?;
        }
        for timbre in 0..4 {
            for note in 60..66 {
                input.midi(&[0x90 | timbre, note, 100])?;
            }
        }
        thread::sleep(Duration::from_millis(250));
        let held = player.status();
        if held.held_voices != 24 || held.failed || held.output_peak < 1e-8 {
            return Err(format!("Pitch queue device pressure failed: {held:?}").into());
        }
        for timbre in 0..4 {
            input.midi(&[0xe0 | timbre, 127, 127])?;
            player.timbre_secondary(
                timbre,
                radias_synth_application::secondary::SecondaryProgram {
                    selection: 32,
                    pitch: Default::default(),
                },
            )?;
            if scalar_queue {
                player.timbre_primary_control(
                    timbre,
                    radias_synth_application::primary::PrimaryProgram {
                        selection: 3,
                        control: radias_synth_domain::controller_primary::PrimaryControl {
                            control1: 110,
                            control2: 30,
                            ..Default::default()
                        },
                    },
                )?;
                player.timbre_mixer(
                    timbre,
                    radias_synth_application::mixer::MixerProgram {
                        selections: [3, 32],
                        levels: [90, 40, 20],
                        manual_offsets: [0; 3],
                    },
                )?;
                player.timbre_pan(
                    timbre,
                    radias_synth_domain::controller_pan::PanControl {
                        position: 20 + 25 * timbre,
                        ..Default::default()
                    },
                )?;
            }
        }
        thread::sleep(Duration::from_millis(250));
        let edited = player.status();
        if edited.held_voices != 24 || edited.failed || edited.output_peak < 1e-8 {
            return Err(format!("Pitch queue live device edit failed: {edited:?}").into());
        }
        pressure = Some(
            serde_json::json!({"held_voices":edited.held_voices,"peak":edited.output_peak,"live_bend_and_Sync":true}),
        );
        if scalar_queue {
            for timbre in 0..4 {
                player.timbre_primary_control(
                    timbre,
                    radias_synth_application::primary::PrimaryProgram {
                        selection: 51,
                        control: radias_synth_domain::controller_primary::PrimaryControl {
                            control1: 64,
                            control2: 115,
                            ..Default::default()
                        },
                    },
                )?;
            }
            thread::sleep(Duration::from_millis(250));
            let vpm = player.status();
            if vpm.held_voices != 24 || vpm.failed || vpm.output_peak < 1e-8 {
                return Err(format!("VPM queued control device failed: {vpm:?}").into());
            }
            pressure.as_mut().unwrap()["CTRL1_ratio_mixer_pan_and_VPM_under24_actors"] =
                true.into();
        }
        if noise_queue {
            let mut scenes = Vec::new();
            for selection in [4u8, 5] {
                for (control1, control2) in [(110, 35), (32, 115)] {
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
                                selections: [selection, 32],
                                levels: [90, 40, 20],
                                manual_offsets: [0; 3],
                            },
                        )?;
                    }
                    thread::sleep(Duration::from_millis(250));
                    let status = player.status();
                    if status.held_voices != 24 || status.failed || status.output_peak < 1e-8 {
                        return Err(format!(
                            "24-actor Noise/Formant control device failed: {status:?}"
                        )
                        .into());
                    }
                    scenes.push(serde_json::json!({"selection":selection,"control1":control1,"control2":control2,"held_voices":status.held_voices,"peak":status.output_peak}));
                }
            }
            pressure.as_mut().unwrap()["Noise_Formant_CTRL1_CTRL2_under24_actors"] = scenes.into();
        }
        if filter2_queue {
            let mut scenes = Vec::new();
            for link in [0u8, 128] {
                for output in 0..3 {
                    for route in 1..=3 {
                        let selected = link | (output << 4) | route;
                        raw[86] = 3; // Original Sine descriptor; distinct MIDI owners configured before notes.
                        raw[97] = selected;
                        raw[106] = 72;
                        raw[107] = 80;
                        let owner: [u8; 228] = raw[48..276].try_into()?;
                        for timbre in 1..4 {
                            raw[48 + 228 * timbre..276 + 228 * timbre].copy_from_slice(&owner);
                        }
                        let compiled = compile_program(
                            &Program::from_bytes(&raw)
                                .map_err(|_| "Invalid Filter2 pressure fixture")?,
                            0,
                            &map,
                            &mix,
                            base,
                        )?;
                        player.load_program(compiled)?;
                        thread::sleep(Duration::from_millis(60));
                        for timbre in 0..4 {
                            player.timbre(timbre, true, timbre)?;
                        }
                        for timbre in 0..4 {
                            for note in 60..66 {
                                input.midi(&[0x90 | timbre, note, 100])?;
                            }
                        }
                        for (cutoff, resonance) in [(37, 21), (100, 96)] {
                            for timbre in 0..4 {
                                let mut filter = compiled.timbres[timbre as usize]
                                    .dynamic_filter2
                                    .ok_or("Regular Filter2 controller absent")?;
                                filter.controls.cutoff.cutoff = cutoff;
                                filter.controls.cutoff.linked_cutoff = cutoff;
                                filter.controls.resonance.resonance = resonance;
                                filter.controls.resonance.linked_resonance = resonance;
                                player.timbre_filter_routing(
                                    timbre,
                                    filter.route & 3,
                                    compiled.timbres[timbre as usize].filter2,
                                )?;
                                player.timbre_filter2_program(timbre, filter)?;
                            }
                            thread::sleep(Duration::from_millis(120));
                            let status = player.status();
                            if status.held_voices != 24
                                || status.failed
                                || status.output_peak < 1e-8
                            {
                                return Err(format!(
                                    "Filter2 route{selected} device pressure failed: {status:?}"
                                )
                                .into());
                            }
                            scenes.push(serde_json::json!({"route":selected,"cutoff":cutoff,"resonance":resonance,"held_voices":status.held_voices,"peak":status.output_peak}));
                        }
                    }
                }
            }
            pressure.as_mut().unwrap()["regular_Filter2_all18_routes_with_live_controls_under24_actors"] =
                scenes.into();
        }
        if comb_queue {
            let mut scenes = Vec::new();
            for link in [0u8, 128] {
                for route in 1..=3 {
                    let selected = link | 48 | route;
                    raw[86] = 3;
                    raw[97] = selected;
                    let owner: [u8; 228] = raw[48..276].try_into()?;
                    for timbre in 1..4 {
                        raw[48 + 228 * timbre..276 + 228 * timbre].copy_from_slice(&owner);
                    }
                    let compiled = compile_program(
                        &Program::from_bytes(&raw).map_err(|_| "Invalid Comb pressure fixture")?,
                        0,
                        &map,
                        &mix,
                        base,
                    )?;
                    player.load_program(compiled)?;
                    thread::sleep(Duration::from_millis(60));
                    for timbre in 0..4 {
                        player.timbre(timbre, true, timbre)?;
                    }
                    for timbre in 0..4 {
                        for note in 60..64 {
                            input.midi(&[0x90 | timbre, note, 100])?;
                        }
                    }
                    for (cutoff, resonance) in [(37, 21), (100, 96)] {
                        for timbre in 0..4 {
                            let mut controls = compiled.timbres[timbre as usize]
                                .comb
                                .ok_or("Comb controller absent")?;
                            controls.cutoff.cutoff = cutoff;
                            controls.cutoff.linked_cutoff = cutoff;
                            controls.resonance.resonance = resonance;
                            controls.resonance.linked_resonance = resonance;
                            player.timbre_comb(timbre, route, controls)?;
                        }
                        thread::sleep(Duration::from_millis(120));
                        let status = player.status();
                        if status.held_voices != 16 || status.failed || status.output_peak < 1e-8 {
                            return Err(format!(
                                "Comb route{selected} device pressure failed: {status:?}"
                            )
                            .into());
                        }
                        scenes.push(serde_json::json!({"route":selected,"cutoff":cutoff,"resonance":resonance,"held_voices":status.held_voices,"peak":status.output_peak}));
                    }
                }
            }
            pressure.as_mut().unwrap()["Comb_all6_route_LINK_choices_with_live_controls_under16_fresh_actors"] =
                scenes.into();
        }
        if shaper_queue {
            let mut scenes = Vec::new();
            let costs = firmware::voice_cost_tables(&sys)?;
            for mode in 1..=12 {
                for position in 0..=1 {
                    for route in 0..=3 {
                        let program = radias_synth_application::shaper::ShaperProgram {
                            mode: radias_synth_application::shaper::ShaperMode::from_panel(mode)
                                .unwrap(),
                            ..Default::default()
                        };
                        let cost = costs
                            .cost(radias_synth_domain::voice_allocation::VoiceCostParameters {
                                primary: 3,
                                secondary: 0,
                                filter_route: route,
                                drive_mode: program.allocation_mode(),
                                shaper_type: program.allocation_type(),
                            })
                            .unwrap();
                        let owner_notes = if cost * 12 < 65536 { 6 } else { 5 };
                        raw[86] = 3;
                        raw[97] = route;
                        raw[110] = program.allocation_mode() | (position << 4);
                        raw[111] = program.allocation_type();
                        raw[112] = 80;
                        let owner: [u8; 228] = raw[48..276].try_into()?;
                        for timbre in 1..4 {
                            raw[48 + 228 * timbre..276 + 228 * timbre].copy_from_slice(&owner);
                        }
                        let compiled = compile_program(
                            &Program::from_bytes(&raw)
                                .map_err(|_| "Invalid shaper device fixture")?,
                            0,
                            &map,
                            &mix,
                            base,
                        )?;
                        player.load_program(compiled)?;
                        thread::sleep(Duration::from_millis(60));
                        for timbre in 0..4 {
                            player.timbre(timbre, true, timbre)?;
                        }
                        for timbre in 0..4 {
                            for note in 60..60 + owner_notes {
                                input.midi(&[0x90 | timbre, note, 100])?;
                            }
                        }
                        for depth in [35, 100] {
                            for timbre in 0..4 {
                                player.timbre_shaper(timbre, mode, position, depth)?;
                            }
                            thread::sleep(Duration::from_millis(100));
                            let status = player.status();
                            if status.held_voices != u32::from(owner_notes) * 4
                                || status.failed
                                || status.output_peak < 1e-8
                            {
                                return Err(format!("Shaper mode{mode}/position{position}/route{route} device failed: {status:?}").into());
                            }
                            scenes.push(serde_json::json!({"mode":mode,"position":position,"route":route,"depth":depth,"held_voices":status.held_voices,"peak":status.output_peak}));
                        }
                    }
                }
            }
            pressure.as_mut().unwrap()["shaper_all96_mode_position_route_choices_with_live_depth_and_original_budget"] =
                scenes.into();
        }
        player.stop()?;
        thread::sleep(Duration::from_millis(100));
        if player.status().active_voices != 0 {
            return Err("Pitch queue Stop retained actors".into());
        }
    }
    let status = player.status();
    let passed = !status.failed && status.deadline_misses == 0 && status.audible_frames > 1000;
    let report = serde_json::json!({"passed":passed,"sample_rate":status.sample_rate,"device":status.device,
        "audible_frames":status.audible_frames,"deadline_misses":status.deadline_misses,"worst_callback_ms":status.worst_render_ns as f64/1e6,
        "stored_four_timbres_loaded_atomically":true,"stored_key_windows_cases":cases,"failed_generator_preserves_current_sound":true,
        "CPU_emulation_in_renderer":false,"parameter_templates_from_raw_program_used":parameter_templates,"full_bank_audio_parity_qualified":false,"complete_native_engine":false});
    let mut report = report;
    if pitch_queue {
        report["production_shared_pitch_AMP_Filter1_queue"] = true.into();
        report["four_timbre_24_actor_pressure"] = pressure.unwrap();
    }
    fs::write(
        out.join(if parameter_templates {
            "parameter-template-device.json"
        } else if shaper_queue {
            "shaper-queue-device.json"
        } else if comb_queue {
            "comb-queue-device.json"
        } else if filter2_queue {
            "filter2-queue-device.json"
        } else if noise_queue {
            "noise-queue-device.json"
        } else if scalar_queue {
            "control-queue-device.json"
        } else if pitch_queue {
            "pitch-queue-device.json"
        } else {
            "stored-program-device.json"
        }),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Stored program realtime device gate failed".into());
    }
    Ok(())
}
