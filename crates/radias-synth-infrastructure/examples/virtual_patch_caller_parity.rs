//! Whole SYS021832(r0=0) through native DDD compilation and the common FIFO.
use radias_synth_application::{
    dsp_transport::SendQueueError,
    live_modulation::{LiveModulationError, publish_live_virtual_patches},
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    dsp_control::ParameterPacket,
    virtual_patch_live::{
        LiveCompilationError, LiveCompilerPorts, LiveCompilerTables, LiveVirtualPatchRequest,
    },
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn set_word(state: &mut ActorControlState, offset: usize, value: i16) {
    state.bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}
fn set_long(state: &mut ActorControlState, offset: usize, value: i32) {
    state.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
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
    let modulation = firmware::modulation_tables(&sys)?;
    let tables = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let mix = master.filter_mix()?;
    let raw = fs::read(out.join("virtual-patch-caller-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x56504331 {
        return Err("Unsupported caller observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut accumulator_errors,
        mut packet_errors,
        mut transport_errors,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut receives, mut parameter_words) = (0u32, 0u64);
    let mut first_error = None;
    let mut first_transport_error = None;
    let mut atomic_rejections = 0;
    let mut source_destination = [[0u32; 40]; 16];
    let mut stages = [[0u32; 8]; 3];
    let mut assignments_seen = [0u32; 256];
    let mut flags_seen = [0u32; 256];
    while cursor < raw.len() {
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
        let bend = take(&raw, &mut cursor) as i16;
        let wheel = take(&raw, &mut cursor) as u8;
        let auxiliary = take(&raw, &mut cursor) as i16;
        let midi_receive_flags = take(&raw, &mut cursor) as u8;
        let mut assignments = [0u8; 5];
        let mut assignable_values = [0i8; 5];
        for index in 0..5 {
            assignments[index] = take(&raw, &mut cursor) as u8;
            assignable_values[index] = take(&raw, &mut cursor) as i8;
            assignments_seen[usize::from(assignments[index])] += 1;
        }
        let portamento_time = take(&raw, &mut cursor) as u8;
        let portamento_switch_required = take(&raw, &mut cursor) != 0;
        let portamento_switch = take(&raw, &mut cursor) != 0;
        let midi_pan = take(&raw, &mut cursor).checked_sub(1).map(|v| v as u8);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
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
        let request = LiveVirtualPatchRequest {
            body: &body,
            sources: ActorVirtualPatchPorts {
                bend,
                wheel,
                auxiliary,
                midi_receive_flags,
                assignments,
                assignable_values,
            },
            compilers: LiveCompilerPorts {
                portamento_time,
                portamento_switch_required,
                portamento_switch,
                midi_pan,
            },
        };
        let prior = ActorControlState { bytes: before };
        if calls == 0 {
            let test_slot = (local + 12 * chip) as usize;
            for case in 0..7 {
                let mut silent = body;
                let mut state = prior;
                set_long(&mut state, 0xa4, 0);
                set_long(&mut state, 0xa8, 0);
                for offset in (0x116..=0x162).step_by(2) {
                    set_word(&mut state, offset, 0);
                }
                for route in 0..6 {
                    silent[88 + 3 * route] = 64;
                    state.bytes[0x1c4 + 2 * route] = 0;
                }
                let mut transport = SynthesisParameterTransport::default();
                if case != 0 {
                    transport.configure_constructor_filter_mix(
                        radias_synth_domain::filter_control::FilterMixTable {
                            weights: mix.weights,
                        },
                    );
                }
                if case != 1 {
                    transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
                }
                let expected = match case {
                    0 => {
                        set_word(&mut state, 0x120, 1);
                        LiveModulationError::Publish(SendQueueError::MissingFilterContext)
                    }
                    1 => {
                        set_long(&mut state, 0xa8, 256);
                        LiveModulationError::Publish(SendQueueError::MissingPitchContext)
                    }
                    2 => {
                        silent[86] = 5;
                        silent[87] = 63;
                        silent[88] = 127;
                        state.bytes[0x37] = 100;
                        LiveModulationError::Compile(LiveCompilationError::VirtualPatch(
                            ActorVirtualPatchError::InvalidDestination(63),
                        ))
                    }
                    3 => {
                        state.bytes[0x1e0] = 6;
                        set_word(&mut state, 0x11a, 1);
                        LiveModulationError::Compile(LiveCompilationError::UnsupportedPrimary)
                    }
                    4 => {
                        state.bytes[0x1e3] = 2;
                        state.bytes[0x1e4] = 15;
                        set_word(&mut state, 0x128, 1);
                        LiveModulationError::Compile(LiveCompilationError::UnsupportedShaper)
                    }
                    5 => LiveModulationError::Publish(SendQueueError::InvalidSlot),
                    _ => LiveModulationError::Publish(SendQueueError::Full),
                };
                let rejected_request = LiveVirtualPatchRequest {
                    body: &silent,
                    ..request
                };
                if case == 6 {
                    for _ in 0..512 {
                        publish_live_virtual_patches(
                            0,
                            test_slot,
                            &mut state,
                            rejected_request,
                            &tables,
                            &modulation,
                            &mut transport,
                        )
                        .map_err(|e| format!("FIFO preparation failed: {e:?}"))?;
                    }
                }
                let saved = state;
                let pending = transport.pending();
                let result = publish_live_virtual_patches(
                    0,
                    if case == 5 { 24 } else { test_slot },
                    &mut state,
                    rejected_request,
                    &tables,
                    &modulation,
                    &mut transport,
                );
                if result != Err(expected) || state != saved || transport.pending() != pending {
                    return Err(format!("Non-atomic live rejection {case}: {result:?}").into());
                }
                atomic_rejections += 1;
            }
        }
        let compiled = prior
            .compile_live_virtual_patches(request, &tables, &modulation)
            .map_err(|e| format!("{e:?}"))?;
        if compiled.controller.bytes != after {
            state_errors += 1;
            let offset = compiled
                .controller
                .bytes
                .iter()
                .zip(&after)
                .position(|(a, b)| a != b)
                .unwrap();
            first_error.get_or_insert(serde_json::json!({"call":calls,"variant":variant,"offset":offset,"native":compiled.controller.bytes[offset],"original":after[offset]}));
        }
        if compiled.targets.values != targets || compiled.targets.linked_pitch != linked {
            accumulator_errors += 1;
        }
        let native_packets: Vec<Vec<u16>> = compiled
            .publication
            .operations()
            .iter()
            .filter_map(|operation| match *operation {
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
        let slot = (local + 12 * chip) as usize;
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
            transport.restore_parameters(slot, before_dsp);
            let mut controller = prior;
            let native_targets = publish_live_virtual_patches(
                0,
                slot,
                &mut controller,
                request,
                &tables,
                &modulation,
                &mut transport,
            )
            .map_err(|e| format!("{e:?}"))?;
            let (mut clock, mut ready_at, mut seen_count, mut good) = (
                0,
                busy,
                0,
                controller.bytes == after
                    && native_targets.values == targets
                    && native_targets.linked_pitch == linked,
            );
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
                if !good {
                    first_transport_error.get_or_insert(serde_json::json!({"call":calls,"variant":variant,"partition":partition,"ack":ack,"seen":seen,
                    "first_word":transport.parameter_state(slot).iter().zip(expected).position(|(a,b)|a!=b)}));
                }
                ready_at = *ack + busy;
                parameter_words += 160;
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= seen_count == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                transport_errors += 1;
                first_transport_error.get_or_insert(serde_json::json!({"call":calls,"variant":variant,"partition":partition,
                "source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending()}));
            }
        }
        for route in 0..6 {
            source_destination[usize::from(body[86 + 3 * route] & 15)]
                [usize::from(body[87 + 3 * route] & 63)] += 1;
        }
        for e in 0..3 {
            stages[e][usize::from(before[0x94 + e])] += 1;
        }
        flags_seen[usize::from(midi_receive_flags)] += 1;
        receives += count;
        calls += 1;
    }
    let report = serde_json::json!({"passed":state_errors+accumulator_errors+packet_errors+transport_errors==0,
        "whole_original_live_Virtual_Patch_calls":calls,"whole_original_E319_receives":receives,"controller_bytes_compared":u64::from(calls)*496,
        "accumulator_longs_compared":u64::from(calls)*41,"parameter_words_compared":parameter_words,
        "state_errors":state_errors,"accumulator_errors":accumulator_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"atomic_rejection_cases":atomic_rejections,
        "first_error":first_error,"first_transport_error":first_transport_error,"envelope_stages":stages,"MIDI_assignments":assignments_seen.to_vec(),
        "MIDI_receive_flags":flags_seen.to_vec(),"source_destination_pairs_covered":source_destination.iter().flatten().filter(|c|**c>0).count(),
        "whole_live_source_getter_route_changed_callback_and_common_FIFO_used":true,"functional_caller_return_and_all_ACKs_checked":true,
        "source_outputs_used_only_for_comparison":true,"hardware_IRQ_DMA_and_sample_job_timing_qualified":false,
        "full_native_note_constructor_enabled":false,"whole_original_production_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("virtual-patch-caller-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if state_errors + accumulator_errors + packet_errors + transport_errors != 0 {
        return Err("Whole live Virtual Patch differs".into());
    }
    Ok(())
}
