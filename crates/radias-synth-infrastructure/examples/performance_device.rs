#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("Performance device gate requires desktop-io");
}
#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::{
        amplifier::ControllerTables, modulation::VoiceModulationTables,
    };
    use radias_synth_domain::{
        control_slew::SlewWeights, performance::GlobalPerformance, program::Program,
    };
    use radias_synth_infrastructure::{
        audio::NativePlayer,
        firmware::{self, MasterTables},
        prepared::{ControlMap, PreparedVoice},
        stored_program::compile_program,
    };
    use std::{fs, path::PathBuf, thread, time::Duration};
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&master)?;
    let plans = ["saw", "pulse", "triangle", "sine"]
        .into_iter()
        .map(|name| {
            PreparedVoice::from_program_json(
                &fs::read(root.join(format!("assets/native-va/{name}.json"))).unwrap(),
            )
            .unwrap()
        })
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
    player.gain(0.05);
    player.configure_performance(GlobalPerformance::default())?;
    player.configure_mixer(firmware::mixer_scales(&sys)?)?;
    player.configure_secondary(firmware::fine_tune_table(&sys)?)?;
    player.configure_noise(
        tables.pitch()?,
        tables.noise_pitch()?,
        firmware::formant_counter_seeds(&sys)?,
    )?;
    player.configure_voice_groups(firmware::voice_group_tables(&sys)?)?;
    player.controller_filter_tables(firmware::controller_filter_tables(&sys)?)?;
    player.configure_filter2(firmware::filter2_control_tables(&sys)?)?;
    player.configure_comb(firmware::comb_control_tables(&sys)?)?;
    player.configure_pan(
        firmware::pan_tables(&sys)?,
        SlewWeights {
            target: tables.word(0x4026)? as i16,
            memory: tables.word(0x4027)? as i16,
        },
    )?;
    let map = ControlMap::from_json(&fs::read(
        root.join("assets/native-va/filter-controls.json"),
    )?)?;
    let mix = tables.filter_mix()?;
    let mut raw = fs::read(out.join("stored-four-timbres.program.bin"))?;
    for i in 0..4 {
        let b = 48 + 228 * i;
        raw[b] = 128;
        raw[b + 4] = [0, 1, 0, 2][i];
        raw[b + 5] = if i == 2 { 159 } else { 255 };
        raw[b + 6] = 0;
        raw[b + 7] = 127;
        raw[b + 8] = 0;
        raw[b + 32] = 128;
        raw[b + 38] = i as u8;
        raw[b + 45] = 0;
        raw[b + 61] = 100;
        raw[b + 66] = 64;
        raw[b + 76..b + 84].copy_from_slice(&[0, 0, 127, 10, 1, 127, 64, 64]);
    }
    let compile = |raw: &[u8]| -> Result<_, Box<dyn std::error::Error>> {
        let program = Program::from_bytes(raw).map_err(|_| "Invalid controlled program")?;
        Ok(compile_program(&program, 0, &map, &mix, base)?)
    };
    player.load_program(compile(&raw)?)?;
    let input = player.input();
    for channel in [0, 1, 2] {
        input.midi(&[0x90 | channel, 60, 100])?;
    }
    thread::sleep(Duration::from_millis(180));
    if player.status().held_voices != 4 || player.status().output_peak < 1e-8 {
        return Err("Four stored performance timbres are not audible".into());
    }
    let mut cases = Vec::new();
    for (channel, value, expected) in [
        (0, 0, [0, 32512, 32512, 32512]),
        (1, 64, [0, 16384, 32512, 32512]),
        (3, 0, [0, 16384, 32512, 32512]),
        (2, 32, [0, 16384, 32512, 8192]),
        (0, 127, [32512, 16384, 32512, 8192]),
    ] {
        input.midi(&[0xb0 | channel, 11, value])?;
        thread::sleep(Duration::from_millis(120));
        let gain = player.source_gains();
        let status = player.status();
        if gain != expected || status.held_voices != 4 || status.output_peak < 1e-8 {
            return Err(format!("Expression route{channel}/{value}: {gain:?}, {status:?}").into());
        }
        cases.push(serde_json::json!({"channel":channel,"value":value,"gains":gain,"held_voices":status.held_voices,"output_peak":status.output_peak}));
    }
    for i in 0..4 {
        let b = 48 + 228 * i;
        raw[b + 4] = 0;
        raw[b + 5] = 255;
    }
    input.midi(&[0xb0, 11, 0])?;
    player.load_program(compile(&raw)?)?;
    thread::sleep(Duration::from_millis(80));
    input.midi(&[0x90, 60, 100])?;
    thread::sleep(Duration::from_millis(250));
    let muted = player.status();
    if player.source_gains() != [0; 4] || muted.held_voices != 4 || muted.output_peak > 1e-7 {
        return Err(format!("Stored reload did not preserve Expression mute:{muted:?}").into());
    }
    input.midi(&[0xb0, 11, 127])?;
    thread::sleep(Duration::from_millis(150));
    if player.source_gains() != [32512; 4]
        || player.status().output_peak < 1e-8
        || player.status().held_voices != 4
    {
        return Err("Expression restore lost held notes or output".into());
    }
    input.midi(&[0xb5, 11, 25])?;
    for i in 0..4 {
        raw[48 + 228 * i + 4] = 5;
    }
    player.load_program(compile(&raw)?)?;
    thread::sleep(Duration::from_millis(80));
    if player.source_gains() != [6400; 4] {
        return Err("Expression received before program load was lost".into());
    }
    for i in 0..4 {
        let b = 48 + 228 * i;
        raw[b + 4] = 0;
        raw[b + 5] = 64;
    }
    input.midi(&[0xb0, 11, 0])?;
    player.load_program(compile(&raw)?)?;
    input.midi(&[0x90, 60, 100])?;
    thread::sleep(Duration::from_millis(200));
    if player.source_gains() != [0; 4] {
        return Err("Global mode0 did not use receive bit40".into());
    }
    player.configure_performance(GlobalPerformance {
        channel: 0,
        amplitude_receive_mode: 1,
    })?;
    thread::sleep(Duration::from_millis(150));
    if player.source_gains() != [32512; 4]
        || player.status().output_peak < 1e-8
        || player.status().held_voices != 4
    {
        return Err("Global mode1 did not select receive bit20 on held notes".into());
    }
    player.stop()?;
    thread::sleep(Duration::from_millis(50));
    let status = player.status();
    let passed = !status.failed
        && status.deadline_misses == 0
        && status.audible_frames > 0
        && status.active_voices == 0;
    let report = serde_json::json!({"passed":passed,"cases":cases,"Expression_receive_flags_and_all_channel_routes":true,
        "four_timbres_held_during_gain_changes":true,"unrouted_channel_preserves_gain":true,
        "program_reload_and_pre_received_Expression_preserved":true,"Global_mode_live_receive_mask_switch":true,
        "mute_output_peak":muted.output_peak,"sample_rate":status.sample_rate,"device":status.device,
        "deadline_misses":status.deadline_misses,"worst_callback_ms":status.worst_render_ns as f64/1e6,
        "audible_frames":status.audible_frames,"CPU_emulation_in_renderer":false,"complete_native_engine":false});
    fs::write(
        out.join("performance-device.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native performance device gate failed".into());
    }
    Ok(())
}
