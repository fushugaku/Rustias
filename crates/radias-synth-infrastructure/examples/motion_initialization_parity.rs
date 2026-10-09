//! Whole unchanged motion initialization, native shared history/state/FIFO work.
use radias_synth_application::{
    dsp_transport::SendQueueError,
    motion_initialization::{
        MotionAssignmentPublication, MotionNotePublication, MotionNotePublicationError,
        initialize_motion_assignment, initialize_note_motion,
    },
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_pitch_preparation::ActorPitchPorts,
    dsp_control::ParameterPacket,
    manual_parameters::{ManualCompilationError, ManualCompilerPorts, ManualCompilerTables},
    motion_initialization::{MotionControlState, MotionInitializationError, MotionNoteRequest},
    virtual_patch_live::{LiveCompilerPorts, LiveCompilerTables},
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
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
    let primary_pitch = firmware::primary_pitch_sender_table(&sys)?;
    let tables = ManualCompilerTables {
        live: &live,
        primary_pitch,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let mix = master.filter_mix()?;
    let raw = fs::read(out.join("motion-initialization-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4d4f5431 {
        return Err("Unsupported motion observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut motion_errors,
        mut packet_errors,
        mut transport_errors,
        mut receives,
        mut application_calls,
        mut atomic_rejections,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let mut parameter_words = 0u64;
    let mut procedure_coverage = [0u32; 2];
    let mut single_assignments = [0u32; 256];
    let mut active_assignments = [[0u32; 256]; 3];
    let mut gates = [0u32; 5];
    let mut special_unchanged = [0u32; 2];
    let (
        mut first_state_error,
        mut first_motion_error,
        mut first_packet_error,
        mut first_transport_error,
    ) = (None, None, None, None);
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
        let track = take(&raw, &mut cursor) as u8;
        let assignments = core::array::from_fn(|_| take(&raw, &mut cursor) as u8);
        let program_flags = take(&raw, &mut cursor) as u8;
        let global_flags = take(&raw, &mut cursor) as u8;
        let common_receive_flags = take(&raw, &mut cursor) as u8;
        let owner_receive_flags = take(&raw, &mut cursor) as u8;
        let wheel = take(&raw, &mut cursor) as u8;
        let timbre = take(&raw, &mut cursor) as u8;
        let midi_mode = take(&raw, &mut cursor) as u8;
        let portamento_time = take(&raw, &mut cursor) as u8;
        let portamento_switch = take(&raw, &mut cursor) != 0;
        let configuration_mode = take(&raw, &mut cursor) as u8;
        let midi_volume = take(&raw, &mut cursor) as u8;
        let midi_pan = take(&raw, &mut cursor) as u8;
        let context_gain = take(&raw, &mut cursor) as u16;
        let bend_q16 = take(&raw, &mut cursor) as i32;
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_motion: [u8; 48] = raw[cursor..cursor + 48].try_into()?;
        cursor += 48;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let after_motion: [u8; 48] = raw[cursor..cursor + 48].try_into()?;
        cursor += 48;
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
        let ports = ManualCompilerPorts {
            live: LiveCompilerPorts {
                portamento_time,
                portamento_switch_required: common_receive_flags & 8 != 0,
                portamento_switch,
                midi_pan: (before[0x1e7] != 0).then_some(midi_pan),
            },
            pitch: ActorPitchPorts {
                timbre,
                midi_mode,
                bend_q16,
                wheel,
                common_receive_flags,
            },
            amplifier: ActorAmplifierPorts {
                configuration_mode,
                owner_receive_flags,
                context_gain,
                midi_volume,
            },
        };
        let request = MotionNoteRequest {
            assignments,
            program_flags,
            global_flags,
            body: &body,
            ports,
        };
        let prior = ActorControlState { bytes: before };
        let initial_motion = MotionControlState {
            bytes: before_motion,
        };
        let slot = (local + 12 * chip) as usize;
        let compiled = if service == 0 {
            prior.initialize_motion_assignment(
                assignments[usize::from(track)],
                track,
                initial_motion,
                &body,
                ports,
                &tables,
            )
        } else {
            prior.initialize_motion_note(request, initial_motion, &tables)
        }
        .map_err(|e| format!("{e:?}"))?;
        if compiled.controller.bytes != after {
            state_errors += 1;
            let offset = compiled
                .controller
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b)
                .unwrap();
            first_state_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"offset":offset,"native":compiled.controller.bytes[offset],"original":after[offset]}));
        }
        if compiled.motion.bytes != after_motion {
            motion_errors += 1;
            first_motion_error
                .get_or_insert(serde_json::json!({"service":service,"variant":variant}));
        }
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
            first_packet_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"native":native_packets,"original":packets.iter().map(|(_,p,_)|p).collect::<Vec<_>>()}));
        }
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), primary_pitch);
            transport.restore_parameters(slot, before_dsp);
            let mut controller = prior;
            let mut motion = initial_motion;
            if service == 0 {
                initialize_motion_assignment(
                    MotionAssignmentPublication {
                        clock: 0,
                        slot,
                        assignment: assignments[usize::from(track)],
                        track,
                        body: &body,
                        ports,
                    },
                    &mut controller,
                    &mut motion,
                    &tables,
                    &mut transport,
                )
            } else {
                initialize_note_motion(
                    MotionNotePublication {
                        clock: 0,
                        slot,
                        controls: request,
                    },
                    &mut controller,
                    &mut motion,
                    &tables,
                    &mut transport,
                )
            }
            .map_err(|e| format!("{e:?}"))?;
            if controller.bytes != after || motion.bytes != after_motion {
                return Err("Application motion state differs".into());
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
                first_transport_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"partition":partition,"source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending(),"native_acks":native_acks,"source_acks":packets.iter().map(|(t,_,_)|t).collect::<Vec<_>>(),"native_plan":format!("{:?}",compiled.publication.operations())}));
            }
        }
        if service == 0 {
            let assignment = assignments[usize::from(track)];
            single_assignments[usize::from(assignment)] += 1;
            if assignment == 4 && matches!(before[0x1e0] & 15, 6 | 7) {
                special_unchanged[usize::from((before[0x1e0] & 15) - 6)] += 1;
            }
        } else {
            let drum = i32::from((midi_mode & 0xe0) >> 5) - 1 == i32::from(timbre & 3);
            let gate = if before_motion[0x2c] & 128 == 0 {
                0
            } else if program_flags & 128 == 0 {
                1
            } else if drum {
                2
            } else if global_flags & 2 != 0 {
                3
            } else {
                4
            };
            gates[gate] += 1;
            if gate == 4 {
                for (i, a) in assignments.iter().enumerate() {
                    active_assignments[i][usize::from(*a)] += 1;
                }
            }
        }
        if calls == 0 {
            use MotionNotePublicationError::{Compile, Transport};
            for case in 0..7 {
                let mut controller = prior;
                let mut motion = initial_motion;
                motion.bytes[0x2c] |= 128;
                for index in 0..3 {
                    motion.bytes[12 * index + 4..12 * index + 6]
                        .copy_from_slice(&0x4000u16.to_be_bytes());
                    motion.bytes[12 * index + 6..12 * index + 8].fill(0);
                }
                let mut rejected = MotionNoteRequest {
                    assignments: [34, 35, 36],
                    program_flags: 128,
                    global_flags: 0,
                    ..request
                };
                rejected.ports.pitch.midi_mode = ((timbre + 2) << 5) & 0xe0;
                let mut transport = SynthesisParameterTransport::default();
                if case != 3 {
                    transport.configure_constructor_filter_mix(
                        radias_synth_domain::filter_control::FilterMixTable {
                            weights: mix.weights,
                        },
                    );
                }
                if case != 4 {
                    transport.configure_pitch_receivers(pitch_rom.clone(), primary_pitch);
                }
                let expected = match case {
                    0 => {
                        controller.bytes[0x1e0] = 6;
                        rejected.assignments[2] = 4;
                        Compile(MotionInitializationError::Manual(
                            ManualCompilationError::UnsupportedPrimary,
                        ))
                    }
                    1 => {
                        controller.bytes[0x1e3] = 2;
                        controller.bytes[0x1e4] = 15;
                        rejected.assignments[1] = 21;
                        Compile(MotionInitializationError::Manual(
                            ManualCompilationError::UnsupportedShaper,
                        ))
                    }
                    2 => {
                        controller.bytes[0x1ea] = 128;
                        rejected.assignments[2] = 19;
                        Compile(MotionInitializationError::Manual(
                            ManualCompilationError::InvalidGainBank,
                        ))
                    }
                    3 => {
                        rejected.assignments[1] = 12;
                        Transport(SendQueueError::MissingFilterContext)
                    }
                    4 => {
                        rejected.assignments[2] = 5;
                        Transport(SendQueueError::MissingPitchContext)
                    }
                    5 => Transport(SendQueueError::InvalidSlot),
                    _ => Transport(SendQueueError::Full),
                };
                if case == 6 {
                    let idle = prior
                        .compile_manual_parameter(34, 0, &body, ports, &tables)
                        .map_err(|e| format!("{e:?}"))?;
                    for _ in 0..512 {
                        transport
                            .publish_actor_descriptors(0, slot, &idle.publication)
                            .map_err(|e| format!("{e:?}"))?;
                    }
                }
                let (saved, saved_motion, pending, parameters) = (
                    controller,
                    motion,
                    transport.pending(),
                    transport.parameter_state(slot),
                );
                let result = initialize_note_motion(
                    MotionNotePublication {
                        clock: 0,
                        slot: if case == 5 { 24 } else { slot },
                        controls: rejected,
                    },
                    &mut controller,
                    &mut motion,
                    &tables,
                    &mut transport,
                );
                if result != Err(expected)
                    || controller != saved
                    || motion != saved_motion
                    || transport.pending() != pending
                    || transport.parameter_state(slot) != parameters
                {
                    return Err(format!("Non-atomic motion rejection {case}: {result:?}").into());
                }
                atomic_rejections += 1;
            }
        }
        procedure_coverage[service as usize] += 1;
        receives += count;
        calls += 1;
    }
    let passed = state_errors + motion_errors + packet_errors + transport_errors == 0
        && procedure_coverage == [32768, 8192]
        && application_calls == 122880
        && atomic_rejections == 7
        && single_assignments.iter().all(|c| *c == 128)
        && gates.iter().all(|c| *c > 0)
        && special_unchanged.iter().all(|c| *c > 0)
        && active_assignments.iter().all(|a| a.iter().all(|c| *c > 0));
    let report = serde_json::json!({"passed":passed,"whole_original_motion_calls":calls,"whole_original_E319_receives":receives,"procedure_coverage":procedure_coverage,"single_assignment_byte_coverage":single_assignments.to_vec(),"active_assignment_byte_coverage":active_assignments.iter().map(|v|v.to_vec()).collect::<Vec<_>>(),"gate_coverage":gates,"PCM_input_special_unchanged_selection_coverage":special_unchanged,"controller_bytes_compared":u64::from(calls)*496,"motion_bytes_compared":u64::from(calls)*48,"parameter_words_compared":parameter_words,"application_calls":application_calls,"atomic_rejection_cases":atomic_rejections,"state_errors":state_errors,"motion_errors":motion_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"first_state_error":first_state_error,"first_motion_error":first_motion_error,"first_packet_error":first_packet_error,"first_transport_error":first_transport_error,"source_outputs_used_only_for_assertions":true,"PCM_and_external_input_special_CTRL2_callbacks_qualified":false,"independent_whole_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("motion-initialization-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Whole native motion initialization differs".into());
    }
    Ok(())
}
