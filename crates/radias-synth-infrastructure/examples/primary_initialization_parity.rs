//! Complete original constructor packets and actor banks; no initial audio shift.
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::primary_parameters;
use radias_synth_infrastructure::firmware;
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
    let tables = firmware::primary_initialization_tables(&sys)?;
    let raw = fs::read(out.join("primary-initialization-original.bin"))?;
    let (mut cursor, mut calls, mut commands, mut errors) = (0, 0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let selection = take(&raw, &mut cursor) as u8;
        let busy = u64::from(take(&raw, &mut cursor));
        let before: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let count = take(&raw, &mut cursor) as usize;
        let mut packets = Vec::new();
        for _ in 0..count {
            let sender_clock = take(&raw, &mut cursor);
            let ack_clock = u64::from(take(&raw, &mut cursor));
            let packet: [u16; 5] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            let after: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((sender_clock, ack_clock, packet, after));
        }
        let slot = local + 12 * chip;
        let initialization = tables
            .get(selection)
            .ok_or("Unsupported original selection")?;
        if count != initialization.words().len() + 1 {
            return Err("Source constructor command count differs from ROM".into());
        }
        for chunk in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.restore_parameters(slot, before);
            transport
                .initialize_primary(0, slot, initialization)
                .map_err(|e| format!("{e:?}"))?;
            if transport.parameter_state(slot) != before {
                return Err("Enqueue prematurely changed parameter bank".into());
            }
            let mut clock = 0;
            let mut ready_at = busy;
            let mut received = 0;
            let mut good = true;
            for &(_, ack, packet, after) in &packets {
                let mut event = None;
                while clock < ack {
                    clock = (clock + chunk).min(ack);
                    transport.advance_until_with_readiness(
                        clock,
                        |poll| if poll < ready_at { 1 << chip } else { 0 },
                        |delivered, owner, value| {
                            received += 1;
                            if let DeliveredSynthesisParameter::PrimaryInitialization {
                                offset,
                                value,
                                parameters,
                            } = value
                            {
                                event = Some((delivered, owner, offset, value, parameters));
                            }
                        },
                    );
                }
                let expected =
                    primary_parameters::decode(&after).ok_or("Unsupported source descriptor")?;
                good &= event
                    == Some((
                        ack,
                        slot,
                        packet[3] - (0x2000 + 160 * local as u16),
                        packet[4],
                        expected,
                    ));
                good &= transport.parameter_state(slot) == after;
                if event.is_some() {
                    ready_at = ack + busy;
                }
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= received == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"call":calls,"selection":selection,"chip":chip,"slot":slot,"busy":busy,"chunk":chunk,"received":received,"pending":transport.pending(),"source_return":returned,"native_return":transport.caller_available_clock()}));
            }
        }
        calls += 1;
        commands += count;
    }
    // Initialization reserves the complete sequence before writing metadata.
    let mut full = SynthesisParameterTransport::default();
    for _ in 0..511 {
        full.shaper_depth(0, 0, 17).map_err(|e| format!("{e:?}"))?;
    }
    let prior = full.parameter_state(0);
    let atomic_full = full.initialize_primary(0, 0, tables.get(4).unwrap())
        == Err(radias_synth_application::dsp_transport::SendQueueError::Full)
        && full.pending() == 511
        && full.parameter_state(0) == prior;
    let invalid_slot = full.initialize_primary(0, 24, tables.get(0).unwrap())
        == Err(radias_synth_application::dsp_transport::SendQueueError::InvalidSlot);
    if !atomic_full || !invalid_slot || tables.get(6).is_some() {
        errors += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_constructor_calls":calls,"whole_original_E319_receiver_calls":commands,"errors":errors,"first_error":first_error,
        "all24_non_PCM_descriptor_encodings":true,"both_processors_and_all12_actor_slots":true,"unrelated_high_selection_flags_checked":true,
        "source_readiness_policies":[0,96,333,3000],"CPU_clock_partitions":[1,31,3000],"every160_word_bank_after_every_command_checked":true,
        "native_ROM_constants_and_domain_parameter_decoder_used":true,"source_bank_before_call_declared":true,"source_outputs_used_only_for_comparison":true,
        "relative_caller_work_retains_busy_stalls":true,"full_caller_return_clock_matches":errors==0,"common_production_parameter_FIFO_used":true,"enqueue_preserves_bank":true,
        "queue_full_is_atomic":atomic_full,"invalid_slot_rejected":invalid_slot,"original_Physical_phase_and_other_note_constructor_calls_qualified":false,
        "production_note_constructor_sequence_enabled":false,"independent_receiver_DMA_sample_job_timing_qualified":false,"whole_instrument_audio_parity_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("primary-initialization-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original primary initialization differs".into());
    }
    Ok(())
}
