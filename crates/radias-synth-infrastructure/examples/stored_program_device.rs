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
    player.gain(0.05);
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
    let map = ControlMap::from_json(&fs::read(
        root.join("assets/native-va/filter-controls.json"),
    )?)?;
    let mix = tables.filter_mix()?;
    let raw = fs::read(out.join("stored-four-timbres.program.bin"))?;
    let program = Program::from_bytes(&raw).map_err(|_| "Invalid stored fixture")?;
    let compiled = compile_program(&program, 0, &map, &mix, base)?;
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
    let unavailable = compile_program(&bank[0], 0, &map, &mix, base)?;
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
    let status = player.status();
    let passed = !status.failed && status.deadline_misses == 0 && status.audible_frames > 1000;
    let report = serde_json::json!({"passed":passed,"sample_rate":status.sample_rate,"device":status.device,
        "audible_frames":status.audible_frames,"deadline_misses":status.deadline_misses,"worst_callback_ms":status.worst_render_ns as f64/1e6,
        "stored_four_timbres_loaded_atomically":true,"stored_key_windows_cases":cases,"failed_generator_preserves_current_sound":true,
        "CPU_emulation_in_renderer":false,"full_bank_audio_parity_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("stored-program-device.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Stored program realtime device gate failed".into());
    }
    Ok(())
}
