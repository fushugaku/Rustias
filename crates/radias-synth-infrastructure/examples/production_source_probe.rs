#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("Production source probe requires desktop-io");
}
#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::{
        amplifier::ControllerTables, modulation::VoiceModulationTables,
        portamento::PortamentoTables,
    };
    use radias_synth_domain::{
        Sample, control_slew::SlewWeights, pan::StereoFrame, program::Program,
    };
    use radias_synth_infrastructure::{
        audio::NativeOfflineRenderer,
        firmware::{self, MasterTables},
        prepared::{ControlMap, PreparedVoice},
        rdl,
        stored_program::{compile_drum_kit, compile_program},
        wav,
    };
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let shared_transport = std::env::args().nth(2).as_deref() == Some("--shared-transport");
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
    if shared_transport {
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
    let raw = fs::read(out.join("drum-common-center.program.bin"))?;
    // Accepted original boot INPUT words, not observed phases or DSP controls.
    // The native domain derives all physical slot phases/mixer seeds itself.
    let boot = ["", "slave-"].map(|chip| -> Result<_, Box<dyn std::error::Error>> {
        let input: serde_json::Value = serde_json::from_slice(&fs::read(out.join(format!(
            "live-drum-common-center-reference-{chip}noise-boot-inputs.json"
        )))?)?;
        Ok(radias_synth_domain::noise::NoiseFrameSeeds::from_inputs(
            input["input602"].as_u64().unwrap() as i16,
            input["input603"].as_u64().unwrap() as i16,
        ))
    });
    let [master_boot, slave_boot] = boot;
    renderer
        .controls()
        .configure_physical_frames([master_boot?, slave_boot?])?;
    let program = Program::from_bytes(&raw).unwrap();
    let source = rdl::drum_kits(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut kit = *source[0].bytes();
    kit[18..34].fill(0);
    kit[34..36].fill(255);
    for i in 0..16 {
        kit[36 + i] = if i == 0 { 36 } else { 127 };
        kit[52 + 104 * i..156 + 104 * i].copy_from_slice(&raw[64..168]);
        if i != 0 {
            kit[52 + 104 * i + 45] = 0;
        }
    }
    renderer.controls().load_program_with_drums(
        compile_program(&program, 0, &map, &mix, base)?,
        Some(compile_drum_kit(
            &program,
            radias_synth_domain::drum::DrumKit::from_bytes(&kit).unwrap(),
            &map,
            &mix,
            base,
        )?),
    )?;
    let input = renderer.controls().input();
    let mut samples = vec![[Sample(0); 8]; 8280];
    let mut states = Vec::new();
    for (frame, sample) in samples.iter_mut().enumerate() {
        if frame == 48 {
            input.midi(&[0x90, 36, 100])?;
        }
        if frame == 4196 {
            input.midi(&[0x80, 36, 0])?;
        }
        let mut stereo = [StereoFrame::default(); 1];
        renderer.render_into(&mut stereo);
        sample[0] = stereo[0].left;
        sample[1] = stereo[0].right;
        if [48, 49, 52, 53, 54, 72, 88, 92, 112, 4196, 4224, 8279].contains(&frame) {
            let voice = renderer.active_voice(0);
            states.push(
                serde_json::json!({"frame":frame,"pitch":renderer.controls().actor_pitch_codes(),
                "held":renderer.controls().status().held_voices,
                "amplifier":voice.and_then(|v|v.amplifier.as_ref()).map(|a|format!("{:?}",a.control())),
                "envelope":voice.and_then(|v|v.amplifier.as_ref()).map(|a|format!("{:?}",a.envelope)),
                "filter_coefficients":voice.map(|v|format!("{:?}",v.renderer.current_filter())),
                "mixer":voice.map(|v|format!("{:?}",v.renderer.current_mixer())),
                "envelope_rate":voice.map(|v|v.renderer.envelope_rate()),
                "amplifier_delivery":renderer.amplifier_delivery_state(),
                "controller_service":renderer.controller_service_state().map(|(timer,ticks)|serde_json::json!({"counter":timer.counter,"phase":timer.prescaler_phase,"interrupts":ticks})),
                "voice":voice.map(|v|format!("{:?}",v.renderer.voice)),
                "sample":[sample[0].0,sample[1].0]}),
            );
        }
    }
    wav::write_buses(
        &out.join(if shared_transport {
            "production-source-drum-center-native-shared.wav"
        } else {
            "production-source-drum-center-native.wav"
        }),
        &samples,
    )?;
    fs::write(
        out.join(if shared_transport {
            "production-source-drum-center-native-shared-state.json"
        } else {
            "production-source-drum-center-native-state.json"
        }),
        serde_json::to_vec_pretty(&states)?,
    )?;
    // Diagnostic comparison only; these original coefficients never enter
    // the production generator, its constructor or the note factory.
    let observed = PreparedVoice::from_reference_va_parameters(&fs::read(
        out.join("live-drum-common-center-reference-voice-va-inputs.bin"),
    )?)?;
    fs::write(
        out.join("production-source-drum-center-original-coefficients.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"filter":format!("{:?}",observed.parameters.filter),
            "mixer":format!("{:?}",observed.parameters.mix),"primary":format!("{:?}",observed.parameters.primary),
            "envelope_target":observed.parameters.envelope_target,"envelope_rate":observed.parameters.envelope_rate,
            "used_as_renderer_inputs":false}),
        )?,
    )?;
    println!(
        "Native full loaded-program factory recorded8280 frames from accepted note48 and release4196; comparison is unqualified until complete audio agrees"
    );
    Ok(())
}
