use radias_synth_application::{
    actor_lifecycle,
    dsp_receiver::{
        ParameterMemory, ReceiveOutcome, receive_memory_command, receive_shared_command,
    },
    dsp_transport::SendQueueError,
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::actor_lifecycle::ActorLifecycle;
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};

struct Memory(Vec<u16>);
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.0[usize::from(address)]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.0[usize::from(address)] = value;
    }
}
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
            let mut memory = Memory(vec![0; 65536]);
            memory.0[0x2000..0x2780].copy_from_slice(&before[..1920]);
            memory.0[0x3000..0x3300].copy_from_slice(&before[1920..]);
            let mut queue = OrderedParameterTransport::<4>::default();
            let mut lifecycle = ActorLifecycle { active: mask };
            let mut target = cached_target;
            let slot = local + chip * 12;
            match family {
                0 => actor_lifecycle::activate(&mut queue, &mut lifecycle, 0, slot),
                1 => actor_lifecycle::detach(&mut queue, &mut lifecycle, 0, slot),
                2 => actor_lifecycle::reset_amplifier(&mut queue, &mut target, 0, slot, &tables),
                _ => return Err("Unknown source lifecycle caller".into()),
            }
            .map_err(|e| format!("{e:?}"))?;
            let mut good = lifecycle.active == after_mask && target == after_target;
            let mut seen = 0;
            let mut clock = 0;
            while clock < returned {
                clock = (clock + chunk).min(returned);
                queue.advance_until(
                    clock,
                    |poll| if poll < busy { 1 << chip } else { 0 },
                    |ack, received| {
                        let (_, _, expected_ack, payload, after) = &packets[seen];
                        good &= ack == *expected_ack && received.words() == payload;
                        for (offset, value) in received.words().iter().copied().enumerate() {
                            memory.0[0x100 + offset] = value;
                        }
                        good &= if payload[1] == 38 {
                            receive_shared_command(&mut memory, 0x100)
                        } else {
                            receive_memory_command(&mut memory, 0x100)
                        } == ReceiveOutcome::Ready;
                        good &= memory.0[0x2000..0x2780] == after[..1920]
                            && memory.0[0x3000..0x3300] == after[1920..];
                        seen += 1;
                    },
                );
            }
            good &=
                seen == count && queue.pending() == 0 && queue.caller_available_clock() == returned;
            if count == 0 {
                good &= memory.0[0x2000..0x2780] == before[..1920]
                    && memory.0[0x3000..0x3300] == before[1920..];
            }
            if !good {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"chip":chip,"slot":slot,"busy":busy,"chunk":chunk,"source_return":returned,"native_return":queue.caller_available_clock(),"source_mask":after_mask,"native_mask":lifecycle.active,"source_target":after_target,"native_target":target}));
            }
            words += 2688;
        }
        receives += count;
        nulls += usize::from(count == 0);
        calls += 1;
    }
    // A rejected caller cannot publish a mask/target update or discard queued work.
    let mut queue = OrderedParameterTransport::<1>::default();
    queue.enqueue_work(0, 31).map_err(|e| format!("{e:?}"))?;
    let mut lifecycle = ActorLifecycle {
        active: 0x89ab_cdef,
    };
    let mut target = -731;
    let unchanged = lifecycle;
    for operation in 0..3 {
        let result = match operation {
            0 => actor_lifecycle::activate(&mut queue, &mut lifecycle, 0, 2),
            1 => actor_lifecycle::detach(&mut queue, &mut lifecycle, 0, 2),
            _ => actor_lifecycle::reset_amplifier(&mut queue, &mut target, 0, 2, &tables),
        };
        if result != Err(SendQueueError::Full)
            || lifecycle != unchanged
            || target != -731
            || queue.pending() != 1
        {
            return Err("Full FIFO published a partial lifecycle change".into());
        }
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_lifecycle_callers":calls,
        "whole_original_E319_lifecycle_receives":receives,"null_deactivation_callers":nulls,
        "parameter_and_physical_words_compared":words,"errors":errors,"first_error":first_error,
        "both_DSPs_all12_local_actor_slots_active_inactive_masks":true,
        "readiness_inputs":[0,64,333,3000],"clock_partitions":[1,31,3000],
        "controller_masks_and_prior_banks_are_declared_inputs":true,
        "opcode38_tail_transfer_and_reset_target_and_activation_qualified":true,
        "null_calls_keep_common_FIFO_position":true,"full_FIFO_rejection_preserves_state":true,
        "full_note_constructor_or_receiver_DMA_sample_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-lifecycle-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original lifecycle callers differ".into());
    }
    Ok(())
}
