//! Whole SYS014f40 native compilation, atomic FIFO and unchanged E319 results.
use radias_synth_application::{
    dsp_transport::SendQueueError,
    note_refresh::{NoteRefreshPublication, NoteRefreshPublicationError, refresh_note_controls},
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_amplifier_preparation::{ActorAmplifierPorts, InvalidUnisonGainBank},
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorOperation,
    actor_lfo_initialization::{ActorLfoState, InvalidLfoShape},
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    dsp_control::ParameterPacket,
    note_refresh::{NoteRefreshError, NoteRefreshPorts, NoteRefreshRequest, NoteRefreshTables},
    virtual_patch_live::{LiveCompilationError, LiveCompilerPorts, LiveCompilerTables},
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn set_word(state: &mut ActorControlState, offset: usize, value: i16) {
    state.bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}
fn set_long(state: &mut ActorControlState, offset: usize, value: i32) {
    state.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
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
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let modulation = firmware::modulation_tables(&sys)?;
    let lfo = firmware::lfo_tables(&sys)?;
    let compilers = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let tables = NoteRefreshTables {
        lfo: &lfo,
        compilers: &compilers,
        modulation: &modulation,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let mix = master.filter_mix()?;
    let raw = fs::read(out.join("note-refresh-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4e524631 {
        return Err("Unsupported note refresh observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut accumulator_errors,
        mut packet_errors,
        mut transport_errors,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut receives, mut parameter_words, mut atomic_rejections) = (0u32, 0u64, 0u32);
    let (mut first_error, mut first_transport_error) = (None, None);
    let mut slots = [[0u32; 12]; 2];
    let mut stages = [[0u32; 8]; 3];
    let mut policies = [0u32; 4];
    let mut source_destination = [[0u32; 40]; 16];
    while cursor < raw.len() {
        let variant = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = u64::from(take(&raw, &mut cursor));
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
        let portamento_time = take(&raw, &mut cursor) as u8;
        let portamento_switch = take(&raw, &mut cursor) != 0;
        let midi_pan = take(&raw, &mut cursor) as u8;
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
        if before[0x36] > 127 || before[0x37] > 127 {
            return Err("Note refresh requires decoded MIDI note/velocity fields".into());
        }
        let lfos = pair(&raw, &mut cursor);
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let expected_lfos = pair(&raw, &mut cursor);
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
        let ports = NoteRefreshPorts {
            virtual_patch: ActorVirtualPatchPorts {
                bend,
                wheel,
                auxiliary,
                midi_receive_flags: owner_receive_flags,
                assignments,
                assignable_values,
            },
            live_compilers: LiveCompilerPorts {
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
        let request = NoteRefreshRequest {
            body: &body,
            lfos,
            ports,
        };
        let prior = ActorControlState { bytes: before };
        let slot = (local + 12 * chip) as usize;
        if calls == 0 {
            use NoteRefreshPublicationError::{Compile, Transport};
            for case in 0..9 {
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
                        Transport(SendQueueError::MissingFilterContext)
                    }
                    1 => {
                        set_long(&mut state, 0xa8, 256);
                        Transport(SendQueueError::MissingPitchContext)
                    }
                    2 => {
                        silent[86..89].copy_from_slice(&[5, 63, 127]);
                        state.bytes[0x37] = 100;
                        Compile(NoteRefreshError::Live(LiveCompilationError::VirtualPatch(
                            ActorVirtualPatchError::InvalidDestination(63),
                        )))
                    }
                    3 => {
                        state.bytes[0x1e0] = 6;
                        set_word(&mut state, 0x11a, 1);
                        Compile(NoteRefreshError::Live(
                            LiveCompilationError::UnsupportedPrimary,
                        ))
                    }
                    4 => {
                        state.bytes[0x1e3] = 2;
                        state.bytes[0x1e4] = 15;
                        set_word(&mut state, 0x128, 1);
                        Compile(NoteRefreshError::Live(
                            LiveCompilationError::UnsupportedShaper,
                        ))
                    }
                    5 => Transport(SendQueueError::InvalidSlot),
                    6 => Transport(SendQueueError::Full),
                    7 => {
                        silent[77] = 128;
                        Compile(NoteRefreshError::LfoShape(InvalidLfoShape))
                    }
                    _ => {
                        state.bytes[0x1ea] = 128;
                        Compile(NoteRefreshError::GainBank(InvalidUnisonGainBank))
                    }
                };
                let rejected = NoteRefreshRequest {
                    body: &silent,
                    ..request
                };
                if case == 6 {
                    for _ in 0..512 {
                        refresh_note_controls(
                            NoteRefreshPublication {
                                clock: 0,
                                slot,
                                controls: rejected,
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
                let result = refresh_note_controls(
                    NoteRefreshPublication {
                        clock: 0,
                        slot: if case == 5 { 24 } else { slot },
                        controls: rejected,
                    },
                    &mut state,
                    &tables,
                    &mut transport,
                );
                if result != Err(expected) || state != saved || transport.pending() != pending {
                    return Err(
                        format!("Non-atomic note refresh rejection {case}: {result:?}").into(),
                    );
                }
                atomic_rejections += 1;
            }
        }
        let compiled = prior
            .compile_note_refresh(request, &tables)
            .map_err(|e| format!("{e:?}"))?;
        if compiled.controller.bytes != after || lfos != expected_lfos {
            state_errors += 1;
            let offset = compiled
                .controller
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b);
            first_error.get_or_insert(serde_json::json!({"variant":variant,"actor_offset":offset,"native":offset.map(|i|compiled.controller.bytes[i]),"original":offset.map(|i|after[i])}));
        }
        if compiled.modulations.values != targets || compiled.modulations.linked_pitch != linked {
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
            refresh_note_controls(
                NoteRefreshPublication {
                    clock: 0,
                    slot,
                    controls: request,
                },
                &mut controller,
                &tables,
                &mut transport,
            )
            .map_err(|e| format!("{e:?}"))?;
            let (mut clock, mut ready_at, mut seen_count, mut good) =
                (0, busy, 0, controller.bytes == after);
            for (ack, payload, expected) in &packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));});
                }
                good &= seen == Some((*ack, slot, true))
                    && transport.parameter_state(slot) == *expected;
                if !good {
                    first_transport_error.get_or_insert(serde_json::json!({"variant":variant,"partition":partition,"ack":ack,"seen":seen,"first_word":transport.parameter_state(slot).iter().zip(expected).position(|(a,b)|a!=b)}));
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
                first_transport_error.get_or_insert(serde_json::json!({"variant":variant,"partition":partition,"native_return":transport.caller_available_clock(),"source_return":returned,"pending":transport.pending()}));
            }
        }
        slots[chip as usize][local as usize] += 1;
        policies[[0, 64, 333, 3000].iter().position(|v| *v == busy).unwrap()] += 1;
        for e in 0..3 {
            stages[e][before[0x94 + e] as usize] += 1;
        }
        for route in 0..6 {
            source_destination[(body[86 + 3 * route] & 15) as usize]
                [(body[87 + 3 * route] & 63) as usize] += 1;
        }
        receives += count;
        calls += 1;
    }
    let passed = calls == 2048
        && state_errors + accumulator_errors + packet_errors + transport_errors == 0
        && atomic_rejections == 9
        && slots.iter().flatten().all(|v| *v != 0)
        && stages.iter().flatten().all(|v| *v != 0)
        && policies.iter().all(|v| *v != 0)
        && source_destination.iter().flatten().all(|v| *v != 0);
    let report = serde_json::json!({"passed":passed,"whole_original_SYS014f40_calls":calls,"whole_original_E319_receives":receives,
        "voice_bytes_compared":u64::from(calls)*560,"accumulator_words_compared":u64::from(calls)*41,"parameter_words_compared":parameter_words,
        "state_errors":state_errors,"accumulator_errors":accumulator_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,
        "atomic_rejection_cases":atomic_rejections,"first_error":first_error,"first_transport_error":first_transport_error,
        "DSP_and_slot_coverage":slots,"envelope_stages":stages,"readiness_policy_coverage":policies,"source_destination_pairs_covered":source_destination.iter().flatten().filter(|v|**v!=0).count(),
        "full_caller_state_each_ACK_packet_parameter_bank_and_return_checked":true,"source_outputs_used_only_for_assertions":true,
        "decoded_MIDI_notes_and_velocities_in_0_127_domain":true,
        "full_SYS01ee38_and_SYS01e838_qualified":false,"independent_IRQ_DMA_sample_job_and_whole_production_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("note-refresh-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native whole note refresh differs or coverage incomplete".into());
    }
    Ok(())
}
