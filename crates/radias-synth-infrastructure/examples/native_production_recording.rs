#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("Native production recording requires desktop-io");
}
#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_domain::{Sample, pan::StereoFrame};
    use radias_synth_infrastructure::wav;
    use std::{fs, path::PathBuf, time::Instant};
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let templates_enabled = std::env::args().any(|a| a == "--parameter-templates");
    let prefix = if templates_enabled {
        "parameter-template-production"
    } else {
        "native-production-drum-and-mixed"
    };
    let started = Instant::now();
    let reference = render(&root, 128, false)?;
    for chunk in [1, 31, 257, 512, 2048] {
        let other = render(&root, chunk, false)?;
        if reference != other {
            return Err(format!("Production block partition{chunk} differs").into());
        }
        if chunk == 512 {
            let frames: Vec<[Sample; 2]> = other.iter().map(|s| [s.left, s.right]).collect();
            wav::write_stereo(&out.join(format!("{prefix}-512.wav")), &frames)?;
        }
    }
    let with_silent_event = render(&root, 31, true)?;
    if reference != with_silent_event {
        return Err("Silent instrument event advanced the already sounding actors".into());
    }
    let pressure_started = Instant::now();
    let pressure = render_with_case(&root, 128, false, true, None, None, false)?;
    let pressure_seconds = pressure_started.elapsed().as_secs_f64();
    let pressure_other = render_with_case(&root, 31, false, true, None, None, false)?;
    if pressure != pressure_other {
        return Err("24-voice AMP transport partition changes sound".into());
    }
    let mut mono_frames = 0;
    for mode in [0u8, 1, 2, 3, 64, 65, 66, 67] {
        let mono = render_with_case(&root, 128, false, false, Some(mode), None, false)?;
        let other = render_with_case(&root, 31, false, false, Some(mode), None, false)?;
        if mono != other {
            return Err(format!("Mono AMP transport mode{mode} changes across partitions").into());
        }
        mono_frames += mono.len();
    }
    let mut pitch_mode_frames = 0;
    for selection in [
        0u8, 1, 2, 3, 4, 5, 16, 17, 18, 19, 32, 33, 34, 35, 48, 49, 50, 51,
    ] {
        let a = render_with_case(&root, 128, false, true, None, Some(selection), false)?;
        let b = render_with_case(&root, 31, false, true, None, Some(selection), false)?;
        if a != b || !a.iter().any(|s| *s != StereoFrame::default()) {
            return Err(format!("Primary pitch mode{selection} partition/sound failed").into());
        }
        pitch_mode_frames += a.len();
        let a = render_with_case(&root, 128, false, false, Some(64), Some(selection), false)?;
        let b = render_with_case(&root, 31, false, false, Some(64), Some(selection), false)?;
        if a != b {
            return Err(format!("Mono primary pitch mode{selection} partition failed").into());
        }
        pitch_mode_frames += a.len();
    }
    let mut noise_pressure = Vec::new();
    for selection in [4u8, 5] {
        let now = Instant::now();
        let a = render_with_case(&root, 128, false, true, None, Some(selection), true)?;
        let seconds = now.elapsed().as_secs_f64();
        let b = render_with_case(&root, 31, false, true, None, Some(selection), true)?;
        if a != b || !a.iter().any(|s| *s != StereoFrame::default()) {
            return Err(format!("24-actor Noise/Formant mode{selection} delivery failed").into());
        }
        noise_pressure.push(serde_json::json!({"selection":selection,"frames":a.len(),"render_seconds":seconds,"partitions_equal":true}));
    }
    let mut filter2_pressure = Vec::new();
    let mut comb_pressure = Vec::new();
    for link in [0u8, 128] {
        for output in 0..4 {
            for route in 1..=3 {
                let selection = link | (output << 4) | route;
                let config = RenderCase {
                    pressure: true,
                    full_pressure: true,
                    filter2_route: Some(selection),
                    primary_mode: Some(3),
                    notes_per_timbre: if output == 3 { Some(4) } else { None },
                    ..Default::default()
                };
                let now = Instant::now();
                let a = render_config(&root, 128, config)?;
                let seconds = now.elapsed().as_secs_f64();
                let b = render_config(&root, 31, config)?;
                if a != b || !a.iter().any(|s| *s != StereoFrame::default()) {
                    return Err(format!(
                        "Regular Filter2 route{selection} pressure changes across partitions"
                    )
                    .into());
                }
                let mono = RenderCase {
                    mono_mode: Some(64),
                    filter2_route: Some(selection),
                    primary_mode: Some(3),
                    ..Default::default()
                };
                let a_mono = render_config(&root, 128, mono)?;
                let b_mono = render_config(&root, 31, mono)?;
                if a_mono != b_mono {
                    return Err(format!(
                        "Mono Filter2 route{selection} delivery changes across partitions"
                    )
                    .into());
                }
                let entry = serde_json::json!({"route":selection,"pressure_frames":a.len(),"pressure_render_seconds":seconds,"Mono_frames":a_mono.len(),"partitions_equal":true,"held_pressure_actors":if output==3 {16} else {24}});
                if output == 3 {
                    comb_pressure.push(entry);
                } else {
                    filter2_pressure.push(entry);
                }
            }
        }
    }
    let mut shaper_cases = Vec::new();
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let costs = radias_synth_infrastructure::firmware::voice_cost_tables(&sys)?;
    for mode in 1..=12 {
        for position in 0..=1 {
            for route in 0..=3 {
                let program = radias_synth_application::shaper::ShaperProgram {
                    mode: radias_synth_application::shaper::ShaperMode::from_panel(mode).unwrap(),
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
                let case = RenderCase {
                    pressure: true,
                    full_pressure: true,
                    primary_mode: Some(3),
                    filter2_route: Some(route),
                    notes_per_timbre: Some(owner_notes),
                    shaper: Some((mode, position)),
                    ..Default::default()
                };
                let now = Instant::now();
                let a = render_config(&root, 128, case)?;
                let seconds = now.elapsed().as_secs_f64();
                let b = render_config(&root, 31, case)?;
                if a != b || !a.iter().any(|v| *v != StereoFrame::default()) {
                    return Err(format!(
                        "Shaper mode{mode}/position{position}/route{route} pressure differs"
                    )
                    .into());
                }
                let mono = RenderCase {
                    mono_mode: Some(64),
                    primary_mode: Some(3),
                    filter2_route: Some(route),
                    shaper: Some((mode, position)),
                    ..Default::default()
                };
                let a_mono = render_config(&root, 128, mono)?;
                let b_mono = render_config(&root, 31, mono)?;
                if a_mono != b_mono {
                    return Err(format!(
                        "Shaper mode{mode}/position{position}/route{route} Mono differs"
                    )
                    .into());
                }
                shaper_cases.push(serde_json::json!({"mode":mode,"position":position,"route":route,"held_pressure_actors":owner_notes*4,"pressure_frames":a.len(),"pressure_render_seconds":seconds,"Mono_frames":a_mono.len(),"partitions_equal":true}));
            }
        }
    }
    let frames: Vec<[Sample; 2]> = reference.iter().map(|s| [s.left, s.right]).collect();
    wav::write_stereo(&out.join(format!("{prefix}.wav")), &frames)?;
    let audible = reference
        .iter()
        .filter(|s| **s != StereoFrame::default())
        .count();
    if audible == 0 {
        return Err("Production native recording is silent".into());
    }
    let report = serde_json::json!({"passed":true,"frames":reference.len(),"audible_frames":audible,
        "sample_rate":48000,"channels":2,"block_partitions":[128,1,31,257,512,2048],
        "render_seconds_total":started.elapsed().as_secs_f64(),
        "production_generator_command_bus_and_note_factory_shared_with_CoreAudio":true,
        "parameter_templates_compiled_from_lossless_program_and_SYS_tables":templates_enabled,
        "nonaligned_events_and_silent_instrument_do_not_advance_unrelated_sound":true,
        "audio_device_opened":false,"firmware_interpreter_used":false,
        "ordinary_poly_notes_drum_pads_and_MIDI_live_body_edits_exercised":true,
        "all24_actors_across_four_timbres_AMP_queue_checked":true,"24_voice_frames":pressure.len(),"24_voice_render_seconds":pressure_seconds,
        "Mono_single_multi_all_priorities_four_timbre_DSP_state_retention_checked":true,"Mono_modes":8,"Mono_total_frames":mono_frames,
        "production_pitch_18_modes_four_timbres_checked":true,"production_pitch_mode_frames":pitch_mode_frames,"production_pitch_and_Unison_detune_share_AMP_Filter1_queue":true,
        "live_CTRL1_CTRL2_mixer_pan_queued_edits_four_timbres_and_24_actor_pressure":true,"Mono_full_mixer_pan_and_pitch_state_retained":true,
        "Noise_Formant_CTRL1_CTRL2_share_production_queue":true,"Noise_Formant_24_actor_pressure":noise_pressure,
        "Mono_primary_control_edits_wait_for_delivery":true,
        "regular_Filter2_queued_frequency_resonance_input_gain":true,"Filter2_all18_route_output_LINK_pressure_and_Mono_cases":filter2_pressure,
        "held_Mono_Filter2_state_and_coefficient_targets_retained":true,
        "Comb_all6_route_LINK_pressure_and_Mono_cases":comb_pressure,"held_Mono_Comb_entire_delay_memory_and_phase_retained":true,
        "shaper_all96_modes_positions_routes_pressure_and_Mono_cases":shaper_cases,"held_Mono_shaper_private_state_current_target_retained":true,
        "recorded_original_controls_or_samples_replayed":false,
        "original_complete_audio_parity_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join(if templates_enabled {
            "parameter-template-production-verification.json"
        } else {
            "native-production-recording-verification.json"
        }),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[cfg(feature = "desktop-io")]
fn render(
    root: &std::path::Path,
    chunk: usize,
    silent_event: bool,
) -> Result<Vec<radias_synth_domain::pan::StereoFrame>, Box<dyn std::error::Error>> {
    render_with_case(root, chunk, silent_event, false, None, None, false)
}
#[derive(Clone, Copy, Default)]
#[cfg(feature = "desktop-io")]
struct RenderCase {
    silent_event: bool,
    pressure: bool,
    mono_mode: Option<u8>,
    primary_mode: Option<u8>,
    full_pressure: bool,
    filter2_route: Option<u8>,
    notes_per_timbre: Option<u8>,
    shaper: Option<(u8, u8)>,
}
#[cfg(feature = "desktop-io")]
fn render_with_case(
    root: &std::path::Path,
    chunk: usize,
    silent_event: bool,
    pressure: bool,
    mono_mode: Option<u8>,
    primary_mode: Option<u8>,
    full_pressure: bool,
) -> Result<Vec<radias_synth_domain::pan::StereoFrame>, Box<dyn std::error::Error>> {
    render_config(
        root,
        chunk,
        RenderCase {
            silent_event,
            pressure,
            mono_mode,
            primary_mode,
            full_pressure,
            filter2_route: None,
            notes_per_timbre: None,
            shaper: None,
        },
    )
}
#[cfg(feature = "desktop-io")]
fn render_config(
    root: &std::path::Path,
    chunk: usize,
    case: RenderCase,
) -> Result<Vec<radias_synth_domain::pan::StereoFrame>, Box<dyn std::error::Error>> {
    let RenderCase {
        silent_event,
        pressure,
        mono_mode,
        primary_mode,
        full_pressure,
        filter2_route,
        notes_per_timbre,
        shaper,
    } = case;
    use radias_synth_application::{
        amplifier::ControllerTables, modulation::VoiceModulationTables,
        portamento::PortamentoTables,
    };
    use radias_synth_domain::{control_slew::SlewWeights, pan::StereoFrame, program::Program};
    use radias_synth_infrastructure::{
        audio::NativeOfflineRenderer,
        firmware::{self, MasterTables},
        prepared::{ControlMap, PreparedVoice},
        rdl,
        stored_program::{compile_drum_kit, compile_program},
    };
    use std::fs;
    let out = root.join("runs/native-clone");
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
    let mut renderer = NativeOfflineRenderer::with_clock(
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
    let player = renderer.controls();
    player.configure_controller_service(firmware::controller_service_timer(&sys)?)?;
    player.configure_amplifier_delivery(firmware::amplifier_rate_table(&sys)?)?;
    let slave = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    player.configure_pitch_delivery(
        [
            tables.pitch_receiver_rom()?,
            MasterTables::from_host_stream(&slave)?.pitch_receiver_rom()?,
        ],
        firmware::primary_pitch_sender_table(&sys)?,
    )?;
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
    let mut raw = fs::read(out.join("drum-common-center.program.bin"))?;
    raw[24] = 0;
    raw[64 + 0x10] = 128; // Ordinary Poly.
    if let Some(mode) = mono_mode {
        raw[80] = mode;
    }
    if let Some(mode) = primary_mode {
        raw[86] = mode;
        raw[87] = 80;
        raw[88] = 70;
    }
    if let Some(route) = filter2_route {
        raw[97] = route;
        raw[106] = 72;
        raw[107] = 80;
    }
    if let Some((mode, position)) = shaper {
        let program = radias_synth_application::shaper::ShaperMode::from_panel(mode).unwrap();
        raw[110] = (if mode == 1 { 1 } else { 2 }) | (position << 4);
        raw[111] = radias_synth_application::shaper::ShaperProgram {
            mode: program,
            ..Default::default()
        }
        .allocation_type();
        raw[112] = 80;
    }
    let ordinary = Program::from_bytes(&raw).unwrap();
    if pressure || mono_mode.is_some() {
        let owner: [u8; 228] = raw[48..276].try_into().unwrap();
        for timbre in 1..4 {
            raw[48 + 228 * timbre..276 + 228 * timbre].copy_from_slice(&owner);
        }
    }
    let ordinary = if pressure || mono_mode.is_some() {
        Program::from_bytes(&raw).unwrap()
    } else {
        ordinary
    };
    let compiled = compile_program(&ordinary, 0, &map, &mix, base)?;
    let templates = firmware::parameter_template_tables(&sys, tables.filter_mix()?)?;
    let attach_templates = std::env::args().any(|a| a == "--parameter-templates");
    let compiled = if attach_templates {
        compiled
            .with_parameter_templates(&ordinary, &templates)
            .map_err(|e| format!("{e:?}"))?
    } else {
        compiled
    };
    renderer.controls().load_program(compiled)?;
    let mut result = Vec::new();
    let append =
        |engine: &mut NativeOfflineRenderer, output: &mut Vec<StereoFrame>, count: usize| {
            let start = output.len();
            output.resize(start + count, StereoFrame::default());
            for block in output[start..].chunks_mut(chunk) {
                engine.render_into(block);
            }
        };
    let input = renderer.controls().input();
    if let Some(mode) = mono_mode {
        for timbre in 0..4 {
            renderer.controls().timbre(timbre, true, timbre)?;
        }
        for timbre in 0..4 {
            input.midi(&[0x90 | timbre, 60, 100])?;
        }
        append(&mut renderer, &mut result, 8197);
        if renderer.controls().status().held_voices != 4 {
            return Err("Mono scene lost its four initial actors".into());
        }
        let held: Vec<_> = (0..24)
            .filter_map(|slot| {
                renderer.active_voice(slot).map(|v| {
                    (
                        slot,
                        v.renderer.voice.filter.state,
                        v.renderer.current_filter(),
                        v.renderer.voice.envelope,
                        v.renderer.envelope_rate(),
                        v.renderer.envelope_target(),
                        v.renderer.current_pitch(),
                        v.renderer.voice.secondary,
                        v.renderer.pitch_state(),
                        v.renderer.scalar_state(),
                        v.renderer.filter2_state(),
                        v.renderer.shaper_state(),
                        v.renderer.comb.clone(),
                    )
                })
            })
            .collect();
        let next_note = if mode & 3 == 1 { 48 } else { 72 };
        for timbre in 0..4 {
            input.midi(&[0x90 | timbre, next_note, 63])?;
        }
        renderer.render_into(&mut []);
        for (
            slot,
            filter,
            coefficients,
            envelope,
            rate,
            target,
            pitch,
            secondary,
            pitch_state,
            scalar_state,
            filter2_state,
            shaper_state,
            comb,
        ) in held
        {
            let voice = renderer
                .active_voice(slot)
                .ok_or("Mono actor was replaced during restart")?;
            if voice.note != next_note
                || voice.renderer.voice.filter.state != filter
                || voice.renderer.current_filter() != coefficients
                || voice.renderer.voice.envelope != envelope
                || voice.renderer.envelope_rate() != rate
                || voice.renderer.envelope_target() != target
                || voice.renderer.current_pitch() != pitch
                || voice.renderer.voice.secondary != secondary
                || voice.renderer.pitch_state() != pitch_state
                || voice.renderer.scalar_state() != scalar_state
                || voice.renderer.filter2_state() != filter2_state
                || voice.renderer.shaper_state() != shaper_state
                || voice.renderer.comb != comb
            {
                return Err(
                    format!("Mono restart mode{mode}/slot{slot} reset active DSP memory").into(),
                );
            }
        }
        let retained: Vec<_> = (0..24)
            .filter_map(|slot| {
                renderer.active_voice(slot).map(|v| {
                    (
                        slot,
                        v.renderer.pitch_state(),
                        v.renderer.filter2_state(),
                        v.renderer.shaper_state(),
                        v.renderer.comb.clone(),
                    )
                })
            })
            .collect();
        for (control1, control2, frames) in [(110, 35, 1007), (32, 115, 1009)] {
            for timbre in 0..4 {
                renderer.controls().timbre_primary_control(
                    timbre,
                    radias_synth_application::primary::PrimaryProgram {
                        selection: primary_mode.unwrap_or(raw[86]),
                        control: radias_synth_domain::controller_primary::PrimaryControl {
                            control1,
                            control2,
                            ..Default::default()
                        },
                    },
                )?;
                if filter2_route.is_some() {
                    edit_second_filter(renderer.controls(), &compiled, timbre, control1, control2)?;
                }
                if let Some((mode, position)) = shaper {
                    renderer
                        .controls()
                        .timbre_shaper(timbre, mode, position, control1)?;
                }
            }
            renderer.render_into(&mut []);
            if frames == 1007 {
                for (slot, state, filter2_state, shaper_state, comb) in &retained {
                    if renderer.active_voice(*slot).unwrap().renderer.pitch_state() != *state {
                        return Err("Mono control edit changed DSP targets before delivery".into());
                    }
                    if renderer
                        .active_voice(*slot)
                        .unwrap()
                        .renderer
                        .filter2_state()
                        != *filter2_state
                    {
                        return Err("Mono Filter2 edit changed DSP targets before delivery".into());
                    }
                    if renderer
                        .active_voice(*slot)
                        .unwrap()
                        .renderer
                        .shaper_state()
                        != *shaper_state
                    {
                        return Err("Mono depth edit changed shaper state before delivery".into());
                    }
                    if renderer.active_voice(*slot).unwrap().renderer.comb != *comb {
                        return Err(
                            "Mono Comb edit reset private delay memory before delivery".into()
                        );
                    }
                }
            }
            append(&mut renderer, &mut result, frames);
        }
        append(&mut renderer, &mut result, 8191);
        for timbre in 0..4 {
            input.midi(&[0x80 | timbre, next_note, 0])?;
        }
        append(&mut renderer, &mut result, 8193);
        for timbre in 0..4 {
            input.midi(&[0x80 | timbre, 60, 0])?;
        }
        append(&mut renderer, &mut result, 8192);
        if renderer.controls().status().held_voices != 0 || renderer.amplifier_delivery_state().1 {
            return Err("Mono final release lost the AMP queue".into());
        }
        renderer.controls().stop()?;
        append(&mut renderer, &mut result, 1024);
        if renderer.amplifier_delivery_state() != (0, false)
            || renderer.controls().status().active_voices != 0
        {
            return Err("Mono Stop retained actor or pending packet".into());
        }
        return Ok(result);
    }
    if pressure {
        let note_end = 60
            + notes_per_timbre.unwrap_or(if primary_mode.is_some() && !full_pressure {
                1
            } else {
                6
            });
        for timbre in 0..4 {
            renderer.controls().timbre(timbre, true, timbre)?;
        }
        for timbre in 0..4 {
            for note in 60..note_end {
                input.midi(&[0x90 | timbre, note, 100])?;
            }
        }
        append(&mut renderer, &mut result, 8192);
        if renderer.controls().status().held_voices != u32::from(note_end - 60) * 4
            || renderer.amplifier_delivery_state().1
        {
            return Err(format!("Transport pressure primary{primary_mode:?}/Filter2{filter2_route:?}/expected{} failed: {:?}",u32::from(note_end-60)*4,renderer.controls().status()).into());
        }
        for timbre in 0..4 {
            let count = (0..24)
                .filter_map(|slot| renderer.active_voice(slot))
                .filter(|v| v.held && v.timbre == timbre)
                .count();
            if count != usize::from(note_end - 60) {
                return Err("Pressure notes were layered on the wrong MIDI owner".into());
            }
        }
        for timbre in 0..4 {
            input.midi(&[0xe0 | timbre, 127, 127])?;
            renderer.controls().timbre_secondary(
                timbre,
                radias_synth_application::secondary::SecondaryProgram {
                    selection: 32,
                    pitch: radias_synth_domain::controller_secondary::SecondaryPitch {
                        semitone: 76,
                        ..Default::default()
                    },
                },
            )?;
            if let Some((mode, position)) = shaper {
                renderer
                    .controls()
                    .timbre_shaper(timbre, mode, position, 35 + 11 * timbre)?;
            }
            let selection = primary_mode.unwrap_or(raw[86]);
            if filter2_route.is_some() {
                edit_second_filter(
                    renderer.controls(),
                    &compiled,
                    timbre,
                    37 + 11 * timbre,
                    21 + 15 * timbre,
                )?;
            }
            renderer.controls().timbre_primary_control(
                timbre,
                radias_synth_application::primary::PrimaryProgram {
                    selection,
                    control: radias_synth_domain::controller_primary::PrimaryControl {
                        control1: 110,
                        control2: 35,
                        ..Default::default()
                    },
                },
            )?;
            renderer.controls().timbre_mixer(
                timbre,
                radias_synth_application::mixer::MixerProgram {
                    selections: [selection, 32],
                    levels: [90, 40, 20],
                    manual_offsets: [0; 3],
                },
            )?;
            renderer.controls().timbre_pan(
                timbre,
                radias_synth_domain::controller_pan::PanControl {
                    position: 20 + 25 * timbre,
                    ..Default::default()
                },
            )?;
        }
        append(&mut renderer, &mut result, 1007);
        for timbre in 0..4 {
            if let Some((mode, position)) = shaper {
                renderer
                    .controls()
                    .timbre_shaper(timbre, mode, position, 100 - 9 * timbre)?;
            }
            if filter2_route.is_some() {
                edit_second_filter(
                    renderer.controls(),
                    &compiled,
                    timbre,
                    100 - 9 * timbre,
                    96 - 8 * timbre,
                )?;
            }
            renderer.controls().timbre_primary_control(
                timbre,
                radias_synth_application::primary::PrimaryProgram {
                    selection: primary_mode.unwrap_or(raw[86]),
                    control: radias_synth_domain::controller_primary::PrimaryControl {
                        control1: 32,
                        control2: 115,
                        ..Default::default()
                    },
                },
            )?;
            renderer.controls().timbre_pan(
                timbre,
                radias_synth_domain::controller_pan::PanControl {
                    position: 110 - 25 * timbre,
                    ..Default::default()
                },
            )?;
        }
        append(&mut renderer, &mut result, 1009);
        for timbre in 0..4 {
            for note in 60..note_end {
                input.midi(&[0x80 | timbre, note, 0])?;
            }
        }
        append(&mut renderer, &mut result, 8192);
        if renderer.controls().status().held_voices != 0 || renderer.amplifier_delivery_state().1 {
            return Err("24-voice release transport failed".into());
        }
        renderer.controls().stop()?;
        append(&mut renderer, &mut result, 1024);
        if renderer.amplifier_delivery_state() != (0, false)
            || renderer.controls().status().active_voices != 0
        {
            return Err("Pressure Stop retained transport/actor".into());
        }
        return Ok(result);
    }
    for note in [60, 64, 67] {
        input.midi(&[0x90, note, 100])?;
    }
    append(&mut renderer, &mut result, 48);
    // The original controlled source accepts the first note at frame48.
    // Use nonaligned boundaries throughout: state must not jump to frame128.
    for note in [60, 64, 67] {
        input.midi(&[0x80, note, 0])?;
    }
    renderer.render_into(&mut []);
    if renderer.controls().status().held_voices != 0 {
        return Err("Frame48 release was not serviced without advancing audio".into());
    }
    for note in [60, 64, 67] {
        input.midi(&[0x90, note, 100])?;
    }
    append(&mut renderer, &mut result, 8197);
    if renderer.controls().status().held_voices != 3 {
        return Err("Recording missed ordinary Poly note factory".into());
    }
    for note in [60, 64, 67] {
        input.midi(&[0x80, note, 0])?;
    }
    append(&mut renderer, &mut result, 8183);
    if renderer.controls().status().held_voices != 0 {
        return Err("Recording lost ordinary note release".into());
    }
    raw[24] = 32;
    let owner_timbre: [u8; 228] = raw[48..276].try_into().unwrap();
    raw[276..504].copy_from_slice(&owner_timbre);
    let source = rdl::drum_kits(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut kit = *source[0].bytes();
    for i in 0..16 {
        kit[18 + i] = 0;
        kit[36 + i] = 36 + i as u8;
        kit[52 + 104 * i..156 + 104 * i].copy_from_slice(&raw[64..168]);
        kit[52 + 104 * i + 0x13] = 64 + i as u8;
        kit[52 + 104 * i + 0x16] = i as u8 % 4;
    }
    kit[52 + 104 * 3 + 0x2d] = 0;
    let program = Program::from_bytes(&raw).unwrap();
    let drums = compile_drum_kit(
        &program,
        radias_synth_domain::drum::DrumKit::from_bytes(&kit).unwrap(),
        &map,
        &mix,
        base,
    )?;
    let compiled = compile_program(&program, 0, &map, &mix, base)?;
    let (compiled, drums) = if attach_templates {
        (
            compiled
                .with_parameter_templates(&program, &templates)
                .map_err(|e| format!("{e:?}"))?,
            drums
                .with_parameter_templates(&templates)
                .map_err(|e| format!("{e:?}"))?,
        )
    } else {
        (compiled, drums)
    };
    renderer
        .controls()
        .load_program_with_drums(compiled, Some(drums))?;
    renderer.controls().timbre(1, true, 1)?;
    renderer.controls().drum_pad(0, 100)?;
    renderer.controls().drum_pad(2, 100)?;
    input.midi(&[0x91, 65, 100])?;
    append(&mut renderer, &mut result, 100);
    if silent_event {
        renderer.controls().drum_pad(3, 100)?;
    }
    append(&mut renderer, &mut result, 7);
    if silent_event {
        renderer.controls().drum_pad(3, 0)?;
    }
    append(&mut renderer, &mut result, 8072);
    if renderer.controls().status().held_voices != 3 {
        return Err(format!(
            "Mixed drum/ordinary event routing differs: {:?}, pitches {:?}",
            renderer.controls().status(),
            renderer.controls().actor_pitch_codes()
        )
        .into());
    }
    // Recompile the complete body; no original coefficient/target trace is used.
    let mut body: [u8; 104] = kit[52..156].try_into().unwrap();
    body[0x2d] = 42;
    body[0x31] = 0;
    body[0x13] = 76;
    let mut changed_kit = radias_synth_domain::drum::DrumKit::from_bytes(&kit).unwrap();
    changed_kit.replace_instrument(0, &body).unwrap();
    let changed = compile_drum_kit(&program, changed_kit, &map, &mix, base)?;
    let changed = if attach_templates {
        changed
            .with_parameter_templates(&templates)
            .map_err(|e| format!("{e:?}"))?
    } else {
        changed
    }
    .instruments[0];
    renderer.controls().edit_drum_instrument(0, changed)?;
    input.midi(&[0xe1, 127, 100])?;
    append(&mut renderer, &mut result, 8213);
    if renderer.controls().status().held_voices != 3
        || !renderer
            .controls()
            .actor_pitch_codes()
            .contains(&(72 * 256))
        || !renderer
            .controls()
            .actor_pitch_codes()
            .contains(&(62 * 256))
    {
        return Err("Production recording lost independent live body/pitch".into());
    }
    renderer.controls().drum_pad(0, 0)?;
    renderer.controls().drum_pad(2, 0)?;
    input.midi(&[0x81, 65, 0])?;
    append(&mut renderer, &mut result, 8189);
    if renderer.controls().status().held_voices != 0 {
        return Err("Mixed release retained held actor".into());
    }
    renderer.controls().stop()?;
    append(&mut renderer, &mut result, 4096);
    if renderer.controls().status().active_voices != 0 {
        return Err("Production Stop retained active actors".into());
    }
    if renderer.amplifier_delivery_state() != (0, false) {
        return Err("Recording retained or overflowed AMP delivery".into());
    }
    Ok(result)
}

#[cfg(feature = "desktop-io")]
fn edit_second_filter(
    player: &radias_synth_infrastructure::audio::NativePlayer,
    compiled: &radias_synth_application::stored_program::CompiledProgram,
    timbre: u8,
    cutoff: u8,
    resonance: u8,
) -> Result<(), String> {
    let t = compiled.timbres[timbre as usize];
    if let Some(mut filter) = t.dynamic_filter2 {
        filter.controls.cutoff.cutoff = cutoff;
        filter.controls.cutoff.linked_cutoff = cutoff;
        filter.controls.resonance.resonance = resonance;
        filter.controls.resonance.linked_resonance = resonance;
        player.timbre_filter_routing(timbre, filter.route & 3, t.filter2)?;
        player.timbre_filter2_program(timbre, filter)?;
    } else if let Some(mut comb) = t.comb {
        comb.cutoff.cutoff = cutoff;
        comb.cutoff.linked_cutoff = cutoff;
        comb.resonance.resonance = resonance;
        comb.resonance.linked_resonance = resonance;
        player.timbre_comb(
            timbre,
            compiled.stored.timbres[timbre as usize]
                .controls
                .filter_route
                & 3,
            comb,
        )?;
    }
    Ok(())
}
