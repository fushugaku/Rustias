//! Whole original preparation-only Virtual Patch calls, not partial stores.
use radias_synth_application::actor_preparation::{
    ActorPreparationError, ActorPreparationRequest, ActorPreparationTables,
    prepare_primary_and_virtual_patches, prepare_virtual_patches_and_publish_descriptors,
};
use radias_synth_application::dsp_transport::SendQueueError;
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
};
use radias_synth_infrastructure::firmware::{
    self, MasterTables, amplifier_tables, modulation_tables,
};
use std::{fs, path::PathBuf};

fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let amplifier = amplifier_tables(&sys)?;
    let tables = modulation_tables(&sys)?;
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let descriptor_tables = firmware::parameter_template_tables(&sys, master.filter_mix()?)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let pitch_dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let raw = fs::read(out.join("actor-virtual-patch-original.bin"))?;
    let (mut cursor, mut calls, mut controller_errors, mut accumulator_errors) = (0, 0, 0, 0);
    let mut first_error = None;
    let mut first_source_error = None;
    let mut source_errors = 0;
    let mut clock_errors = 0;
    let mut first_clock_error = None;
    let mut family_coverage = [0u32; 3];
    let (mut transport_errors, mut receives, mut parameter_words) = (0u32, 0u32, 0u64);
    let mut first_transport_error = None;
    let mut atomic_rejections = 0;
    let mut source_coverage = [0u32; 16];
    let mut destination_coverage = [0u32; 40];
    let mut assignment_coverage = [0u32; 256];
    let mut receive_flag_coverage = [0u32; 256];
    let mut source_destination_coverage = [[0u32; 40]; 16];
    while cursor < raw.len() {
        let family = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let busy = u64::from(take(&raw, &mut cursor));
        let bend = take(&raw, &mut cursor) as i16;
        let wheel = take(&raw, &mut cursor) as u8;
        let auxiliary = take(&raw, &mut cursor) as i16;
        let owner = take(&raw, &mut cursor) as u8;
        let midi_receive_flags = take(&raw, &mut cursor) as u8;
        let mut assignments = [0; 5];
        let mut assignable_values = [0; 5];
        for index in 0..5 {
            assignments[index] = take(&raw, &mut cursor) as u8;
            assignable_values[index] = take(&raw, &mut cursor) as i8;
        }
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let source_clocks = take(&raw, &mut cursor);
        if take(&raw, &mut cursor) != 0 {
            return Err("Preparation-only original emitted HPI packets".into());
        }
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let expected: [i32; 40] = core::array::from_fn(|_| take(&raw, &mut cursor) as i32);
        let linked = take(&raw, &mut cursor) as i32;
        let mut expected_sources = [0i32; 16];
        let mut source_work = [0u16; 16];
        for source in 0..16 {
            expected_sources[source] = take(&raw, &mut cursor) as i32;
            source_work[source] = take(&raw, &mut cursor) as u16;
        }
        let route_work: [u16; 6] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let target_work: [u16; 40] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let packet_count = take(&raw, &mut cursor) as usize;
        let mut packets = Vec::new();
        for _ in 0..packet_count {
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let state: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((ack, payload, state));
        }
        let mut controller = ActorControlState { bytes: before };
        let ports = ActorVirtualPatchPorts {
            bend,
            wheel,
            auxiliary,
            midi_receive_flags,
            assignments,
            assignable_values,
        };
        let native_sources = controller.virtual_patch_sources(&body, ports, &amplifier);
        for selector in 0..16 {
            if native_sources[selector] != expected_sources[selector] {
                source_errors += 1;
                first_source_error.get_or_insert(serde_json::json!({"call": calls,"selector":selector,"native":native_sources[selector],"original":expected_sources[selector],"ports":format!("{ports:?}"),"velocity":before[0x37],"sensitivities":[body[0x39],body[0x41],body[0x49]]}));
            }
        }
        let (targets, work, total_work) = if family != 0 {
            let prepared = prepare_primary_and_virtual_patches(
                &controller,
                &body,
                owner,
                ports,
                &amplifier,
                &tables,
            )
            .map_err(|e| format!("{e:?}"))?;
            controller = prepared.controller;
            (
                prepared.modulations,
                prepared.virtual_patch_work,
                prepared.primary_work_clocks + prepared.virtual_patch_work.total(),
            )
        } else {
            let (targets, work) = controller
                .prepare_virtual_patches_with_work(&body, ports, &amplifier, &tables)
                .map_err(|e| format!("{e:?}"))?;
            (targets, work, work.total())
        };
        for (label, native, original) in [
            ("source", work.sources.to_vec(), source_work.to_vec()),
            ("route", work.routes.to_vec(), route_work.to_vec()),
            ("target", work.targets.to_vec(), target_work.to_vec()),
            ("whole", vec![total_work], vec![source_clocks as u16]),
        ] {
            if native != original {
                clock_errors += 1;
                let index = native
                    .iter()
                    .zip(&original)
                    .position(|(a, b)| a != b)
                    .unwrap();
                first_clock_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"part":label,"index":index,"native":native[index],"original":original[index],"body":body.to_vec(),"sources":native_sources}));
            }
        }
        if controller.bytes != after {
            controller_errors += 1;
            let offset = controller
                .bytes
                .iter()
                .zip(after)
                .position(|(native, original)| *native != original)
                .unwrap();
            first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"variant":variant,"offset":offset,"native":controller.bytes[offset],"original":after[offset]}));
        }
        if targets.values != expected || targets.linked_pitch != linked {
            accumulator_errors += 1;
            let destination = targets
                .values
                .iter()
                .zip(expected)
                .position(|(native, original)| *native != original);
            first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"variant":variant,"destination":destination,"native":destination.map(|d|targets.values[d]),"original":destination.map(|d|expected[d]),"native_linked":targets.linked_pitch,"original_linked":linked}));
        }
        if family == 2 {
            let slot = local + 12 * chip;
            if atomic_rejections == 0 {
                let mut state = ActorControlState { bytes: before };
                let saved = state;
                let mut transport = SynthesisParameterTransport::default();
                let missing = prepare_virtual_patches_and_publish_descriptors(
                    ActorPreparationRequest {
                        clock: 0,
                        slot,
                        body: &body,
                        owner_mode: owner,
                        ports,
                    },
                    &mut state,
                    ActorPreparationTables {
                        amplifier: &amplifier,
                        modulation: &tables,
                        descriptors: &descriptor_tables,
                    },
                    &mut transport,
                );
                if missing
                    != Err(ActorPreparationError::Publish(
                        SendQueueError::MissingFilterContext,
                    ))
                    || state != saved
                    || transport.pending() != 0
                {
                    return Err("Missing context changed preparation/FIFO".into());
                }
                let mut invalid = body;
                invalid[0x56] = 5;
                invalid[0x57] = 63;
                invalid[0x58] = 127;
                state.bytes[0x37] = 127;
                state.bytes[0x1c4] = 0;
                state.bytes[0x158..0x15a].fill(0);
                let saved = state;
                let rejected = prepare_virtual_patches_and_publish_descriptors(
                    ActorPreparationRequest {
                        clock: 0,
                        slot,
                        body: &invalid,
                        owner_mode: owner,
                        ports,
                    },
                    &mut state,
                    ActorPreparationTables {
                        amplifier: &amplifier,
                        modulation: &tables,
                        descriptors: &descriptor_tables,
                    },
                    &mut transport,
                );
                if rejected
                    != Err(ActorPreparationError::VirtualPatch(
                        ActorVirtualPatchError::InvalidDestination(63),
                    ))
                    || state != saved
                    || transport.pending() != 0
                {
                    return Err("Invalid route changed preparation/FIFO".into());
                }
                transport.configure_constructor_filter_mix(master.filter_mix()?);
                transport.configure_pitch_receivers(pitch_rom.clone(), pitch_dispatch);
                loop {
                    let saved = state;
                    let pending = transport.pending();
                    match prepare_virtual_patches_and_publish_descriptors(
                        ActorPreparationRequest {
                            clock: 0,
                            slot,
                            body: &body,
                            owner_mode: owner,
                            ports,
                        },
                        &mut state,
                        ActorPreparationTables {
                            amplifier: &amplifier,
                            modulation: &tables,
                            descriptors: &descriptor_tables,
                        },
                        &mut transport,
                    ) {
                        Ok(()) => {}
                        Err(ActorPreparationError::Publish(SendQueueError::Full)) => {
                            if state != saved || transport.pending() != pending {
                                return Err("Full queue changed preparation/FIFO".into());
                            }
                            break;
                        }
                        Err(e) => {
                            return Err(format!("Unexpected preparation rejection: {e:?}").into());
                        }
                    }
                }
                atomic_rejections = 3;
            }
            for partition in [1u64, 31, 3000] {
                let mut transport = SynthesisParameterTransport::default();
                transport.configure_constructor_filter_mix(master.filter_mix()?);
                transport.configure_pitch_receivers(pitch_rom.clone(), pitch_dispatch);
                transport.restore_parameters(slot, before_dsp);
                let mut state = ActorControlState { bytes: before };
                prepare_virtual_patches_and_publish_descriptors(
                    ActorPreparationRequest {
                        clock: 0,
                        slot,
                        body: &body,
                        owner_mode: owner,
                        ports,
                    },
                    &mut state,
                    ActorPreparationTables {
                        amplifier: &amplifier,
                        modulation: &tables,
                        descriptors: &descriptor_tables,
                    },
                    &mut transport,
                )
                .map_err(|e| format!("{e:?}"))?;
                let (mut clock, mut ready_at, mut seen_count, mut good) =
                    (0, busy, 0, state == controller);
                for (ack, payload, expected) in &packets {
                    let mut seen = None;
                    while clock < *ack {
                        clock = (clock + partition).min(*ack);
                        transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{
                            seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));
                        });
                    }
                    good &= seen == Some((*ack, slot, true))
                        && transport.parameter_state(slot) == *expected;
                    ready_at = *ack + busy;
                    parameter_words += 160;
                }
                transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
                good &= seen_count == packet_count
                    && transport.pending() == 0
                    && transport.caller_available_clock() == returned;
                if !good {
                    transport_errors += 1;
                    first_transport_error.get_or_insert(serde_json::json!({"call":calls,"partition":partition,"source_return":returned,"native_return":transport.caller_available_clock(),"expected_receives":packet_count,"native_receives":seen_count}));
                }
            }
        }
        receives += packet_count as u32;
        for route in 0..6 {
            source_coverage[usize::from(body[0x56 + 3 * route] & 15)] += 1;
            destination_coverage[usize::from(body[0x57 + 3 * route] & 63)] += 1;
            source_destination_coverage[usize::from(body[0x56 + 3 * route] & 15)]
                [usize::from(body[0x57 + 3 * route] & 63)] += 1;
        }
        family_coverage[family as usize] += 1;
        for assignment in assignments {
            assignment_coverage[usize::from(assignment)] += 1;
        }
        receive_flag_coverage[usize::from(midi_receive_flags)] += 1;
        calls += 1;
    }
    let report = serde_json::json!({
        "passed": controller_errors == 0 && accumulator_errors == 0 && source_errors == 0 && clock_errors == 0 && transport_errors==0,
        "whole_original_SYS021832_calls": calls, "family_coverage": family_coverage,
        "controller_bytes_compared": calls * 496, "accumulator_longs_compared": calls * 41,
        "controller_errors": controller_errors, "accumulator_errors": accumulator_errors,
        "first_error": first_error, "source_coverage": source_coverage,
        "source_errors":source_errors,"first_source_error":first_source_error,
        "clock_errors":clock_errors,"first_clock_error":first_clock_error,
        "whole_and_each_source_route_target_work_compared":true,
        "clock_values_compared":calls*63,
        "transport_errors":transport_errors,"first_transport_error":first_transport_error,
        "whole_original_three_procedure_chains":family_coverage[2],
        "whole_original_E319_receives":receives,"parameter_words_compared":parameter_words,
        "native_preparation_work_then_descriptor_common_FIFO_used":true,
        "atomic_preparation_publication_rejection_cases":atomic_rejections,
        "destination_coverage": destination_coverage.to_vec(),
        "assignment_coverage": assignment_coverage.to_vec(),
        "receive_flag_coverage": receive_flag_coverage.to_vec(),
        "source_destination_pairs_covered":source_destination_coverage.iter().flatten().filter(|v|**v>0).count(),
        "whole_original_source_getter_calls":calls*16,
        "application_primary_then_Virtual_Patch_preparation_used":true,
        "six_previous_feedback_depths_and_all40_targets": true,
        "all16_sources_compiled_from_raw_actor_body_and_MIDI_assignment_ports": true,
        "whole_source_instruction_execution_unmodified": true,
        "preparation_only_r0_1_no_host_dispatch": true,
        "live_changed_value_compiler_dispatch_qualified": false,
        "whole_native_Virtual_Patch_functional_caller_work_qualified": clock_errors == 0,
        "whole_note_constructor_and_production_audio_qualified": false,
        "complete_native_engine": false,
    });
    fs::write(
        out.join("actor-virtual-patch-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if controller_errors + accumulator_errors + source_errors + clock_errors != 0 {
        return Err("Whole Virtual Patch preparation differs".into());
    }
    Ok(())
}
