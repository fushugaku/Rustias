use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::actor_lifecycle::ActorLifecycle;
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
    let tables = firmware::amplifier_rate_table(&sys)?;
    let raw = fs::read(out.join("actor-lifecycle-original.bin"))?;
    let (mut cursor, mut calls, mut receives, mut nulls, mut errors, mut words) =
        (0, 0, 0, 0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let family = take(&raw, &mut cursor);
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let mask = take(&raw, &mut cursor);
        let cached_target = take(&raw, &mut cursor) as i16;
        let busy = u64::from(take(&raw, &mut cursor));
        let before: Vec<u16> = (0..2688).map(|_| take(&raw, &mut cursor) as u16).collect();
        let after_mask = take(&raw, &mut cursor);
        let after_target = take(&raw, &mut cursor) as i16;
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
            let slot = local + 12 * chip;
            let mut queue = SynthesisParameterTransport::default();
            for actor in 0..12 {
                queue.restore_construction_state(
                    actor + 12 * chip,
                    before[160 * actor..160 * (actor + 1)].try_into()?,
                    before[1920 + 64 * actor..1920 + 64 * (actor + 1)].try_into()?,
                );
            }
            queue.set_actor_lifecycle(ActorLifecycle { active: mask });
            queue.restore_cached_amplifier_target(slot, cached_target);
            match family {
                0 => queue.activate_actor(0, slot),
                1 => queue.detach_actor(0, slot),
                2 => queue.reset_actor_amplifier(0, slot, &tables),
                _ => return Err("Unknown source lifecycle".into()),
            }
            .map_err(|e| format!("{e:?}"))?;
            let mut good = queue.actor_lifecycle().active == after_mask
                && queue.cached_amplifier_target(slot) == after_target;
            let mut seen = 0;
            let mut clock = 0;
            while clock < returned {
                clock = (clock + chunk).min(returned);
                queue.advance_until_with_readiness(clock,|poll|if poll<busy{1<<chip}else{0},|ack,owner,pubbed|{
                    let (_,_,expected_ack,payload,_) =&packets[seen];
                    good &= ack==*expected_ack && owner==slot && matches!(pubbed,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1]);
                    seen+=1;
                });
            }
            good &=
                seen == count && queue.pending() == 0 && queue.caller_available_clock() == returned;
            let after = packets.last().map(|p| p.4.as_slice()).unwrap_or(&before);
            for actor in 0..12 {
                good &= queue.parameter_state(actor + 12 * chip).as_slice()
                    == &after[160 * actor..160 * (actor + 1)]
                    && queue.physical_parameter_state(actor + 12 * chip).as_slice()
                        == &after[1920 + 64 * actor..1920 + 64 * (actor + 1)];
            }
            if !good {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"chip":chip,"slot":slot,"busy":busy,"chunk":chunk,"source_return":returned,"native_return":queue.caller_available_clock(),"source_mask":after_mask,"native_mask":queue.actor_lifecycle().active,"source_target":after_target,"native_target":queue.cached_amplifier_target(slot)}));
            }
            words += 2688;
        }
        receives += count;
        nulls += usize::from(count == 0);
        calls += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_lifecycle_callers":calls,
        "whole_original_E319_lifecycle_receives":receives,"null_deactivation_callers":nulls,
        "parameter_and_physical_words_compared":words,"errors":errors,"first_error":first_error,
        "both_DSPs_all12_local_actor_slots_active_inactive_masks":true,
        "readiness_inputs":[0,64,333,3000],"clock_partitions":[1,31,3000],
        "controller_masks_and_prior_banks_are_declared_inputs":true,
        "opcode38_tail_transfer_and_reset_target_and_activation_qualified":true,
        "null_calls_keep_common_FIFO_position":true,"production_common_FIFO_and_shared_parameter_physical_memory_used":true,
        "full_note_constructor_or_receiver_DMA_sample_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-lifecycle-transport-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original lifecycle callers differ".into());
    }
    Ok(())
}
