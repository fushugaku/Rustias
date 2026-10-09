#[cfg(not(feature = "desktop-io"))]
fn main() {
    panic!("Production note memory gate requires desktop-io");
}
#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::{
        amplifier::ControllerTables, modulation::VoiceModulationTables,
        portamento::PortamentoTables,
    };
    use radias_synth_domain::{control_slew::SlewWeights, pan::StereoFrame, program::Program};
    use radias_synth_infrastructure::{
        audio::NativeOfflineRenderer,
        firmware::{self, MasterTables},
        prepared::{ControlMap, PreparedVoice},
        stored_program::compile_program,
    };
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&master)?;
    let mut plans = ["saw", "pulse", "triangle", "sine"]
        .map(|name| {
            PreparedVoice::from_program_json(
                &fs::read(root.join(format!("assets/native-va/{name}.json"))).unwrap(),
            )
            .unwrap()
        })
        .into_iter()
        .collect::<Vec<_>>();
    let base = plans[0].parameters.filter;
    // A captured bootstrap may contain an active filter history. The fresh
    // parameter-block contract must clear it independently of that template.
    for plan in &mut plans {
        plan.initial.filter.state.first = 0x12345678;
        plan.initial.filter.state.second = -42424242;
        plan.initial.filter.state.post = [0x76543210, -12345678];
    }
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
    let mut cases = Vec::new();
    for (name, mode) in [("Poly", 128), ("Mono Single", 0), ("Mono Multi", 64)] {
        raw[64 + 0x10] = mode;
        let program = Program::from_bytes(&raw).unwrap();
        renderer
            .controls()
            .load_program(compile_program(&program, 0, &map, &mix, base)?)?;
        let input = renderer.controls().input();
        input.midi(&[0x90, 60, 100])?;
        renderer.render_into(&mut []);
        let first = renderer.active_voice(0).ok_or("First actor absent")?;
        if first.renderer.voice.primary.phase.0 != 0
            || first.renderer.voice.primary.modulated_phase.0 != 0xc000_0000
        {
            return Err(format!(
                "{name} fresh Sine parameter phases differ from original initialization"
            )
            .into());
        }
        if first.renderer.voice.filter.state != Default::default()
            || first.renderer.voice.envelope.0 != 0
        {
            return Err(format!("{name} fresh DSP memory inherited dirty template").into());
        }
        renderer.render_into(&mut [StereoFrame::default(); 129]);
        let first = renderer.active_voice(0).unwrap();
        let memory = (
            first.renderer.voice.filter.state,
            first.renderer.voice.envelope,
        );
        let primary = first.renderer.voice.primary.phase;
        if memory.0 == Default::default() || memory.1.0 == 0 {
            return Err("Test voice did not build audible filter/envelope history".into());
        }
        input.midi(&[0x90, 64, 100])?;
        renderer.render_into(&mut []);
        let kept = renderer.active_voice(0).unwrap();
        if (
            kept.renderer.voice.filter.state,
            kept.renderer.voice.envelope,
        ) != memory
        {
            return Err(format!("{name} second event reset the existing DSP memory").into());
        }
        if kept.renderer.voice.primary.phase != primary {
            return Err("Zero-frame event advanced physical phase".into());
        }
        if mode == 128 {
            let second = renderer.active_voice(1).ok_or("Poly second actor absent")?;
            if second.renderer.voice.filter.state != Default::default()
                || second.renderer.voice.envelope.0 != 0
            {
                return Err(
                    "Poly second actor inherited another actor's private DSP memory".into(),
                );
            }
        }
        cases.push(
            serde_json::json!({"mode":name,"fresh_parameter_memory_zero":true,
            "second_note_existing_memory_retained":true,"phase_not_advanced_by_event":true}),
        );
    }
    let report = serde_json::json!({"passed":true,"cases":cases,
        "production_command_bus_and_factories_exercised":true,
        "dirty_bootstrap_first_second_and_post_filter_memory_cleared":true,
        "original_D534_fresh_resets_and_no_held_Mono_reset_observed":true,
        "original_live_audio_or_HPI_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("production-note-memory-verification.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
