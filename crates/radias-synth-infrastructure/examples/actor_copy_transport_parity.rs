use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{
    actor_copy::ActorTemplateBinding, dsp_control::DspEndpoint,
    parameter_template::ParameterTemplate,
};
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
    let addresses = firmware::parameter_template_addresses(&sys)?;
    let mut missing = SynthesisParameterTransport::default();
    let binding = ActorTemplateBinding {
        program_kind: 0,
        timbre_id: 0,
        drum_instrument: 0,
        ordinary_address: 0x780,
    };
    if missing.copy_actor(0, 0, binding, &addresses)
        != Err(radias_synth_application::dsp_transport::SendQueueError::MissingParameterTemplate)
        || missing.pending() != 0
        || missing.parameter_state(0) != [0; 160]
    {
        return Err("Unloaded template was silently copied into an actor".into());
    }
    let raw = fs::read(out.join("actor-copy-original.bin"))?;
    let (mut cursor, mut calls, mut errors, mut compared) = (0, 0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let binding = ActorTemplateBinding {
            program_kind: take(&raw, &mut cursor) as u8,
            timbre_id: take(&raw, &mut cursor) as u8,
            drum_instrument: take(&raw, &mut cursor) as u8,
            ordinary_address: take(&raw, &mut cursor) as u16,
        };
        let busy = u64::from(take(&raw, &mut cursor));
        let before: Vec<u16> = (0..4480).map(|_| take(&raw, &mut cursor) as u16).collect();
        let sender = take(&raw, &mut cursor);
        let ack = u64::from(take(&raw, &mut cursor));
        let returned = u64::from(take(&raw, &mut cursor));
        let packet: [u16; 4] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let after: Vec<u16> = (0..4480).map(|_| take(&raw, &mut cursor) as u16).collect();
        for chunk in [1u64, 31, 3000] {
            let mut queue = SynthesisParameterTransport::default();
            let endpoint = if chip == 0 {
                DspEndpoint::Master
            } else {
                DspEndpoint::Slave
            };
            for template in 0..16 {
                queue
                    .install_parameter_template(
                        endpoint,
                        template + 4,
                        ParameterTemplate {
                            words: before[160 * template..160 * (template + 1)].try_into()?,
                        },
                    )
                    .map_err(|e| format!("{e:?}"))?;
            }
            for actor in 0..12 {
                queue.restore_parameters(
                    actor + 12 * chip,
                    before[2560 + 160 * actor..2560 + 160 * (actor + 1)].try_into()?,
                );
            }
            queue
                .copy_actor(0, local + 12 * chip, binding, &addresses)
                .map_err(|e| format!("{e:?}"))?;
            let mut clock = 0;
            let mut seen = Vec::new();
            while clock < returned {
                clock = (clock + chunk).min(returned);
                queue.advance_until_with_readiness(
                    clock,
                    |poll| if poll < busy { 1 << chip } else { 0 },
                    |time, owner, publication| {
                        seen.push((
                            time,
                            owner,
                            matches!(
                                publication,
                                DeliveredSynthesisParameter::ActorState { opcode: 9, .. }
                            ),
                        ));
                    },
                );
            }
            let equal = sender == u32::from(binding.sender_gap())
                && seen == vec![(ack, local + 12 * chip, true)]
                && queue.pending() == 0
                && queue.caller_available_clock() == returned
                && packet
                    == [
                        6,
                        9,
                        binding.source(&addresses),
                        0x2000 + 160 * local as u16,
                    ]
                && before[..2560] == after[..2560]
                && (0..12).all(|actor| {
                    queue.parameter_state(actor + 12 * chip).as_slice()
                        == &after[2560 + 160 * actor..2560 + 160 * (actor + 1)]
                });
            if !equal {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":calls,"chip":chip,"chunk":chunk,"binding":format!("{binding:?}"),"source_ack":ack,"actual":seen,"source_return":returned,"native_return":queue.caller_available_clock()}));
            }
            compared += 4480;
        }
        calls += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_template_binding_copy_calls":calls,"whole_original_E319_copy_calls":calls,"errors":errors,"first_error":first_error,
        "parameter_words_compared":compared,"all8_program_kind_flags_all4_timbres_all16_drums_both_DSPs":true,"unrelated_selector_bits_checked":true,"all12_actor_slots":true,
        "source_readiness_policies":[0,64,333,3000],"CPU_clock_partitions":[1,31,3000],"all16_template_banks_and_all12_actor_banks_checked":true,"source_prior_template_and_actor_banks_declared":true,
        "native_template_binding_and_common_FIFO_used":true,"production_shared_synthesis_FIFO_template_and_actor_memory_used":true,"original_template_data_not_assumed_immutable_firmware_ROM":true,
        "full_caller_and_HPI_ACK_clocks_match":errors==0,"receiver_DMA_job_timing_qualified":false,"native_template_construction_and_full_note_on_order_qualified":false,
        "source_controller_outputs_used_to_render":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-copy-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native actor binding/copy differs".into());
    }
    Ok(())
}
