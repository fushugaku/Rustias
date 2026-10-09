//! Complete stored EG2 fields compiled through the production application.
//! Other DSP controls/actors and controller delivery clocks are source fixtures.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::{AmplifierController, ControllerTables},
    program::TimbreControls,
};
use radias_synth_domain::{Sample, pan::StereoFrame, program::Program};
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::PreparedVoice,
    wav,
};
use serde_json::Value;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = ControllerTables {
        curves: firmware::envelope_curves(&system)?,
        timing: firmware::envelope_timing_tables(&system)?,
        amplifier: firmware::amplifier_tables(&system)?,
    };
    let master_image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let waveform = MasterTables::from_host_stream(&master_image)?.waveform()?;
    let labels = args.collect::<Vec<_>>();
    let native_rates = labels.iter().any(|s| s == "--native-rates");
    let rate_tables = firmware::amplifier_rate_table(&system)?;
    for label in labels.into_iter().filter(|s| s != "--native-rates") {
        let (source_name, label) = label
            .split_once(':')
            .map_or((None, label.as_str()), |(name, label)| (Some(name), label));
        let name = if let Some(name) = source_name {
            name.to_owned()
        } else if label.starts_with("envelope-curve-") {
            format!("live-{label}")
        } else {
            format!("live-{label}-reference")
        };
        let stored = Program::from_bytes(&fs::read(out.join(format!("{label}.program.bin")))?)
            .map_err(|_| "Original program is truncated")?;
        let controls = TimbreControls::from_timbre(stored.timbre(0).unwrap())
            .map_err(|_| "Invalid original patch route")?;
        // These declared shared instrument inputs match the controlled source
        // scene. They are not compiled voice/target observations.
        let mut program = controls.amplifier(0x7f00, None, 0);
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let mut events: Vec<Value> =
            fs::read_to_string(out.join(format!("{name}-native-envelope-events.jsonl")))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        if native_rates {
            events.extend(
                fs::read_to_string(out.join(format!("{name}-amplifier-packet-events.jsonl")))?
                    .lines()
                    .map(serde_json::from_str::<Value>)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        let key_path = out.join(format!("{name}-native-amplifier-key-events.jsonl"));
        let native_key = label.starts_with("amp-key-");
        let native_expression = label.starts_with("amp-expression-");
        let native_common = label.starts_with("drum-common-");
        let common = radias_synth_domain::program_binding::ProgramCommon {
            level: stored.bytes()[25],
            pan: stored.bytes()[26],
        };
        let pan_tables = firmware::pan_tables(&system)?;
        let mut common_application = 0u8;
        if native_common {
            for suffix in ["native-pan-events.jsonl", "program-binding-events.jsonl"] {
                events.extend(
                    fs::read_to_string(out.join(format!("{name}-{suffix}")))?
                        .lines()
                        .map(serde_json::from_str::<Value>)
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        let global = radias_synth_infrastructure::rdl::global_performance(&fs::read(
            root.join("firmware/Radias-backup.rdl"),
        )?)?;
        let channel = stored.timbre(0).unwrap().channel(global.channel);
        let receive_flags = stored.timbre(0).unwrap().bytes()[5];
        let mut expression = radias_synth_domain::performance::ExpressionState::default();
        let mut cached_expression = expression;
        if native_expression {
            let context: Value = serde_json::from_slice(&fs::read(
                out.join(format!("{name}-performance-context.json")),
            )?)?;
            if context["amplitude_receive_mode"].as_u64()
                != Some(global.amplitude_receive_mode as u64)
                || context["global_channel"].as_u64() != Some(global.channel as u64)
            {
                return Err("Stored Global context differs from Source runtime".into());
            }
            program.source_gain = expression.gain(channel, receive_flags, global);
            events.extend(
                fs::read_to_string(out.join(format!("{name}-native-performance-events.jsonl")))?
                    .lines()
                    .map(serde_json::from_str::<Value>)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        if native_key {
            events.extend(
                fs::read_to_string(key_path)?
                    .lines()
                    .map(serde_json::from_str::<Value>)
                    .collect::<Result<Vec<_>, _>>()?,
            );
            events.sort_by_key(|e| (e["frame"].as_u64().unwrap(), e["order"].as_u64().unwrap()));
        }
        let original = fs::read(out.join(format!("{name}-original-complete-mix.wav")))?;
        if original.len() < 44 || &original[36..40] != b"data" {
            return Err("Source WAV format differs".into());
        }
        let mut output = vec![[Sample(0); 8]; (original.len() - 44) / 32];
        let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
        let mut amplifier_delivery =
            radias_synth_domain::amplifier_delivery::AmplifierDelivery::default();
        let mut pending_rate = 0u16;
        let mut rate_requests = 0;
        let mut rate_commits = 0;
        if native_rates {
            renderer.set_envelope_rate(0);
        }
        if native_common {
            let raw_word = |i: usize| u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap());
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
        let mut inactive = radias_synth_domain::stereo_cache::StereoCache::default();
        let mut controller = None::<AmplifierController>;
        let mut event_index = 0;
        let mut target = 0;
        let mut errors = 0;
        let mut services = 0;
        let mut commits = 0;
        let mut compiled_controls = 0;
        let mut key_modulation = 0i16;
        let mut key_compilations = 0;
        let mut expression_events = 0;
        let mut expression_cache_events = 0;
        let mut binding_events = 0;
        let mut pan_compilations = 0;
        let mut pan_commits = 0;
        let mut pan_target = 0u16;
        // Observe the controller boundary immediately after the last recorded
        // audio frame as well. Its assertions add no frame to either WAV.
        let mut closing_boundary = [Sample(0); 8];
        for (frame, sample) in output
            .iter_mut()
            .chain(core::iter::once(&mut closing_boundary))
            .enumerate()
        {
            while let Some(event) = events.get(event_index) {
                if event["frame"].as_u64().ok_or("Source clock missing")? > frame as u64 {
                    break;
                }
                let kind = event["kind"].as_str().ok_or("Source event kind missing")?;
                if let Some(input) = event["parameters"].as_array() {
                    if input.len() != 9 {
                        return Err("Source envelope inputs truncated".into());
                    }
                    let p = |i: usize| input[i].as_u64().unwrap() as u8;
                    // Accepted note/velocity inputs include the original
                    // pre-assignment note0, followed by its assigned MIDI note.
                    let parameters = program.parameters(p(8), p(7));
                    let actual = [
                        parameters.attack,
                        parameters.decay,
                        parameters.sustain,
                        parameters.release,
                        parameters.curve & 7,
                        parameters.velocity_sensitivity,
                        parameters.key_tracking,
                        parameters.velocity,
                        parameters.note,
                    ];
                    if actual != core::array::from_fn::<_, 9, _>(p) {
                        return Err(format!(
                            "{name}: stored EG2 fields differ from original controller inputs"
                        )
                        .into());
                    }
                    if kind == "note_on" {
                        if event["initial_phase"].as_u64() != Some(0) {
                            return Err("Unqualified note initialization phase".into());
                        }
                        controller = Some(AmplifierController::from_program(
                            program,
                            parameters.note,
                            parameters.velocity,
                            &tables,
                        ));
                    } else {
                        controller
                            .as_mut()
                            .ok_or("Controller has not started")?
                            .parameters = parameters;
                    }
                }
                match kind {
                    "packet_request" => {
                        let action = event["action"]
                            .as_u64()
                            .ok_or("Original AMP action absent")?;
                        let update = match action {
                            0 | 1 => amplifier_delivery.binding(
                                &rate_tables,
                                event["attack"].as_u64().unwrap() as u8,
                                event["attack_modulation"].as_u64().unwrap() as i16,
                                target,
                                action == 1,
                            ),
                            2 => {
                                if event["expected_mode_before"].as_u64()
                                    != Some(u64::from(amplifier_delivery.mode))
                                {
                                    errors += 1;
                                }
                                amplifier_delivery.service(&rate_tables, target)
                            }
                            3 => radias_synth_domain::amplifier_delivery::AmplifierDelivery::reset(
                                &rate_tables,
                            ),
                            _ => return Err("Unknown original AMP packet action".into()),
                        };
                        if let radias_synth_domain::amplifier_delivery::AmplifierPacket::RateAndTarget {rate,..}=update {pending_rate=rate;}
                        rate_requests += 1;
                    }
                    "rate_commit" => {
                        let rate = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            pending_rate
                        };
                        if event["expected_rate"].as_u64() != Some(u64::from(rate)) {
                            errors += 1;
                        }
                        renderer.set_envelope_rate(rate as i16);
                        rate_commits += 1;
                    }
                    "program_binding" => {
                        common_application = u8::from(
                            event["alternate"]
                                .as_bool()
                                .ok_or("Source binding kind missing")?,
                        );
                        if event["expected_common_flag"].as_u64() != Some(common_application as u64)
                        {
                            return Err("Native actor binding flag differs".into());
                        }
                        program.midi_volume = common.level_for(common_application);
                        if let Some(ctrl) = &mut controller {
                            ctrl.program_common(program.midi_volume, &tables);
                        }
                        binding_events += 1;
                    }
                    "pan_target" => {
                        let input = event["control"]
                            .as_array()
                            .filter(|r| r.len() == 6)
                            .ok_or("Pan context missing")?;
                        let p = |i: usize| input[i].as_u64().unwrap() as u32;
                        if p(0) != controls.pan as u32
                            || p(4) != common_application as u32
                            || p(5) != common.pan as u32
                        {
                            return Err("Stored common pan binding differs".into());
                        }
                        let pan = radias_synth_domain::controller_pan::PanControl {
                            position: controls.pan,
                            manual_offset: p(1) as i16,
                            modulation: p(2) as i16,
                            timbre_offset: p(3) as i8,
                            midi_pan: common.pan_for(common_application),
                        };
                        if event["expected_target"].as_u64() != Some(pan.target() as u64) {
                            return Err("Native common pan target differs".into());
                        }
                        pan_target = pan_tables.compile(pan.target());
                        pan_compilations += 1;
                    }
                    "pan_commit" => {
                        let value = if event["pc"].as_u64() == Some(0xd534) {
                            0
                        } else {
                            pan_target
                        };
                        if event["expected_target"].as_u64() != Some(value as u64) {
                            return Err("Native common pan DSP commit differs".into());
                        }
                        renderer.set_pan_target(value as i16);
                        pan_commits += 1;
                    }
                    "expression" => {
                        expression.set(
                            event["channel"].as_u64().unwrap() as u8,
                            event["value"].as_u64().unwrap() as u8,
                        );
                        expression_events += 1;
                    }
                    "expression_cache" => {
                        let ch = event["channel"].as_u64().unwrap() as u8;
                        let value = event["value"].as_u64().unwrap() as u8;
                        if expression.value(ch) != value {
                            return Err(
                                "Native Expression state differs from Source cache input".into()
                            );
                        }
                        cached_expression.set(ch, value);
                        expression_cache_events += 1;
                    }
                    "amplifier_key" => {
                        if event["tracking"].as_u64() != Some(program.key_tracking as u64) {
                            return Err("Stored AMP key tracking differs".into());
                        }
                        key_modulation = tables.amplifier.key_modulation(
                            program.key_tracking,
                            event["relative_pitch"]
                                .as_u64()
                                .ok_or("AMP relative pitch absent")?
                                as i16,
                        );
                        key_compilations += 1;
                    }
                    "note_on" => {}
                    "tick" => {
                        controller.as_mut().unwrap().service(
                            &tables,
                            event["acknowledged"]
                                .as_bool()
                                .ok_or("Source acknowledgement missing")?,
                        );
                        if controller.as_ref().unwrap().envelope.stage
                            == radias_synth_domain::amp_envelope::EnvelopeStage::ReleaseHold
                        {
                            amplifier_delivery.release_zero();
                        }
                        services += 1;
                    }
                    "release" => controller.as_mut().unwrap().release(&tables),
                    "amplifier" => {
                        let input = event["control"]
                            .as_array()
                            .filter(|r| r.len() == 11)
                            .ok_or("Source amplifier inputs truncated")?;
                        let p = |i: usize| input[i].as_u64().unwrap() as u32;
                        let ctrl = controller.as_mut().unwrap();
                        if native_expression {
                            program.source_gain =
                                cached_expression.gain(channel, receive_flags, global);
                            ctrl.source_gain(program.source_gain, &tables);
                        }
                        target = ctrl.modulations(
                            [
                                if native_key {
                                    key_modulation
                                } else {
                                    p(6) as i16
                                },
                                p(7) as i16,
                            ],
                            &tables,
                        );
                        let native = ctrl.control();
                        let actual = [
                            u32::from(native.level),
                            native.level_offset as u8 as u32,
                            u32::from(native.source_gain),
                            u32::from(native.envelope_level),
                            u32::from(native.velocity),
                            u32::from(native.velocity_sensitivity),
                            native.modulation[0] as u16 as u32,
                            native.modulation[1] as u16 as u32,
                            u32::from(native.midi_volume.is_some()),
                            u32::from(native.midi_volume.unwrap_or(0)),
                            u32::from(native.program_volume),
                        ];
                        if actual != core::array::from_fn::<_, 11, _>(p) {
                            if errors < 3 {
                                eprintln!(
                                    "{name} amplifier frame{frame}: {actual:?} != {:?}",
                                    core::array::from_fn::<_, 11, _>(p)
                                );
                            }
                            errors += 1;
                        }
                        compiled_controls += 1;
                    }
                    "commit" => {
                        if frame as u64 >= plan.reference_start_frame {
                            let committed = if event["pc"].as_u64() == Some(0xd534) {
                                0
                            } else {
                                target
                            };
                            if Some(committed as u16 as u64) != event["expected_target"].as_u64() {
                                errors += 1;
                            }
                            renderer.set_envelope_target(committed);
                            commits += 1;
                        }
                    }
                    _ => return Err("Unknown source controller event".into()),
                }
                event_index += 1;
            }
            if frame as u64 >= plan.reference_start_frame
                && (frame as u64) < plan.reference_start_frame + plan.reference_voice_frames as u64
            {
                if frame as u64 == plan.reference_start_frame {
                    renderer.set_envelope_target(target);
                }
                let mut stereo = [StereoFrame::default(); 1];
                renderer.render(&waveform, &plan.events, &mut stereo);
                sample[0] = stereo[0].left;
                sample[1] = stereo[0].right;
                if frame as u64 + 1
                    == plan.reference_start_frame + plan.reference_voice_frames as u64
                {
                    inactive.initialize(
                        renderer.last_amplified(),
                        renderer.last_pan_current(),
                        renderer.envelope_rate(),
                    );
                }
            } else if frame as u64
                >= plan.reference_start_frame + plan.reference_voice_frames as u64
            {
                let stereo =
                    radias_synth_application::scale_bus(inactive.advance(StereoFrame::default()));
                sample[0] = stereo.left;
                sample[1] = stereo.right;
            }
        }
        wav::write_buses(
            &out.join(format!(
                "{name}-rust-amplifier-{}.wav",
                if native_rates { "delivery" } else { "program" }
            )),
            &output,
        )?;
        let passed = errors == 0 && services > 10 && commits > 10 && event_index == events.len();
        // These static drum-pan fixtures have one composition and two original
        // HPI commits (initial zero, then target), with no later pan changes.
        let passed = passed
            && (!native_common
                || (binding_events == 1 && pan_compilations == 1 && pan_commits == 2));
        let passed = passed && (!native_rates || rate_requests > 10 && rate_commits > 1);
        let report = serde_json::json!({"name":name,"passed":passed,"errors":errors,"services":services,
            "processed_events":event_index,"source_events":events.len(),
            "commits":commits,"compiled_controls":compiled_controls,"frames":output.len(),
            "native_AMP_key_tracking":native_key,"native_AMP_key_compilations":key_compilations,
            "native_Expression_state":native_expression,"native_Expression_events":expression_events,
            "native_Expression_cache_events":expression_cache_events,
            "native_program_common_binding":native_common,"native_binding_events":binding_events,
            "native_common_pan_compilations":pan_compilations,"native_common_pan_commits":pan_commits,
            "native_AMP_rate_packets":native_rates,"rate_requests":rate_requests,"rate_commits":rate_commits,"original_AMP_smoothing_rates_replayed":!native_rates,
            "stored_EG2_eight_fields_and_amp_level_used":true,"native_AmplifierProgram_controller_used":true,
            "velocity_time_sensitivity":program.envelope.velocity_time_sensitivity,
            "velocity_level_sensitivity":program.envelope.velocity_level_sensitivity,
            "key_tracking":program.envelope.key_tracking,"curve":program.envelope.curve,
            "accepted_note_velocity_modulation_and_shared_context_inputs_used":true,
            "other_compiled_DSP_controls_initial_actors_and_delivery_times_are_fixtures":true,
            "original_amplifier_targets_or_envelope_levels_replayed_to_render":false,
            "independent_controller_hpi_timing_qualified":false,"complete_native_engine":false});
        fs::write(
            out.join(format!(
                "{name}-amplifier-{}-parity.json",
                if native_rates {
                    "delivery-voice"
                } else {
                    "program"
                }
            )),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native stored amplifier control mismatches".into());
        }
    }
    Ok(())
}
