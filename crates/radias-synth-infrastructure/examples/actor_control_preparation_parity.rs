//! Original controller preparation followed by original complete descriptors.
use radias_synth_application::actor_preparation::{
    ActorPreparationError, prepare_actor_descriptors,
};
use radias_synth_application::dsp_transport::SendQueueError;
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    parameter_template::{ParameterTemplateTables, TemplateCompilationError},
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn verify_rejections(
    body: &[u8; 104],
    owner: u8,
    prior: ActorControlState,
    tables: &ParameterTemplateTables,
    configured: &mut SynthesisParameterTransport,
) -> Result<u32, Box<dyn std::error::Error>> {
    let mut invalid_body = *body;
    invalid_body[51] = 128;
    for (slot, input, expected) in [
        (
            24,
            body,
            ActorPreparationError::Publish(SendQueueError::InvalidSlot),
        ),
        (
            0,
            body,
            ActorPreparationError::Publish(SendQueueError::MissingFilterContext),
        ),
        (
            0,
            &invalid_body,
            ActorPreparationError::Compile(TemplateCompilationError::InvalidOutputGain),
        ),
    ] {
        let mut controller = prior;
        let mut transport = SynthesisParameterTransport::default();
        let result = prepare_actor_descriptors(
            0,
            slot,
            input,
            owner,
            &mut controller,
            tables,
            &mut transport,
        );
        if result != Err(expected) || controller != prior || transport.pending() != 0 {
            return Err("Rejected preparation changed prior state or FIFO".into());
        }
    }
    let mut controller = prior;
    loop {
        let before = controller;
        let pending = configured.pending();
        match prepare_actor_descriptors(0, 0, body, owner, &mut controller, tables, configured) {
            Ok(()) => {}
            Err(ActorPreparationError::Publish(SendQueueError::Full)) => {
                if controller != before || configured.pending() != pending {
                    return Err("Full FIFO partially published preparation".into());
                }
                break;
            }
            Err(error) => return Err(format!("Unexpected rejection: {error:?}").into()),
        }
    }
    Ok(4)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master)?;
    let slave = MasterTables::from_host_stream(&slave)?;
    let tables = firmware::parameter_template_tables(&sys, master.filter_mix()?)?;
    let rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let raw = fs::read(out.join("actor-control-preparation-original.bin"))?;
    let (
        mut cursor,
        mut calls,
        mut errors,
        mut byte_errors,
        mut clock_errors,
        mut receives,
        mut words,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let mut first_error = None;
    let mut coverage = [0u32; 3];
    let mut rejection_checks = 0;
    while cursor < raw.len() {
        let family = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let busy = u64::from(take(&raw, &mut cursor));
        let owner = take(&raw, &mut cursor) as u8;
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let before_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let preparation_clocks = take(&raw, &mut cursor);
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let returned = u64::from(take(&raw, &mut cursor));
        let count = take(&raw, &mut cursor) as usize;
        let mut packets = Vec::new();
        for _ in 0..count {
            let sender = take(&raw, &mut cursor);
            let entry = take(&raw, &mut cursor);
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let after_dsp: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((sender, entry, ack, payload, after_dsp));
        }
        let mut state = ActorControlState { bytes: before };
        if family == 0 {
            state.refresh_primary();
        } else {
            state.prepare_from_body(&body, owner);
        }
        let native_clocks =
            u32::from(state.primary_preparation_clocks()) + if family == 0 { 0 } else { 45 };
        if state.bytes != after {
            byte_errors += 1;
            let offset = state
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b)
                .unwrap();
            first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"offset":offset,"source":after[offset],"native":state.bytes[offset],"body":body.to_vec(),"control":format!("{:?}",state.primary_inputs())}));
        }
        if native_clocks != preparation_clocks {
            clock_errors += 1;
            first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"source_clocks":preparation_clocks,"native_clocks":native_clocks}));
        }
        if family == 2 {
            if rejection_checks == 0 {
                let mut configured = SynthesisParameterTransport::default();
                configured.configure_constructor_filter_mix(master.filter_mix()?);
                configured.configure_pitch_receivers(rom.clone(), dispatch);
                rejection_checks = verify_rejections(
                    &body,
                    owner,
                    ActorControlState { bytes: before },
                    &tables,
                    &mut configured,
                )?;
            }
            for chunk in [1u64, 31, 3000] {
                let slot = local + 12 * chip;
                let mut transport = SynthesisParameterTransport::default();
                transport.configure_constructor_filter_mix(master.filter_mix()?);
                transport.configure_pitch_receivers(rom.clone(), dispatch);
                transport.restore_parameters(slot, before_dsp);
                let mut application_state = ActorControlState { bytes: before };
                prepare_actor_descriptors(
                    0,
                    slot,
                    &body,
                    owner,
                    &mut application_state,
                    &tables,
                    &mut transport,
                )
                .map_err(|e| format!("{e:?}"))?;
                let (mut clock, mut ready_at, mut seen_count, mut good) =
                    (0, busy, 0, application_state == state);
                for (_, _, ack, payload, after_dsp) in &packets {
                    let mut seen = None;
                    while clock < *ack {
                        clock = (clock + chunk).min(*ack);
                        transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{
                            seen_count+=1;seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));
                        });
                    }
                    good &= seen == Some((*ack, slot, true))
                        && transport.parameter_state(slot) == *after_dsp;
                    ready_at = *ack + busy;
                    words += 160;
                }
                transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
                good &= seen_count == count
                    && transport.pending() == 0
                    && transport.caller_available_clock() == returned;
                if !good {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"chunk":chunk,"received":seen_count,"expected":count,"source_return":returned,"native_return":transport.caller_available_clock()}));
                }
            }
        }
        calls += 1;
        coverage[family as usize] += 1;
        receives += count;
    }
    let report = serde_json::json!({"passed":errors==0&&byte_errors==0&&clock_errors==0,"whole_original_preparation_calls":calls,
        "whole_original_prepare_then_descriptor_chains":coverage[2],"whole_original_E319_chain_receives":receives,
        "controller_bytes_compared":calls*496,"parameter_words_compared":words,"errors":errors,"controller_errors":byte_errors,
        "preparation_clock_errors":clock_errors,"first_error":first_error,"family_coverage":coverage,
        "all24_non_PCM_encodings_both_DSPs_all12_actor_bands":true,"cached_primary_targets_computed_from_native_raw_inputs":true,
        "previous_inactive_shadows_retained":true,"native_shadow_outputs_feed_common_FIFO_descriptor_chain":true,
        "application_preparation_and_publication_used":true,
        "atomic_rejection_cases":rejection_checks,
        "prior_controller_and_DSP_state_readiness_other_modulations_declared_inputs":true,
        "whole_note_constructor_all_virtual_patch_and_sample_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-control-preparation-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if errors + byte_errors + clock_errors != 0 {
        return Err("Native control preparation differs".into());
    }
    Ok(())
}
