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
    let map = ControlMap::from_system(&sys)?;
    let mix = tables.filter_mix()?;
    let mut raw = fs::read(out.join("drum-common-center.program.bin"))?;
    raw[64 + 0x10] = 0; // Source drum allocation bypasses the owning ordinary Mono mode.
    let source = rdl::drum_kits(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut kit = *source[0].bytes();
    for i in 0..16 {
        kit[18 + i] = if i < 2 {
            1
        } else if i == 2 {
            2
        } else {
            0
        };
        kit[36 + i] = 36 + i as u8;
        kit[52 + 104 * i..52 + 104 * (i + 1)].copy_from_slice(&raw[64..168]);
        kit[52 + 104 * i + 0x13] = 64 + i as u8;
        kit[52 + 104 * i + 0x16] = i as u8 % 4;
        kit[52 + 104 * i + 0x37] = 30;
    }
    kit[36 + 9] = 44; // Two independent bodies share one trigger key.
    let load = |program_bytes: &[u8], kit_bytes: &[u8]| -> Result<(), Box<dyn std::error::Error>> {
        let program = Program::from_bytes(program_bytes).map_err(|_| "Invalid source program")?;
        let compiled = compile_program(&program, 0, &map, &mix, base)?;
        let drum = radias_synth_infrastructure::stored_program::compile_drum_kit(
            &program,
            radias_synth_domain::drum::DrumKit::from_bytes(kit_bytes)
                .map_err(|_| "Invalid native kit")?,
            &map,
            &mix,
            base,
        )?;
        player.load_program_with_drums(compiled, Some(drum))?;
        thread::sleep(Duration::from_millis(90));
        Ok(())
    };
    load(&raw, &kit)?;
    let input = player.input();
    let mut cases = Vec::new();
    for (note, velocity, held) in [
        (36, 100, 1),
        (38, 100, 2),
        (37, 100, 2),
        (36, 0, 2),
        (37, 0, 1),
        (38, 0, 0),
        (39, 100, 1),
        (39, 100, 2),
        (39, 0, 1),
        (39, 0, 0),
        (44, 100, 2),
        (44, 0, 0),
    ] {
        input.midi(&[0x90, note, velocity])?;
        thread::sleep(Duration::from_millis(90));
        let status = player.status();
        if status.held_voices != held {
            return Err(format!(
                "Native drum note{note}/{velocity}: held{} != {held}",
                status.held_voices
            )
            .into());
        }
        if velocity != 0 && status.output_peak <= 0.0 {
            return Err("Native drum note is silent".into());
        }
        cases.push(serde_json::json!({"note":note,"velocity":velocity,"held":held,"peak":status.output_peak,
            "actor_pitch_codes":player.actor_pitch_codes()}));
    }
    // Every direct pad addresses its own body and releases its retained key.
    // Two bodies deliberately share a MIDI trigger key in this fixture.
    let mut pad_cases = Vec::new();
    for instrument in 0..16u8 {
        load(&raw, &kit)?;
        player.drum_pad(instrument, 100)?;
        thread::sleep(Duration::from_millis(90));
        let status = player.status();
        let pitch = u32::from(60 + instrument) * 256;
        if status.held_voices != 1
            || status.output_peak <= 0.0
            || !player.actor_pitch_codes().contains(&pitch)
        {
            return Err(
                format!("Direct pad{instrument} did not sound its independent body").into(),
            );
        }
        player.drum_pad(instrument, 0)?;
        thread::sleep(Duration::from_millis(90));
        if player.status().held_voices != 0 {
            return Err(format!("Direct pad{instrument} lost its note-off").into());
        }
        pad_cases.push(
            serde_json::json!({"instrument":instrument,"held_on":1,"held_off":0,
            "pitch_code":pitch,"peak":status.output_peak}),
        );
    }
    // Direct pads address one body, even when two instruments share its key.
    load(&raw, &kit)?;
    player.drum_pad(8, 100)?;
    thread::sleep(Duration::from_millis(90));
    if player.status().held_voices != 1 {
        return Err("Direct pad dispatched duplicate trigger keys".into());
    }
    player.drum_pad(8, 0)?;
    thread::sleep(Duration::from_millis(90));
    if player.status().held_voices != 0 {
        return Err("Direct pad did not release retained key".into());
    }
    load(&raw, &kit)?;
    player.drum_pad(0, 100)?;
    player.drum_pad(2, 100)?;
    thread::sleep(Duration::from_millis(100));
    if player.status().held_voices != 2 {
        return Err("Direct pads did not allocate independently".into());
    }
    let edit = |index: u8, body: &[u8; 104]| -> Result<(), Box<dyn std::error::Error>> {
        let program = Program::from_bytes(&raw).unwrap();
        let mut changed = radias_synth_domain::drum::DrumKit::from_bytes(&kit).unwrap();
        changed.replace_instrument(index as usize, body).unwrap();
        let compiled = radias_synth_infrastructure::stored_program::compile_drum_kit(
            &program, changed, &map, &mix, base,
        )?;
        player.edit_drum_instrument(index, compiled.instruments[index as usize])?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    };
    let mut body0: [u8; 104] = kit[52..156].try_into().unwrap();
    body0[0x2d] = 0;
    edit(0, &body0)?;
    if player.status().held_voices != 2 || player.status().output_peak <= 0.0 {
        return Err("Instrument edit restarted voices or muted unrelated body".into());
    }
    let mut body2: [u8; 104] = kit[52 + 208..156 + 208].try_into().unwrap();
    body2[0x2d] = 0;
    edit(2, &body2)?;
    if player.status().output_peak != 0.0 || player.status().held_voices != 2 {
        return Err("Two body level edits did not mute held voices".into());
    }
    body0[0x2d] = 100;
    body0[0x13] = 76;
    body0[0x31] = 0;
    edit(0, &body0)?;
    if !player.actor_pitch_codes().contains(&(72 * 256))
        || !player.actor_pitch_codes().contains(&(62 * 256))
        || player.status().held_voices != 2
        || player.status().output_peak <= 0.0
    {
        return Err("Selected body pitch/gain edit affected another instrument".into());
    }
    player.drum_pad(0, 0)?;
    player.drum_pad(2, 0)?;
    thread::sleep(Duration::from_millis(100));
    if player.status().held_voices != 0 {
        return Err("Body edit lost direct-pad release identity".into());
    }
    load(&raw, &kit)?;
    player.drum_pad(0, 100)?;
    input.midi(&[0x90, 37, 100])?;
    thread::sleep(Duration::from_millis(100));
    if player.status().held_voices != 2 {
        return Err("Pad and external MIDI tags were incorrectly merged".into());
    }
    player.drum_pad(0, 0)?;
    input.midi(&[0x90, 37, 0])?;
    thread::sleep(Duration::from_millis(100));
    // Independent body pitch uses60+instrument transpose, not the trigger key.
    load(&raw, &kit)?;
    input.midi(&[0x90, 40, 100])?;
    thread::sleep(Duration::from_millis(90));
    let pitch = player.actor_pitch_codes();
    if !pitch.contains(&(64 * 256)) {
        return Err("Drum body pitch did not retain its own transpose".into());
    }
    input.midi(&[0xe0, 127, 127])?;
    thread::sleep(Duration::from_millis(90));
    if !player.actor_pitch_codes().contains(&(64 * 256)) {
        return Err("Global bend incorrectly moved fixed drum pitch".into());
    }
    raw[27] = 65;
    load(&raw, &kit)?;
    input.midi(&[0x90, 36, 100])?;
    thread::sleep(Duration::from_millis(90));
    if player.status().held_voices != 0 {
        return Err("Old key bypassed drum trigger transpose".into());
    }
    input.midi(&[0x90, 37, 100])?;
    thread::sleep(Duration::from_millis(90));
    if player.status().held_voices != 1 || !player.actor_pitch_codes().contains(&(60 * 256)) {
        return Err("Trigger transpose altered body pitch or routing".into());
    }
    kit[52 + 104 * 15 + 0x16] = 6;
    load(&raw, &kit)?;
    let missing = player.unsupported_drum_notes();
    input.midi(&[0x90, 52, 100])?;
    thread::sleep(Duration::from_millis(90));
    if player.unsupported_drum_notes() != missing + 1 || player.status().held_voices != 0 {
        return Err("PCM drum was silently replaced by VA".into());
    }
    input.midi(&[0xb0, 123, 0])?;
    thread::sleep(Duration::from_millis(300));
    let status = player.status();
    let passed = status.deadline_misses == 0 && status.audible_frames > 0;
    let report = serde_json::json!({"passed":passed,"device":status.device,"sample_rate":status.sample_rate,
        "deadline_misses":status.deadline_misses,"worst_callback_ms":status.worst_render_ns as f64/1e6,"audible_frames":status.audible_frames,
        "cases":cases,"pad_cases":pad_cases,"all16_direct_pad_bodies_audible_and_released":true,
        "direct_pad_retained_release_and_duplicate_key_isolation":true,"live_body_edits_preserve_held_voices_and_other_instruments":true,"pad_and_external_MIDI_tags_remain_distinct":true,"exclusive_choke_and_repeated_oldest_release_and_duplicate_key_qualified":true,
        "owning_Mono_mode_does_not_limit_drum_allocation":true,"instrument_pitch_and_trigger_transpose_separate":true,
        "unsupported_PCM_no_VA_substitution":true,"original_complete_audio_parity_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("drum-device.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native drum realtime callback missed its deadline".into());
    }
    Ok(())
}
