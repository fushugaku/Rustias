//! Complete SYS01c8d8 state, nine-word sender, HPI polls and E319 results.
use radias_synth_application::{
    comb_pointer_publication::{CombPointerPublicationError, publish_comb_pointers},
    dsp_transport::{HpiAction, SendQueueError, TimedParameterTransfer},
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    comb_pointer_publication::InvalidCombControllerSlot,
    dsp_control::{DspEndpoint, ParameterPacket},
};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("comb-pointer-publication-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x43505431 {
        return Err("Unsupported Comb pointer observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut packet_errors,
        mut transport_errors,
        mut sender_errors,
        mut receives,
        mut atomic_rejections,
        mut parameter_words,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u64);
    let mut coverage = [0u32; 3];
    let mut flags = [0u32; 256];
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let source_entry = u64::from(take(&raw, &mut cursor));
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
            let state: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((ack, payload, state));
        }
        let actions: Vec<[u32; 4]> = (0..take(&raw, &mut cursor))
            .map(|_| core::array::from_fn(|_| take(&raw, &mut cursor)))
            .collect();
        let polls: Vec<u64> = (0..take(&raw, &mut cursor))
            .map(|_| u64::from(take(&raw, &mut cursor)))
            .collect();
        let prior = ActorControlState { bytes: before };
        let compiled = prior
            .compile_comb_pointer_publication()
            .map_err(|e| format!("{e:?}"))?;
        let state_error = compiled.controller.bytes != after;
        state_errors += u32::from(state_error);
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
        let packet_error = native_packets
            != packets
                .iter()
                .map(|(_, p, _)| p.clone())
                .collect::<Vec<_>>();
        packet_errors += u32::from(packet_error);
        let mut sender_error = false;
        if count != 0 {
            let DescriptorOperation::Send {
                sender,
                offset,
                value,
                before,
                after: finish,
            } = compiled.publication.operations()[0]
            else {
                return Err("Missing native sender".into());
            };
            let origin = u64::from(before);
            let mut transfer = TimedParameterTransfer::from_sender(
                if chip == 0 {
                    DspEndpoint::Master
                } else {
                    DspEndpoint::Slave
                },
                sender,
                0x2000 + 160 * local + u32::from(offset),
                value,
                origin,
            )
            .ok_or("Missing multi-value sender")?;
            let mut native_actions = Vec::new();
            let mut native_polls = Vec::new();
            let mut last = 0;
            while let Some(time) = transfer.next_clock() {
                last = time;
                let port = if transfer.polling() {
                    native_polls.push(time);
                    if time < busy { 1 << chip } else { 0 }
                } else {
                    0
                };
                if let Some(action) = transfer.advance(port) {
                    let (offset, bits, value) = match action {
                        HpiAction::AddressByte { offset, value, .. } => {
                            (offset, 8, u32::from(value))
                        }
                        HpiAction::DataWord { offset, value, .. } => (offset, 16, u32::from(value)),
                        HpiAction::AcknowledgeHint { value, .. } => (0, 16, u32::from(value)),
                    };
                    native_actions.push([time as u32, u32::from(offset), bits, value]);
                }
            }
            sender_error = origin != source_entry
                || native_actions != actions
                || native_polls != polls
                || last + u64::from(finish) != returned;
        } else {
            sender_error |= !actions.is_empty() || !polls.is_empty();
        }
        sender_errors += u32::from(sender_error);
        let slot = (local + 12 * chip) as usize;
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.restore_parameters(slot, before_dsp);
            let mut controller = prior;
            publish_comb_pointers(0, slot, &mut controller, &mut transport)
                .map_err(|e| format!("{e:?}"))?;
            let (mut clock, mut ready_at, mut seen_count, mut good) = (0, busy, 0, true);
            for (ack, payload, expected) in &packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..}if opcode==payload[1])));});
                }
                good &= seen == Some((*ack, slot, true))
                    && transport.parameter_state(slot) == *expected;
                ready_at = *ack + busy;
                parameter_words += 160;
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= controller.bytes == after
                && seen_count == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                transport_errors += 1;
                first_error.get_or_insert(serde_json::json!({"chip":chip,"variant":variant,"partition":partition,"original_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending()}));
            }
        }
        if state_error || packet_error || sender_error {
            first_error.get_or_insert(serde_json::json!({"chip":chip,"variant":variant,"state_error":state_error,"packet_error":packet_error,"sender_error":sender_error,"native":native_packets,"original":packets.iter().map(|(_,p,_)|p).collect::<Vec<_>>()}));
        }
        if calls == 0 {
            for case in 0..3 {
                let mut state = prior;
                state.bytes[0x1e2] = 0x31;
                state.bytes[0x34] = if case == 0 { 24 } else { 0 };
                let mut transport = SynthesisParameterTransport::default();
                let expected = match case {
                    0 => CombPointerPublicationError::Compile(InvalidCombControllerSlot),
                    1 => CombPointerPublicationError::Transport(SendQueueError::InvalidSlot),
                    _ => CombPointerPublicationError::Transport(SendQueueError::Full),
                };
                if case == 2 {
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
                let saved = state;
                let pending = transport.pending();
                let parameters = transport.parameter_state(slot);
                let result = publish_comb_pointers(
                    0,
                    if case == 1 { 24 } else { slot },
                    &mut state,
                    &mut transport,
                );
                if result != Err(expected)
                    || state != saved
                    || transport.pending() != pending
                    || transport.parameter_state(slot) != parameters
                {
                    return Err(
                        format!("Non-atomic Comb pointer rejection {case}: {result:?}").into(),
                    );
                }
                atomic_rejections += 1;
            }
        }
        coverage[if before[0x1e2] & 3 == 0 {
            0
        } else if before[0x1e2] & 0x30 != 0x30 {
            1
        } else {
            2
        }] += 1;
        flags[usize::from(before[0x1e2])] += 1;
        calls += 1;
        receives += count;
    }
    let passed = state_errors + packet_errors + transport_errors + sender_errors == 0
        && calls == 8192
        && receives == 1536
        && coverage == [2048, 4608, 1536]
        && flags.iter().all(|n| *n == 32)
        && atomic_rejections == 3;
    let report = serde_json::json!({"passed":passed,"whole_original_comb_pointer_calls":calls,"whole_original_E319_receives":receives,"controller_bytes_compared":u64::from(calls)*496,"parameter_words_compared":parameter_words,"branch_coverage":coverage,"all256_cached_filter_flags_covered":flags.iter().all(|n|*n==32),"atomic_rejection_cases":atomic_rejections,"state_errors":state_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"sender_action_and_poll_errors":sender_errors,"all_nine_packet_words_and_twelve_HPI_actions_compared":true,"first_error":first_error,"source_outputs_used_only_for_assertions":true,"whole_actor_startup_SYS01eee0_qualified":false,"whole_actor_constructor_SYS01e838_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("comb-pointer-publication-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Complete native Comb pointer publication differs".into());
    }
    Ok(())
}
