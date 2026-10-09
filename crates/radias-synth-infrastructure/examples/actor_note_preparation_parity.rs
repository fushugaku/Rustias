//! Whole SYS01ee38, raw shared state, ordered publications and unchanged E319.
use radias_synth_application::{
    actor_note_preparation::{
        ActorNotePreparationPublication, ActorNotePreparationPublicationError, prepare_actor_note,
    },
    dsp_transport::SendQueueError,
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_amplifier_preparation::{ActorAmplifierPorts, InvalidUnisonGainBank},
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_lfo_initialization::{ActorLfoState, InvalidLfoShape},
    actor_note_initialization::{
        ActorNoteInitializationPorts, ActorNoteInitializationTables, ActorPortamentoInitialization,
    },
    actor_note_preparation::{
        ActorNotePreparationError, ActorNotePreparationPorts, ActorNotePreparationRequest,
        ActorNotePreparationState, ActorNotePreparationTables,
    },
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    dsp_control::ParameterPacket,
    manual_parameters::{ManualCompilationError, ManualCompilerPorts, ManualCompilerTables},
    motion_initialization::{MotionControlState, MotionInitializationError},
    note_modulators::NoteModulatorTables,
    note_refresh::{NoteRefreshError, NoteRefreshTables},
    raw_note_scale::RawNoteScaleContext,
    virtual_patch_live::LiveCompilerTables,
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn pair(raw: &[u8], cursor: &mut usize) -> [ActorLfoState; 2] {
    core::array::from_fn(|_| {
        let bytes = raw[*cursor..*cursor + 32].try_into().unwrap();
        *cursor += 32;
        ActorLfoState { bytes }
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let pitch = firmware::note_pitch_tables(&sys)?;
    let scale = firmware::raw_note_scale_tables(&sys)?;
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let curves = firmware::envelope_curves(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let modulation = firmware::modulation_tables(&sys)?;
    let lfo = firmware::lfo_tables(&sys)?;
    let tempo = firmware::lfo_tempo_tables(&sys)?;
    let mixer = firmware::mixer_scales(&sys)?;
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let live = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let manual = ManualCompilerTables {
        live: &live,
        primary_pitch: dispatch,
    };
    let modulators = NoteModulatorTables {
        curves: &curves,
        timing: &timing,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let refresh = NoteRefreshTables {
        lfo: &lfo,
        compilers: &live,
        modulation: &modulation,
    };
    let note = ActorNoteInitializationTables {
        pitch: &pitch,
        scale: &scale,
        portamento: &portamento,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let tables = ActorNotePreparationTables {
        manual: &manual,
        tempo: &tempo,
        modulators: &modulators,
        refresh: &refresh,
        note: &note,
        mixer: &mixer,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let mix = master.filter_mix()?;
    let raw = fs::read(out.join("actor-note-preparation-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x50524531 {
        return Err("Unsupported actor note preparation observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut lfo_errors,
        mut motion_errors,
        mut seed_errors,
        mut accumulator_errors,
        mut packet_errors,
        mut transport_errors,
        mut clock_errors,
        mut receives,
        mut application_calls,
        mut atomic_rejections,
    ) = (
        0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32,
    );
    let mut parameter_words = 0u64;
    let mut coverage = [0u32; 5];
    let mut first_error = vec![serde_json::Value::Null; 5];
    let mut first_transport_error = None;
    let mut primary_selections = [0u32; 64];
    let mut secondary_selections = [0u32; 256];
    let mut scales = [0u32; 256];
    let mut stages = [[0u32; 8]; 3];
    let mut pairs = [[0u32; 40]; 16];
    let mut timbres = [0u32; 4];
    let mut gates = [0u32; 5];
    let mut max_plan = 0;
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
        let random_seed = take(&raw, &mut cursor) as u16;
        let lfo_clock_rate = take(&raw, &mut cursor);
        let master_tune = take(&raw, &mut cursor) as i32;
        let common_receive_flags = take(&raw, &mut cursor) as u8;
        let owner_receive_flags = take(&raw, &mut cursor) as u8;
        let configuration_mode = take(&raw, &mut cursor) as u8;
        let context_gain = take(&raw, &mut cursor) as u16;
        let midi_volume = take(&raw, &mut cursor) as u8;
        let timbre = take(&raw, &mut cursor) as u8;
        let midi_mode = take(&raw, &mut cursor) as u8;
        let bend_q16 = take(&raw, &mut cursor) as i32;
        let bend = take(&raw, &mut cursor) as i16;
        let wheel = take(&raw, &mut cursor) as u8;
        let auxiliary = take(&raw, &mut cursor) as i16;
        let time = take(&raw, &mut cursor) as u8;
        let switch = take(&raw, &mut cursor) != 0;
        let midi_pan = take(&raw, &mut cursor) as u8;
        let context_flags = take(&raw, &mut cursor) as u8;
        let gate_flags = take(&raw, &mut cursor) as u8;
        let context_pitch_q16 = take(&raw, &mut cursor) as i32;
        let selection = take(&raw, &mut cursor) as u8;
        let transpose_enabled = take(&raw, &mut cursor) != 0;
        let transpose = take(&raw, &mut cursor) as i8;
        let motion_program_flags = take(&raw, &mut cursor) as u8;
        let motion_global_flags = take(&raw, &mut cursor) as u8;
        let motion_assignments = core::array::from_fn(|_| take(&raw, &mut cursor) as u8);
        let mut assignments = [0; 5];
        let mut assignable_values = [0; 5];
        for i in 0..5 {
            assignments[i] = take(&raw, &mut cursor) as u8;
            assignable_values[i] = take(&raw, &mut cursor) as i8;
        }
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let lfos = pair(&raw, &mut cursor);
        let shared_lfos = pair(&raw, &mut cursor);
        let motion = MotionControlState {
            bytes: raw[cursor..cursor + 48].try_into()?,
        };
        cursor += 48;
        let custom_cents = core::array::from_fn(|i| raw[cursor + i] as i8);
        cursor += 256;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let expected_seed = take(&raw, &mut cursor) as u16;
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let after_lfos = pair(&raw, &mut cursor);
        let after_motion = MotionControlState {
            bytes: raw[cursor..cursor + 48].try_into()?,
        };
        cursor += 48;
        let targets: [i32; 40] = core::array::from_fn(|_| take(&raw, &mut cursor) as i32);
        let linked = take(&raw, &mut cursor) as i32;
        let count = take(&raw, &mut cursor);
        let mut packets = Vec::new();
        for _ in 0..count {
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let state: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((ack, payload, state));
        }
        let pitch_ports = ActorPitchPorts {
            timbre,
            midi_mode,
            bend_q16,
            wheel,
            common_receive_flags,
        };
        let sources = ActorVirtualPatchPorts {
            bend,
            wheel,
            auxiliary,
            midi_receive_flags: owner_receive_flags,
            assignments,
            assignable_values,
        };
        let note_ports = ActorNoteInitializationPorts {
            master_tune,
            pitch: pitch_ports,
            scale: RawNoteScaleContext {
                selection,
                global_transpose: transpose_enabled.then_some(transpose),
                custom_cents,
            },
            portamento: ActorPortamentoInitialization {
                time,
                switch_required: common_receive_flags & 8 != 0,
                switch,
                context_flags,
                gate_flags,
                context_pitch_q16,
            },
            sources,
        };
        let amplifier_ports = ActorAmplifierPorts {
            configuration_mode,
            owner_receive_flags,
            context_gain,
            midi_volume,
        };
        let request = ActorNotePreparationRequest {
            body: &body,
            shared_lfos,
            ports: ActorNotePreparationPorts {
                motion_assignments,
                motion_program_flags,
                motion_global_flags,
                lfo_clock_rate,
                midi_pan,
                note: note_ports,
                amplifier: amplifier_ports,
            },
        };
        let prior = ActorNotePreparationState {
            controller: ActorControlState { bytes: before },
            motion,
            lfos,
            random_seed,
        };
        let slot = (local + 12 * chip) as usize;
        let mut state = prior;
        let mut work = 0u16;
        if service < 4 {
            if count != 0 {
                return Err("Pure initial tail unexpectedly published DSP commands".into());
            }
            work = match service {
                0 => state.controller.prepare_initial_mixer_level(&body, false),
                1 => state.controller.prepare_initial_mixer_level(&body, true),
                2 => state.controller.prepare_initial_pan(&body, midi_pan),
                _ => state
                    .controller
                    .prepare_initial_amplifier(&body, amplifier_ports, &amplifier)
                    .map_err(|e| format!("{e:?}"))?,
            };
            clock_errors += u32::from(u64::from(work) != returned);
        } else {
            let compiled = prior
                .prepare_note_controls(request, &tables)
                .map_err(|e| format!("{e:?}"))?;
            state = compiled.state;
            max_plan = max_plan.max(compiled.publication.operations().len());
            accumulator_errors += u32::from(
                compiled.modulations.values != targets
                    || compiled.modulations.linked_pitch != linked,
            );
            let native_packets: Vec<Vec<u16>> = compiled
                .publication
                .operations()
                .iter()
                .filter_map(|op| match *op {
                    DescriptorOperation::Send {
                        sender,
                        offset,
                        value,
                        ..
                    } => Some(
                        ParameterPacket::from_sender(
                            sender,
                            0x2000 + 160 * local + u32::from(offset),
                            value,
                        )
                        .unwrap()
                        .words()
                        .to_vec(),
                    ),
                    _ => None,
                })
                .collect();
            if native_packets
                != packets
                    .iter()
                    .map(|(_, p, _)| p.clone())
                    .collect::<Vec<_>>()
            {
                packet_errors += 1;
            }
            for partition in [1u64, 31, 3000] {
                let mut transport = SynthesisParameterTransport::default();
                transport.configure_constructor_filter_mix(
                    radias_synth_domain::filter_control::FilterMixTable {
                        weights: mix.weights,
                    },
                );
                transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
                transport.restore_parameters(slot, before_dsp);
                let mut candidate = prior;
                prepare_actor_note(
                    ActorNotePreparationPublication {
                        clock: 0,
                        slot,
                        controls: request,
                    },
                    &mut candidate,
                    &tables,
                    &mut transport,
                )
                .map_err(|e| format!("{e:?}"))?;
                if candidate != compiled.state {
                    return Err("Application candidate differs".into());
                }
                application_calls += 1;
                let (mut clock, mut ready_at, mut seen_count, mut good) = (0, busy, 0, true);
                let mut native_acks = Vec::new();
                for (ack, payload, expected) in &packets {
                    let mut seen = None;
                    while clock < *ack {
                        clock = (clock + partition).min(*ack);
                        transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{native_acks.push(time);seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..}if opcode==payload[1])));});
                    }
                    good &= seen == Some((*ack, slot, true))
                        && transport.parameter_state(slot) == *expected;
                    ready_at = *ack + busy;
                    parameter_words += 160;
                }
                transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
                good &= seen_count == count
                    && transport.pending() == 0
                    && transport.caller_available_clock() == returned;
                if !good {
                    transport_errors += 1;
                    first_transport_error.get_or_insert(serde_json::json!({"variant":variant,"partition":partition,"source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending(),"native_acks":native_acks,"source_acks":packets.iter().map(|(t,_,_)|t).collect::<Vec<_>>(),"native_plan":format!("{:?}",compiled.publication.operations())}));
                }
            }
            if coverage[4] == 0 {
                use ActorNotePreparationError::{Motion, Refresh, VirtualPatch};
                use ActorNotePreparationPublicationError::{Compile, Transport};
                for case in 0..9 {
                    let mut silent = body;
                    let mut candidate = prior;
                    for route in 0..6 {
                        silent[88 + 3 * route] = 64;
                        candidate.controller.bytes[0x1c4 + 2 * route] = 0;
                        candidate.controller.bytes[0x158 + 2 * route..0x15a + 2 * route].fill(0);
                    }
                    candidate.motion.bytes[0x2c] |= 128;
                    for track in 0..3 {
                        candidate.motion.bytes[12 * track + 4..12 * track + 6]
                            .copy_from_slice(&0x4000u16.to_be_bytes());
                        candidate.motion.bytes[12 * track + 6..12 * track + 8].fill(0);
                    }
                    let mut rejected = request;
                    rejected.ports.motion_assignments = [34, 35, 36];
                    rejected.ports.motion_program_flags = 128;
                    rejected.ports.motion_global_flags = 0;
                    rejected.ports.note.pitch.midi_mode = ((timbre + 2) << 5) & 0xe0;
                    let expected = match case {
                        0 => {
                            silent[86..89].copy_from_slice(&[5, 63, 127]);
                            rejected.ports.note.sources.wheel = 127;
                            rejected.ports.note.sources.midi_receive_flags |= 16;
                            Compile(VirtualPatch(ActorVirtualPatchError::InvalidDestination(63)))
                        }
                        1 => {
                            silent[77] = 128;
                            silent[79] = 64;
                            silent[84] = 64;
                            Compile(Refresh(NoteRefreshError::LfoShape(InvalidLfoShape)))
                        }
                        2 => {
                            candidate.controller.bytes[0x1ea] = 128;
                            Compile(Refresh(NoteRefreshError::GainBank(InvalidUnisonGainBank)))
                        }
                        3 => {
                            candidate.controller.bytes[0x1e0] = 6;
                            rejected.ports.motion_assignments[2] = 3;
                            Compile(Motion(MotionInitializationError::Manual(
                                ManualCompilationError::UnsupportedPrimary,
                            )))
                        }
                        4 => {
                            candidate.controller.bytes[0x1e3] = 2;
                            candidate.controller.bytes[0x1e4] = 15;
                            rejected.ports.motion_assignments[2] = 21;
                            Compile(Motion(MotionInitializationError::Manual(
                                ManualCompilationError::UnsupportedShaper,
                            )))
                        }
                        5 => Transport(SendQueueError::InvalidSlot),
                        6 => Transport(SendQueueError::Full),
                        7 => {
                            rejected.ports.motion_assignments[2] = 12;
                            Transport(SendQueueError::MissingFilterContext)
                        }
                        _ => {
                            rejected.ports.motion_assignments[2] = 5;
                            Transport(SendQueueError::MissingPitchContext)
                        }
                    };
                    rejected.body = &silent;
                    let mut transport = SynthesisParameterTransport::default();
                    if case != 7 {
                        transport.configure_constructor_filter_mix(
                            radias_synth_domain::filter_control::FilterMixTable {
                                weights: mix.weights,
                            },
                        );
                    }
                    if case != 8 {
                        transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
                    }
                    if case == 6 {
                        let idle = candidate
                            .controller
                            .compile_manual_parameter(
                                34,
                                0,
                                &silent,
                                ManualCompilerPorts {
                                    live:
                                        radias_synth_domain::virtual_patch_live::LiveCompilerPorts {
                                            portamento_time: time,
                                            portamento_switch_required: common_receive_flags & 8
                                                != 0,
                                            portamento_switch: switch,
                                            midi_pan: None,
                                        },
                                    pitch: pitch_ports,
                                    amplifier: amplifier_ports,
                                },
                                &manual,
                            )
                            .map_err(|e| format!("{e:?}"))?;
                        for _ in 0..512 {
                            transport
                                .publish_actor_descriptors(0, slot, &idle.publication)
                                .map_err(|e| format!("Queue preparation: {e:?}"))?;
                        }
                    }
                    let saved = candidate;
                    let pending = transport.pending();
                    let parameters = transport.parameter_state(slot);
                    let result = prepare_actor_note(
                        ActorNotePreparationPublication {
                            clock: 0,
                            slot: if case == 5 { 24 } else { slot },
                            controls: rejected,
                        },
                        &mut candidate,
                        &tables,
                        &mut transport,
                    );
                    if result != Err(expected)
                        || candidate != saved
                        || transport.pending() != pending
                        || transport.parameter_state(slot) != parameters
                    {
                        return Err(
                            format!("Non-atomic preparation rejection {case}: {result:?}").into(),
                        );
                    }
                    atomic_rejections += 1;
                }
            }
            primary_selections[usize::from(before[0x1e0] & 63)] += 1;
            secondary_selections[usize::from(before[0x1e1])] += 1;
            scales[usize::from(selection)] += 1;
            timbres[usize::from(timbre)] += 1;
            for e in 0..3 {
                stages[e][usize::from(before[0x94 + e])] += 1;
            }
            for route in 0..6 {
                pairs[usize::from(body[86 + 3 * route] & 15)]
                    [usize::from(body[87 + 3 * route] & 63)] += 1;
            }
            let drum = i32::from((midi_mode & 0xe0) >> 5) - 1 == i32::from(timbre & 3);
            let gate = if motion.bytes[0x2c] & 128 == 0 {
                0
            } else if motion_program_flags & 128 == 0 {
                1
            } else if drum {
                2
            } else if motion_global_flags & 2 != 0 {
                3
            } else {
                4
            };
            gates[gate] += 1;
        }
        let state_error = state.controller.bytes != after;
        let lfo_error = state.lfos != after_lfos;
        let motion_error = state.motion != after_motion;
        let seed_error = state.random_seed != expected_seed;
        state_errors += u32::from(state_error);
        lfo_errors += u32::from(lfo_error);
        motion_errors += u32::from(motion_error);
        seed_errors += u32::from(seed_error);
        if (state_error
            || lfo_error
            || motion_error
            || seed_error
            || service < 4 && u64::from(work) != returned)
            && first_error[service as usize].is_null()
        {
            let offset = state
                .controller
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b);
            first_error[service as usize] = serde_json::json!({"variant":variant,"offset":offset,"native":offset.map(|i|state.controller.bytes[i]),"original":offset.map(|i|after[i]),"lfo_error":lfo_error,"motion_error":motion_error,"seed_error":seed_error,"native_work":work,"original_return":returned});
        }
        coverage[service as usize] += 1;
        receives += count;
        calls += 1;
    }
    let passed = state_errors
        + lfo_errors
        + motion_errors
        + seed_errors
        + accumulator_errors
        + packet_errors
        + transport_errors
        + clock_errors
        == 0
        && coverage == [4096; 5]
        && (0..6).all(|wave| (0..4).all(|mode| primary_selections[wave + 16 * mode] > 0))
        && timbres == [1024; 4]
        && stages == [[512; 8]; 3]
        && application_calls == 12288
        && atomic_rejections == 9
        && secondary_selections.iter().all(|c| *c > 0)
        && scales.iter().all(|c| *c == 16)
        && pairs.iter().all(|r| r.iter().all(|c| *c > 0))
        && gates.iter().all(|c| *c > 0);
    let report = serde_json::json!({"passed":passed,"whole_original_preparation_calls":calls,"whole_SYS01ee38_calls":coverage[4],"procedure_coverage":coverage,"whole_original_E319_receives":receives,"controller_bytes_compared":u64::from(calls)*496,"LFO_bytes_compared":u64::from(calls)*64,"motion_bytes_compared":u64::from(calls)*48,"parameter_words_compared":parameter_words,"application_calls":application_calls,"atomic_rejection_cases":atomic_rejections,"max_publication_operations":max_plan,"primary_selection_coverage":primary_selections.to_vec(),"secondary_selection_coverage":secondary_selections.to_vec(),"scale_root_coverage":scales.to_vec(),"EG_stage_coverage":stages,"timbre_coverage":timbres,"motion_gate_coverage":gates,"all640_route_input_pairs_covered":pairs.iter().all(|r|r.iter().all(|c|*c>0)),"state_errors":state_errors,"LFO_errors":lfo_errors,"motion_errors":motion_errors,"seed_errors":seed_errors,"accumulator_errors":accumulator_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"clock_errors":clock_errors,"first_error_by_service":first_error,"first_transport_error":first_transport_error,"source_outputs_used_only_for_assertions":true,"whole_actor_construction_SYS01e838_qualified":false,"independent_whole_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-note-preparation-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Complete native actor note preparation differs".into());
    }
    Ok(())
}
