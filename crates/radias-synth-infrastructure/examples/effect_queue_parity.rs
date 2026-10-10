//! Original single active-queue producer/service, including ring and timer wrap.
use radias_synth_domain::{
    effect_queue::{EffectCommandQueue, EffectQueueError},
    effect_updates::CoefficientQueueWord,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-queue-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated queue corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x46515331 {
        return Err("Wrong queue corpus".into());
    }
    let (
        mut cursor,
        mut enqueues,
        mut producer_words,
        mut services,
        mut errors,
        mut host_words,
        mut packets,
        mut write_wraps,
        mut read_wraps,
        mut peak,
    ) = (
        1usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0u16,
    );
    let mut queue = EffectCommandQueue::default();
    let mut first = Value::Null;
    while cursor < words.len() {
        let tag = words[cursor];
        cursor += 1;
        let before = queue.state();
        if tag == 0x1000 {
            let n = words[cursor] as usize;
            cursor += 1;
            let input: Vec<_> = words[cursor..cursor + 2 * n]
                .chunks_exact(2)
                .map(|v| CoefficientQueueWord {
                    address: v[0] as u16,
                    tagged_value: v[1],
                })
                .collect();
            cursor += 2 * n;
            queue
                .enqueue_words(&input)
                .map_err(|_| "Original producer transaction rejected")?;
            let state = queue.state();
            if [
                u32::from(state.write_index),
                u32::from(state.read_index),
                u32::from(state.count),
            ] != words[cursor..cursor + 3]
            {
                errors += 1;
            }
            cursor += 3;
            enqueues += 1;
            producer_words += n;
            peak = peak.max(state.count);
            if state.write_index < before.write_index {
                write_wraps += 1;
            }
        } else if tag == 0x2000 {
            let tick = words[cursor] as u16;
            let status = words[cursor + 1];
            cursor += 2;
            let expected = &words[cursor..cursor + 5];
            cursor += 5;
            let n = words[cursor];
            cursor += 1;
            let mut original = Vec::new();
            for _ in 0..n {
                let address = words[cursor];
                let control = words[cursor + 1];
                let count = words[cursor + 2] as usize;
                cursor += 3;
                original.push((address, control, words[cursor..cursor + count].to_vec()));
                cursor += count;
            }
            let output = queue.service(tick, status & 3 != 0);
            let state = queue.state();
            let actual: Vec<_> = output.packets[..usize::from(output.count)]
                .iter()
                .map(|p| {
                    (
                        u32::from(p.address),
                        1,
                        p.values[..usize::from(p.count)].to_vec(),
                    )
                })
                .collect();
            if [
                u32::from(state.write_index),
                u32::from(state.read_index),
                u32::from(state.count),
                u32::from(state.wait_ticks),
                u32::from(state.wait_started),
            ] != expected
                || actual != original
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"service":services,"tick":tick,"status":status,"native_packets":actual,"original_packets":original,"native_state":format!("{state:?}"),"original_state":expected});
                }
            }
            services += 1;
            host_words += actual.iter().map(|p| p.2.len()).sum::<usize>();
            packets += actual.len();
            if state.read_index < before.read_index {
                read_wraps += 1;
            }
        } else {
            return Err("Unknown queue corpus record".into());
        }
    }
    let mut full = EffectCommandQueue::default();
    let input = vec![
        CoefficientQueueWord {
            address: 7,
            tagged_value: 0x7fffff
        };
        2048
    ];
    full.enqueue_words(&input)
        .map_err(|_| "Full valid queue rejected")?;
    let before = full.state();
    let full_rejected =
        full.enqueue_words(&input[..1]) == Err(EffectQueueError::Full) && full.state() == before;
    let mut invalid = EffectCommandQueue::default();
    let before = invalid.state();
    let malformed_rejected = invalid.enqueue_words(&[CoefficientQueueWord {
        address: 0,
        tagged_value: 0x84000000,
    }]) == Err(EffectQueueError::IncompletePacket)
        && invalid.state() == before;
    let unsupported_rejected = invalid.enqueue_words(&[CoefficientQueueWord {
        address: 0,
        tagged_value: 0x02000000,
    }]) == Err(EffectQueueError::UnsupportedCommand)
        && invalid.state() == before;
    let passed = errors == 0
        && enqueues == 640
        && producer_words == 20480
        && services == 14404
        && write_wraps == 10
        && read_wraps == 10
        && queue.state().count == 0
        && queue.state().wait_ticks == 0
        && full_rejected
        && malformed_rejected
        && unsupported_rejected;
    let report = json!({"passed":passed,"whole_original_producer_batches":enqueues,"original_words_enqueued":producer_words,"whole_original_queue_service_calls":services,"errors":errors,"first_difference":first,
        "host_words_compared":host_words,"host_packets_compared":packets,"write_ring_wraps":write_wraps,"read_ring_wraps":read_wraps,"peak_queue_depth":peak,
        "full_malformed_and_unsupported_transactions_rejected_atomically":[full_rejected,malformed_rejected,unsupported_rejected],"external_tick_and_host_status_are_declared_inputs":true,
        "native_previous_queue_outputs_replayed_as_inputs":false,"original_functions_and_all_callees_execute_without_stubs":true,
        "double_buffer_switches_program_upload_commands_FXD03_audio_or_physical_timer_frequency_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-queue-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native active FX queue: {services} original services, {errors} differences, {write_wraps}/{read_wraps} ring wraps"
    );
    if !passed {
        return Err("Native active effects queue differs".into());
    }
    Ok(())
}
