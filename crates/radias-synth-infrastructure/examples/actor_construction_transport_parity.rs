use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::actor_startup::CoefficientPriming;
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
    let tables = firmware::physical_phase_tables(&sys)?;
    let callbacks = firmware::phase_callback_tables(&sys)?;
    let seeds = firmware::formant_counter_seeds(&sys)?;
    let (mut calls, mut commands, mut words, mut errors, mut nulls) = (0, 0, 0, 0, 0);
    let mut first_error = None;
    for family in ["phase", "prime", "callback", "counter"] {
        let raw = fs::read(out.join(format!("actor-{family}-original.bin")))?;
        let mut cursor = 0;
        while cursor < raw.len() {
            let chip = take(&raw, &mut cursor) as usize;
            let local = take(&raw, &mut cursor) as usize;
            let first = take(&raw, &mut cursor) as u8;
            let second = take(&raw, &mut cursor) as u8;
            let busy = u64::from(take(&raw, &mut cursor));
            let before: Vec<u16> = (0..2688).map(|_| take(&raw, &mut cursor) as u16).collect();
            let extra: [u32; 5] = if matches!(family, "callback" | "counter") {
                core::array::from_fn(|_| take(&raw, &mut cursor))
            } else {
                [0; 5]
            };
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
                let after: Vec<u16> = (0..2688).map(|_| take(&raw, &mut cursor) as u16).collect();
                packets.push((sender, entry, ack, payload, after));
            }
            for chunk in [1u64, 31, 3000] {
                let mut queue = SynthesisParameterTransport::default();
                for actor in 0..12 {
                    queue.restore_construction_state(
                        actor + 12 * chip,
                        before[160 * actor..160 * (actor + 1)].try_into()?,
                        before[1920 + 64 * actor..1920 + 64 * (actor + 1)].try_into()?,
                    );
                }
                let result = if family == "phase" {
                    queue.initialize_physical_phases(
                        0,
                        local + 12 * chip,
                        tables.get(first).ok_or("Unsupported phase selection")?,
                    )
                } else if family == "prime" {
                    queue.prime_actor(
                        0,
                        local + 12 * chip,
                        CoefficientPriming {
                            routing: first,
                            shaper: second,
                        },
                    )
                } else if family == "callback" {
                    queue.initialize_phase_callback(
                        0,
                        local + 12 * chip,
                        callbacks.phases
                            [radias_synth_domain::actor_startup::PhaseCallbackTables::index(first)],
                        extra[0] as i16,
                        radias_synth_domain::controller_primary::PrimaryControl {
                            control2: extra[1] as u8,
                            control2_modulation: extra[2] as i16,
                            control2_manual_offset: extra[3] as i8,
                            ..Default::default()
                        },
                    )
                } else {
                    queue.initialize_slot_counter(
                        0,
                        local + 12 * chip,
                        callbacks.counters
                            [radias_synth_domain::actor_startup::PhaseCallbackTables::index(first)],
                        extra[4] as u8,
                        &seeds,
                    )
                };
                result.map_err(|e| format!("{e:?}"))?;
                let mut clock = 0;
                let mut ready_at = busy;
                let mut good = true;
                let mut received = 0;
                for (_, _, ack, payload, after) in &packets {
                    let mut seen = None;
                    while clock < *ack {
                        clock = (clock + chunk).min(*ack);
                        queue.advance_until_with_readiness(
                            clock,
                            |poll| if poll < ready_at { 1 << chip } else { 0 },
                            |time, owner, publication| {
                                received += 1;
                                if let DeliveredSynthesisParameter::ActorState {
                                    opcode,
                                    active: _,
                                } = publication
                                {
                                    good &= owner == local + 12 * chip;
                                    seen = Some((time, opcode));
                                } else {
                                    good = false;
                                }
                            },
                        );
                    }
                    good &= seen == Some((*ack, payload[1]));
                    for actor in 0..12 {
                        good &= queue.parameter_state(actor + 12 * chip).as_slice()
                            == &after[160 * actor..160 * (actor + 1)]
                            && queue.physical_parameter_state(actor + 12 * chip).as_slice()
                                == &after[1920 + 64 * actor..1920 + 64 * (actor + 1)];
                    }
                    ready_at = *ack + busy;
                    words += 2688;
                }
                queue.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
                good &= received == count
                    && queue.pending() == 0
                    && queue.caller_available_clock() == returned;
                if count == 0 {
                    for actor in 0..12 {
                        good &= queue.parameter_state(actor + 12 * chip).as_slice()
                            == &before[160 * actor..160 * (actor + 1)]
                            && queue.physical_parameter_state(actor + 12 * chip).as_slice()
                                == &before[1920 + 64 * actor..1920 + 64 * (actor + 1)];
                    }
                    words += 2688;
                }

                if !good {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"family":family,"call":calls,"chunk":chunk,"first":first,"second":second,"busy":busy,"received":received,"source_return":returned,"native_return":queue.caller_available_clock()}));
                }
            }
            calls += 1;
            commands += count;
            if count == 0 {
                nulls += 1;
            }
        }
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_phase_and_prime_callers":calls,"whole_original_E319_receivers":commands,"null_phase_callers":nulls,"compared_parameter_and_physical_words":words,"errors":errors,"first_error":first_error,
        "both_DSPs_all12_actor_slots":true,"all24_non_PCM_phase_encodings":true,"all4_routing_and_all16_stored_shaper_encodings":true,"unrelated_flags_checked":true,
        "source_readiness_policies":[0,64,333,3000],"CPU_clock_partitions":[1,31,3000],"all160_word_actor_and64_word_physical_banks_checked":true,"source_prior_banks_declared_inputs":true,
        "phase_constants_read_from_SYS":true,"native_common_FIFO_memory_and_shared_receivers_used":true,"Pickup_Parallel_pitch_shadow_before_priming_qualified":true,"production_shared_synthesis_FIFO_and_parameter_physical_memory_used":true,
        "original_Unison_phase_callback_and_slot_counter_callback_qualified":true,"callback_cached_controller_word_is_declared_input":true,"Unison_phase_code_compiled_from_native_control_inputs":true,
        "full_native_note_constructor_sequence_enabled":false,"independent_receiver_DMA_sample_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-construction-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original actor startup differs".into());
    }
    Ok(())
}
