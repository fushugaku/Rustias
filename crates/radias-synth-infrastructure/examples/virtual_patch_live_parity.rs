//! Changed-value dispatch and complete native leaf controller calculations.
use radias_synth_application::{
    live_modulation::publish_live_destination,
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    virtual_patch_live::{
        LiveCompilerPorts, LiveCompilerTables, LiveDestinationRequest, LiveModulationCompiler,
    },
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn compiler_entry(compiler: LiveModulationCompiler) -> u32 {
    use LiveModulationCompiler::*;
    match compiler {
        SecondaryPitch => 0xc01f74c,
        PrimaryMixer => 0xc0026a6,
        SecondaryMixer => 0xc00278e,
        NoiseMixer => 0xc002872,
        FilterMix => 0xc01c1bc,
        Filter1Resonance => 0xc01c4a0,
        ShaperDepth => 0xc002dd4,
        Pan => 0xc0025fc,
        Portamento => 0xc01fb02,
        Filter1EnvelopeIntensity => 0xc01ba08,
        Filter1KeyTracking => 0xc01ba60,
        Filter2Resonance => 0xc01c7d4,
        Filter2EnvelopeIntensity => 0xc01bf00,
        Filter2KeyTracking => 0xc01bf5c,
        EnvelopeParameter {
            envelope,
            parameter,
        } => [
            [0xc0151bc, 0xc01528e, 0xc013f14, 0xc015366],
            [0xc0155c2, 0xc015660, 0xc01458a, 0xc0156fc],
            [0xc015880, 0xc01594e, 0xc014a22, 0xc015a22],
        ][envelope as usize][parameter as usize],
    }
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
    let filter1 = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let tables = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &filter1,
        comb: &comb,
        portamento: &portamento,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let pitch_dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let mix = master.filter_mix()?;
    let raw = fs::read(out.join("virtual-patch-live-original.bin"))?;
    let (
        mut cursor,
        mut calls,
        mut dispatch_errors,
        mut complete_state_errors,
        mut coefficient_errors,
        mut completed,
    ) = (4, 0, 0, 0, 0, 0);
    if u32::from_le_bytes(raw[..4].try_into()?) != 0x56504c32 {
        return Err("Unsupported live observation format".into());
    }
    let mut first_error = None;
    let mut first_transport_error = None;
    let (mut transport_errors, mut receives, mut parameter_words) = (0u32, 0u32, 0u64);
    let mut coverage = [0u32; 40];
    while cursor < raw.len() {
        let destination = take(&raw, &mut cursor) as u8;
        let chip = take(&raw, &mut cursor);
        let local = take(&raw, &mut cursor);
        let busy = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let amount = take(&raw, &mut cursor) as i32;
        let linked = take(&raw, &mut cursor) as i32;
        let portamento_time = take(&raw, &mut cursor) as u8;
        let switch_required = take(&raw, &mut cursor) != 0;
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
        let count = take(&raw, &mut cursor);
        let callees: Vec<u32> = (0..count).map(|_| take(&raw, &mut cursor)).collect();
        let count = take(&raw, &mut cursor);
        let mut packets = Vec::new();
        let mut observed_packets = Vec::new();
        for _ in 0..count {
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let after_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            observed_packets.push((ack, payload.clone(), after_dsp));
            packets.push(payload);
        }
        let mut state = ActorControlState { bytes: before };
        let update = state
            .store_live_destination(destination, amount, linked)
            .unwrap();
        if update.compiler.map(compiler_entry) != callees.first().copied() {
            dispatch_errors += 1;
            first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"native":update.compiler.map(compiler_entry),"original":callees.first()}));
        }
        let mut coefficient = None;
        let mut amplifier_coefficient = None;
        let mut frequency_coefficient = None;
        let mut filter2_coefficients = None;
        let mut complete = update.compiler.is_none();
        if let Some(compiler) = update.compiler {
            use LiveModulationCompiler::*;
            match compiler {
                PrimaryMixer | SecondaryMixer | NoiseMixer => {
                    let mixer = destination - 3;
                    let value = state.compile_live_mixer(&body, mixer).unwrap();
                    coefficient = Some((0, 0x2f + 2 * u16::from(mixer), value as u16));
                    complete = true;
                }
                SecondaryPitch => {
                    let value = state.compile_live_secondary_pitch(&body, &fine);
                    coefficient = Some((17, 0x25, value as u16));
                    complete = true;
                }
                Pan => {
                    let value = state.compile_live_pan(&body, midi_pan, &pan);
                    coefficient = Some((0, 0x7f, value));
                    complete = true;
                }
                EnvelopeParameter {
                    envelope,
                    parameter,
                } => {
                    state
                        .compile_live_envelope_parameter(&body, envelope, parameter, &timing)
                        .unwrap();
                    complete = true;
                }
                Filter1Resonance => {
                    amplifier_coefficient =
                        Some(state.compile_live_filter1_resonance(&body, &resonance));
                    complete = true;
                }
                FilterMix => {
                    coefficient = Some((18, 0x35, state.compile_live_filter_mix(&body)));
                    complete = true;
                }
                Filter1KeyTracking => {
                    state.compile_live_filter1_key_tracking(&body, &filter1);
                    complete = true;
                }
                Filter1EnvelopeIntensity => {
                    frequency_coefficient = Some((
                        0x3c,
                        state.compile_live_filter1_frequency(&body, &filter1, &amplifier),
                    ));
                    complete = true;
                }
                Filter2EnvelopeIntensity | Filter2KeyTracking => {
                    if compiler == Filter2KeyTracking {
                        state.compile_live_filter2_key_tracking(&body, &filter1);
                    }
                    filter2_coefficients = Some((
                        true,
                        state.compile_live_filter2_frequency(
                            &body, &filter1, &amplifier, &resonance, &comb,
                        ),
                    ));
                    complete = true;
                }
                Filter2Resonance => {
                    let (value, norm) =
                        state.compile_live_filter2_resonance(&body, &resonance, &comb);
                    filter2_coefficients = Some((false, (value, norm.map(u32::from))));
                    complete = true;
                }
                Portamento => {
                    state.compile_live_portamento(
                        portamento_time,
                        switch_required,
                        portamento_switch,
                        &portamento,
                    );
                    complete = true;
                }
                ShaperDepth => {
                    if let Some(value) = state.compile_live_shaper_depth(&body).unwrap() {
                        coefficient = Some((0, 0x54, value as u16));
                    }
                    complete = true;
                }
            }
        }
        if complete {
            completed += 1;
            if state.bytes != after {
                complete_state_errors += 1;
                let offset = state
                    .bytes
                    .iter()
                    .zip(after)
                    .position(|(a, b)| *a != b)
                    .unwrap();
                first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"variant":variant,"offset":offset,"native":state.bytes[offset],"original":after[offset]}));
            }
            if let Some((opcode, offset, value)) = coefficient {
                let address = 0x2000 + 160 * local as u16 + offset;
                let expected = [6, opcode, 0, address, value];
                if packets.len() != 1 || packets[0].as_slice() != expected {
                    coefficient_errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"chip":chip,"busy":busy,"native":expected,"original":packets}));
                }
            } else if let Some((gain, normalization)) = amplifier_coefficient {
                let base = 0x2000 + 160 * local as u16;
                let expected = [
                    vec![
                        6,
                        20,
                        0,
                        base + 0x3e,
                        (gain as u32 >> 16) as u16,
                        gain as u16,
                    ],
                    vec![6, 0, 0, base + 0x36, normalization],
                ];
                if packets != expected {
                    coefficient_errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"native":expected,"original":packets}));
                }
            } else if let Some((offset, value)) = frequency_coefficient {
                let expected = vec![
                    6,
                    19,
                    0,
                    0x2000 + 160 * local as u16 + offset,
                    (value >> 16) as u16,
                    value as u16,
                ];
                if packets.len() != 1 || packets[0] != expected {
                    coefficient_errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"native":expected,"original":packets}));
                }
            } else if let Some((frequency, (value, additional))) = filter2_coefficients {
                let base = 0x2000 + 160 * local as u16;
                let is_comb = state.bytes[0x1e2] & 0x30 == 0x30;
                let offset = if frequency {
                    if is_comb { 0x68 } else { 0x64 }
                } else {
                    if is_comb { 0x60 } else { 0x66 }
                };
                let opcode = if is_comb {
                    1
                } else {
                    if frequency { 19 } else { 20 }
                };
                let mut expected = vec![vec![
                    6,
                    opcode,
                    0,
                    base + offset,
                    (value >> 16) as u16,
                    value as u16,
                ]];
                if let Some(extra) = additional {
                    expected.push(if frequency {
                        vec![6, 1, 0, base + 0x60, (extra >> 16) as u16, extra as u16]
                    } else {
                        vec![6, 0, 0, base + 0x5e, extra as u16]
                    });
                }
                if packets != expected {
                    coefficient_errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"native":expected,"original":packets}));
                }
            } else if !packets.is_empty() {
                coefficient_errors += 1;
            }
        }
        let request = LiveDestinationRequest {
            destination,
            amount,
            linked_pitch: linked,
            body: &body,
            ports: LiveCompilerPorts {
                portamento_time,
                portamento_switch_required: switch_required,
                portamento_switch,
                midi_pan,
            },
        };
        let slot = (local + 12 * chip) as usize;
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), pitch_dispatch);
            transport.restore_parameters(slot, before_dsp);
            let mut controller = ActorControlState { bytes: before };
            publish_live_destination(0, slot, &mut controller, request, &tables, &mut transport)
                .map_err(|e| format!("{e:?}"))?;
            let (mut clock, mut ready_at, mut seen_count, mut good) =
                (0, u64::from(busy), 0, controller.bytes == after);
            for (ack, payload, expected) in &observed_packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll| if poll<ready_at {1<<chip} else {0},|time,owner,event|{
                        seen_count+=1;
                        seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));
                    });
                }
                good &= seen == Some((*ack, slot, true))
                    && transport.parameter_state(slot) == *expected;
                if !good {
                    first_transport_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"variant":variant,"partition":partition,"ack":ack,"native_seen":seen,
                    "first_word":transport.parameter_state(slot).iter().zip(expected).position(|(a,b)|a!=b)}));
                }
                ready_at = *ack + u64::from(busy);
                parameter_words += 160;
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= seen_count == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                transport_errors += 1;
                first_transport_error.get_or_insert(serde_json::json!({"call":calls,"destination":destination,"variant":variant,"partition":partition,
                    "source_return":returned,"native_return":transport.caller_available_clock(),"pending":transport.pending()}));
            }
        }
        receives += count;
        coverage[destination as usize] += 1;
        calls += 1;
    }
    let report = serde_json::json!({"passed":dispatch_errors==0&&complete_state_errors==0&&coefficient_errors==0&&transport_errors==0,
        "whole_original_live_callbacks":calls,"procedure_coverage":coverage.to_vec(),"dispatch_errors":dispatch_errors,
        "complete_native_controller_cases":completed,"complete_state_errors":complete_state_errors,"coefficient_errors":coefficient_errors,
        "first_error":first_error,"all40_live_store_and_compiler_decisions_checked":true,
        "transport_errors":transport_errors,"first_transport_error":first_transport_error,"whole_original_E319_receives":receives,"parameter_words_compared":parameter_words,
        "all40_live_compiler_arithmetic_and_shared_FIFO_delivery_qualified":transport_errors==0,
        "whole_original_production_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("virtual-patch-live-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if dispatch_errors + complete_state_errors + coefficient_errors + transport_errors != 0 {
        return Err("Live controller transition differs".into());
    }
    Ok(())
}
