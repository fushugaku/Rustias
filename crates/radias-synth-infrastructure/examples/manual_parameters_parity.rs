//! Whole motion/manual callbacks, native state/packets/work and common decoder.
use radias_synth_application::{
    dsp_transport::SendQueueError,
    manual_parameters::{ManualParameterError, ManualParameterRequest, apply_manual_parameter},
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_pitch_preparation::ActorPitchPorts,
    dsp_control::ParameterPacket,
    manual_parameters::{
        MANUAL_PARAMETER_OFFSETS, ManualCompilationError, ManualCompilerPorts, ManualCompilerTables,
    },
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
    for (index, offset) in MANUAL_PARAMETER_OFFSETS.iter().enumerate() {
        let p = 0x46550 + 0x1000 + 2 * index;
        if u16::from_be_bytes(sys[p..p + 2].try_into()?) != *offset {
            return Err("Native manual offset map differs from SYS".into());
        }
    }
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
    let raw = fs::read(out.join("manual-parameters-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4d414e31 {
        return Err("Unsupported manual observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut packet_errors,
        mut transport_errors,
        mut receives,
        mut parameter_words,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u64);
    let mut coverage = [0u32; 42];
    let (mut application_calls, mut atomic_rejections) = (0u32, 0u32);
    let mut first_state_error = None;
    let mut first_packet_error = None;
    let mut first_transport_error = None;
    let mut errors_by_parameter = [0u32; 42];
    let mut first_errors_by_parameter = vec![serde_json::Value::Null; 42];
    while cursor < raw.len() {
        let parameter = take(&raw, &mut cursor) as u8;
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
        let variant = take(&raw, &mut cursor);
        let value = take(&raw, &mut cursor) as i16;
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
        take(&raw, &mut cursor);
        take(&raw, &mut cursor);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
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
        let compiled = ActorControlState { bytes: before }
            .compile_manual_parameter(parameter, value, &body, ports, &tables)
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
            first_state_error.get_or_insert(serde_json::json!({"parameter":parameter,"variant":variant,"offset":offset,"native":compiled.controller.bytes[offset],"original":after[offset]}));
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
            first_packet_error.get_or_insert(serde_json::json!({"parameter":parameter,"variant":variant,"native":native_packets,"original":packets.iter().map(|(_,p,_)|p).collect::<Vec<_>>()}));
        }
        let slot = (local + 12 * chip) as usize;
        if calls == 0 {
            use ManualCompilationError::*;
            use ManualParameterError::{Compile, Transport};
            for case in 0..10 {
                let mut state = ActorControlState { bytes: before };
                let mut transport = SynthesisParameterTransport::default();
                if case != 8 {
                    transport.configure_constructor_filter_mix(
                        radias_synth_domain::filter_control::FilterMixTable {
                            weights: mix.weights,
                        },
                    );
                }
                if case != 9 {
                    transport.configure_pitch_receivers(pitch_rom.clone(), primary_pitch);
                }
                let (parameter, expected) = match case {
                    0 => (0, Compile(InvalidParameter)),
                    1 => (42, Compile(InvalidParameter)),
                    2 => {
                        state.bytes[0x1e0] = 6;
                        (3, Compile(UnsupportedPrimary))
                    }
                    3 => {
                        state.bytes[0x1e0] = 7;
                        (4, Compile(UnsupportedPrimary))
                    }
                    4 => {
                        state.bytes[0x1e3] = 2;
                        state.bytes[0x1e4] = 15;
                        (21, Compile(UnsupportedShaper))
                    }
                    5 => {
                        state.bytes[0x1ea] = 128;
                        (19, Compile(InvalidGainBank))
                    }
                    6 => (34, Transport(SendQueueError::InvalidSlot)),
                    7 => (34, Transport(SendQueueError::Full)),
                    8 => (12, Transport(SendQueueError::MissingFilterContext)),
                    _ => (5, Transport(SendQueueError::MissingPitchContext)),
                };
                if case == 7 {
                    for _ in 0..512 {
                        apply_manual_parameter(
                            ManualParameterRequest {
                                clock: 0,
                                slot,
                                parameter,
                                value,
                                body: &body,
                                ports,
                            },
                            &mut state,
                            &tables,
                            &mut transport,
                        )
                        .map_err(|e| format!("Queue preparation: {e:?}"))?;
                    }
                }
                let saved = state;
                let pending = transport.pending();
                let parameters = transport.parameter_state(slot);
                let result = apply_manual_parameter(
                    ManualParameterRequest {
                        clock: 0,
                        slot: if case == 6 { 24 } else { slot },
                        parameter,
                        value,
                        body: &body,
                        ports,
                    },
                    &mut state,
                    &tables,
                    &mut transport,
                );
                if result != Err(expected)
                    || state != saved
                    || transport.pending() != pending
                    || transport.parameter_state(slot) != parameters
                {
                    return Err(format!("Non-atomic manual rejection {case}: {result:?}").into());
                }
                atomic_rejections += 1;
            }
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
            let mut controller = ActorControlState { bytes: before };
            apply_manual_parameter(
                ManualParameterRequest {
                    clock: 0,
                    slot,
                    parameter,
                    value,
                    body: &body,
                    ports,
                },
                &mut controller,
                &tables,
                &mut transport,
            )
            .map_err(|e| format!("{e:?}"))?;
            if controller.bytes != after {
                return Err("Application controller differs".into());
            }
            application_calls += 1;
            let mut native_acks = Vec::new();
            let (mut clock, mut ready_at, mut seen_count, mut good) = (0, busy, 0, true);
            for (ack, payload, expected) in &packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{native_acks.push(time);seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));});
                }
                good &= seen == Some((*ack, slot, true))
                    && transport.parameter_state(slot) == *expected;
                if !good {
                    first_transport_error.get_or_insert(serde_json::json!({"parameter":parameter,"variant":variant,"partition":partition,"ack":ack,"seen":seen,"first_word":transport.parameter_state(slot).iter().zip(expected).position(|(a,b)|a!=b)}));
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
                errors_by_parameter[parameter as usize] += 1;
                if first_errors_by_parameter[parameter as usize].is_null() {
                    first_errors_by_parameter[parameter as usize] = serde_json::json!({"variant":variant,"partition":partition,"source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending(),"native_acks":native_acks,"source_acks":packets.iter().map(|(t,_,_)|t).collect::<Vec<_>>(),"native_plan":format!("{:?}",compiled.publication.operations())});
                }
                first_transport_error.get_or_insert(serde_json::json!({"parameter":parameter,"variant":variant,"partition":partition,"source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending()}));
            }
        }
        coverage[parameter as usize] += 1;
        receives += count;
        calls += 1;
    }
    let passed = state_errors + packet_errors + transport_errors == 0
        && coverage[1..] == [256; 41]
        && application_calls == 31488
        && atomic_rejections == 10;
    let report = serde_json::json!({"passed":passed,"whole_original_manual_calls":calls,"whole_original_E319_receives":receives,"parameter_words_compared":parameter_words,"controller_bytes_compared":u64::from(calls)*496,
  "state_errors":state_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,"procedure_coverage":coverage.to_vec(),"transport_errors_by_parameter":errors_by_parameter.to_vec(),"first_transport_errors_by_parameter":first_errors_by_parameter,"first_state_error":first_state_error,"first_packet_error":first_packet_error,"first_transport_error":first_transport_error,
  "source_outputs_used_only_for_assertions":true,"application_calls":application_calls,"atomic_rejection_cases":atomic_rejections,"PCM_and_external_input_special_CTRL2_paths_qualified":false,"whole_motion_note_initialization_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("manual-parameters-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Whole native manual callbacks differ".into());
    }
    Ok(())
}
