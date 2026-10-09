//! Whole SYS01eee0 startup plus complete scalar callers, all DSP actor/frame banks.
use radias_synth_application::{
    complete_actor_startup::{CompleteStartupPublicationError, start_complete_actor},
    dsp_transport::SendQueueError,
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_lifecycle::ActorLifecycle,
    complete_actor_startup::{CompleteStartupError, CompleteStartupTables},
    dsp_control::ParameterPacket,
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn banks(raw: &[u8], cursor: &mut usize) -> Vec<u16> {
    (0..2688).map(|_| take(raw, cursor) as u16).collect()
}
fn restored(transport: &mut SynthesisParameterTransport, chip: usize, state: &[u16]) {
    for slot in 0..12 {
        let parameters = state[slot * 160..slot * 160 + 160].try_into().unwrap();
        let physical = state[1920 + slot * 64..1920 + slot * 64 + 64]
            .try_into()
            .unwrap();
        transport.restore_construction_state(chip * 12 + slot, parameters, physical);
    }
}
fn state(transport: &SynthesisParameterTransport, chip: usize) -> Vec<u16> {
    let mut words = Vec::with_capacity(2688);
    for slot in 0..12 {
        words.extend(transport.parameter_state(chip * 12 + slot));
    }
    for slot in 0..12 {
        words.extend(transport.physical_parameter_state(chip * 12 + slot));
    }
    words
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let mix = master.filter_mix()?;
    let descriptors = firmware::parameter_template_tables(&sys, master.filter_mix()?)?;
    let phases = firmware::physical_phase_tables(&sys)?;
    let callbacks = firmware::phase_callback_tables(&sys)?;
    let counters = firmware::formant_counter_seeds(&sys)?;
    let pitch = firmware::primary_pitch_sender_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let amplifier = firmware::amplifier_rate_table(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let tables = CompleteStartupTables {
        descriptors: &descriptors,
        phases: &phases,
        callbacks: &callbacks,
        counters: &counters,
        pitch,
        pan: &pan,
        amplifier: &amplifier,
        timing: &timing,
    };
    let raw = fs::read(out.join("complete-actor-startup-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x53414c31 {
        return Err("Unsupported complete startup observation".into());
    }
    let (
        mut calls,
        mut controller_errors,
        mut lifecycle_errors,
        mut packet_errors,
        mut transport_errors,
        mut receives,
        mut atomic_rejections,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let mut bank_words = 0u64;
    let mut coverage = [0u32; 21];
    let mut errors = [0u32; 21];
    let mut first_errors = vec![serde_json::Value::Null; 21];
    let mut max_operations = 0;
    let mut whole_combinations = [[0u32; 13]; 24];
    let mut slots = [0u32; 24];
    while cursor < raw.len() {
        let procedure = take(&raw, &mut cursor) as usize;
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let busy = u64::from(take(&raw, &mut cursor));
        let active = take(&raw, &mut cursor);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp = banks(&raw, &mut cursor);
        let returned = u64::from(take(&raw, &mut cursor));
        let expected_active = take(&raw, &mut cursor);
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let count = take(&raw, &mut cursor);
        let mut packets = Vec::new();
        for _ in 0..count {
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let bank = banks(&raw, &mut cursor);
            packets.push((ack, payload, bank));
        }
        let prior = ActorControlState { bytes: before };
        let slot = local + 12 * chip;
        let compiled = prior
            .compile_complete_startup(
                procedure as u8,
                slot,
                &body,
                ActorLifecycle { active },
                &tables,
            )
            .map_err(|e| format!("Procedure{procedure} variant{variant}: {e:?}"))?;
        max_operations = max_operations.max(compiled.publication.operations().len());
        let changed = compiled.controller.bytes != after;
        controller_errors += u32::from(changed);
        lifecycle_errors += u32::from(compiled.lifecycle.active != expected_active);
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
                        0x2000 + 160 * local as u32 + u32::from(offset),
                        value,
                    )
                    .unwrap()
                    .words()
                    .to_vec(),
                ),
                _ => None,
            })
            .collect();
        let packet_error = native_packets
            != packets
                .iter()
                .map(|(_, p, _)| p.clone())
                .collect::<Vec<_>>();
        packet_errors += u32::from(packet_error);
        if changed || packet_error || compiled.lifecycle.active != expected_active {
            let offset = compiled
                .controller
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b);
            if first_errors[procedure].is_null() {
                first_errors[procedure] = serde_json::json!({"variant":variant,"controller_offset":offset,"native":offset.map(|i|compiled.controller.bytes[i]),"original":offset.map(|i|after[i]),"native_packets":native_packets,"original_packets":packets.iter().map(|(_,p,_)|p).collect::<Vec<_>>(),"native_active":compiled.lifecycle.active,"original_active":expected_active});
            }
        }
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), pitch);
            transport.set_actor_lifecycle(ActorLifecycle { active });
            restored(&mut transport, chip, &before_dsp);
            if procedure == 0 {
                let mut candidate = prior;
                start_complete_actor(0, slot, &body, &mut candidate, &tables, &mut transport)
                    .map_err(|e| format!("{e:?}"))?;
                if candidate != compiled.controller
                    || transport.actor_lifecycle() != compiled.lifecycle
                {
                    return Err("Startup application commit differs".into());
                }
            } else {
                transport
                    .publish_actor_descriptors(0, slot, &compiled.publication)
                    .map_err(|e| format!("{e:?}"))?;
            }
            let (mut clock, mut ready_at, mut seen_count, mut good) = (0, busy, 0, true);
            let mut native_acks = Vec::new();
            let mut first_word = None;
            for (ack, payload, expected) in &packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{seen_count+=1;native_acks.push(time);seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..}if opcode==payload[1])));});
                }
                let actual = state(&transport, chip);
                let different = actual.iter().zip(expected).position(|(a, b)| a != b);
                if first_word.is_none() {
                    first_word = different;
                }
                good &= seen == Some((*ack, slot, true)) && different.is_none();
                ready_at = *ack + busy;
                bank_words += 2688;
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= seen_count == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                transport_errors += 1;
                errors[procedure] += 1;
                if first_errors[procedure].is_null() {
                    first_errors[procedure] = serde_json::json!({"variant":variant,"partition":partition,"original_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending(),"first_bank_word":first_word,"native_acks":native_acks,"source_acks":packets.iter().map(|(t,_,_)|t).collect::<Vec<_>>(),"native_plan":format!("{:?}",compiled.publication.operations())});
                }
            }
        }
        if calls == 0 {
            for case in 0..5 {
                let mut body = body;
                let mut controller = prior;
                let mut transport = SynthesisParameterTransport::default();
                transport.set_actor_lifecycle(ActorLifecycle { active });
                if case != 0 {
                    transport.configure_constructor_filter_mix(
                        radias_synth_domain::filter_control::FilterMixTable {
                            weights: mix.weights,
                        },
                    );
                }
                if case != 1 {
                    transport.configure_pitch_receivers(pitch_rom.clone(), pitch);
                }
                let expected = match case {
                    0 => CompleteStartupPublicationError::Transport(
                        SendQueueError::MissingFilterContext,
                    ),
                    1 => CompleteStartupPublicationError::Transport(
                        SendQueueError::MissingPitchContext,
                    ),
                    2 => {
                        CompleteStartupPublicationError::Compile(CompleteStartupError::InvalidSlot)
                    }
                    3 => {
                        controller.bytes[0x1e4] = 15;
                        controller.bytes[0x1e3] = 2;
                        body[46] = 2;
                        CompleteStartupPublicationError::Compile(CompleteStartupError::Descriptor(radias_synth_domain::parameter_template::TemplateCompilationError::UnsupportedShaperSubtype))
                    }
                    _ => CompleteStartupPublicationError::Transport(SendQueueError::Full),
                };
                if case == 4 {
                    let mut idle = prior;
                    idle.bytes[0x1e2] = 0;
                    let plan = idle
                        .compile_comb_pointer_publication()
                        .map_err(|e| format!("{e:?}"))?;
                    for _ in 0..512 {
                        transport
                            .publish_actor_descriptors(0, slot, &plan.publication)
                            .map_err(|e| format!("{e:?}"))?;
                    }
                }
                let (saved, pending, lifecycle) =
                    (controller, transport.pending(), transport.actor_lifecycle());
                let result = start_complete_actor(
                    0,
                    if case == 2 { 24 } else { slot },
                    &body,
                    &mut controller,
                    &tables,
                    &mut transport,
                );
                if result != Err(expected)
                    || controller != saved
                    || transport.pending() != pending
                    || transport.actor_lifecycle() != lifecycle
                {
                    return Err(format!("Non-atomic startup rejection{case}: {result:?}").into());
                }
                atomic_rejections += 1;
            }
        }
        if procedure == 0 {
            let primary =
                usize::from(before[0x1e0] & 15) * 4 + usize::from((before[0x1e0] >> 4) & 3);
            let shaper = if before[0x1e3] & 3 == 0 {
                0
            } else if before[0x1e3] & 3 == 1 {
                1
            } else {
                2 + usize::from(before[0x1e4] & 15)
            };
            whole_combinations[primary][shaper] += 1;
            slots[slot] += 1;
        }
        calls += 1;
        receives += count;
        coverage[procedure] += 1;
    }
    let passed = controller_errors + lifecycle_errors + packet_errors + transport_errors == 0
        && coverage[0] == 2496
        && coverage[1..] == [576; 20]
        && atomic_rejections == 5
        && whole_combinations.iter().all(|r| r.iter().all(|c| *c > 0))
        && slots.iter().all(|c| *c > 0);
    let report = serde_json::json!({"passed":passed,"whole_original_startup_and_callee_calls":calls,"whole_SYS01eee0_calls":coverage[0],"whole_original_E319_receives":receives,"controller_bytes_compared":u64::from(calls)*496,"all_actor_and_physical_bank_words_compared":bank_words,"procedure_coverage":coverage.to_vec(),"transport_errors_by_procedure":errors.to_vec(),"first_errors_by_procedure":first_errors,"max_publication_operations":max_operations,"atomic_rejection_cases":atomic_rejections,"all24_primary_and13_WS_modes_joint_input_combinations_covered":whole_combinations.iter().all(|r|r.iter().all(|c|*c>0)),"all24_actor_slots_covered":slots.iter().all(|c|*c>0),"controller_errors":controller_errors,"lifecycle_errors":lifecycle_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"source_outputs_used_only_for_assertions":true,"whole_SYS01e838_constructor_qualified":false,"independent_whole_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("complete-actor-startup-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Whole native startup differs".into());
    }
    Ok(())
}
