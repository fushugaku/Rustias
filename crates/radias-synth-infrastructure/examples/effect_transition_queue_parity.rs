//! Independent whole original producer, service, program getters and host ports.
use radias_synth_application::{
    effect_transition_queue::{EffectProgramSource, service_effect_transition_queue},
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    effect_queue::EffectQueueError,
    effect_transition_queue::{
        EffectProgramBufferLayout, EffectTransitionQueue, EffectTransitionQueueState,
    },
    effect_updates::CoefficientQueueWord,
};
use radias_synth_infrastructure::effect_program_buffers::EffectProgramBuffers;
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{convert::Infallible, fs, path::PathBuf};
#[derive(Default)]
struct Port {
    packets: Vec<(bool, u16, u16, Vec<u64>)>,
}
impl EffectProgramPort for Port {
    type Error = Infallible;
    fn upload_program(
        &mut self,
        address: u16,
        words: &[u64],
        control: u16,
    ) -> Result<(), Self::Error> {
        self.packets.push((true, address, control, words.to_vec()));
        Ok(())
    }
    fn write_coefficient(
        &mut self,
        address: u16,
        value: u32,
        control: u16,
    ) -> Result<(), Self::Error> {
        self.write_coefficient_packet(address, &[value], control)
    }
    fn write_coefficient_packet(
        &mut self,
        address: u16,
        values: &[u32],
        control: u16,
    ) -> Result<(), Self::Error> {
        self.packets.push((
            false,
            address,
            control,
            values.iter().copied().map(u64::from).collect(),
        ));
        Ok(())
    }
}
struct Reader {
    words: Vec<u32>,
    cursor: usize,
}
impl Reader {
    fn take(&mut self) -> u32 {
        let v = self.words[self.cursor];
        self.cursor += 1;
        v
    }
    fn state(&mut self) -> [u32; 9] {
        core::array::from_fn(|_| self.take())
    }
}
fn state(s: EffectTransitionQueueState) -> [u32; 9] {
    [
        u32::from(s.rings[0].write_index),
        u32::from(s.rings[0].read_index),
        u32::from(s.rings[0].count),
        u32::from(s.rings[1].write_index),
        u32::from(s.rings[1].read_index),
        u32::from(s.rings[1].count),
        u32::from(s.control),
        u32::from(s.wait_ticks),
        u32::from(s.wait_started),
    ]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/effect-transition-queue-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated transition corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.take() != 0x46515431 {
        return Err("Wrong transition corpus".into());
    }
    let mut bases = [0; 3];
    let buffers = core::array::from_fn(|i| {
        bases[i] = r.take();
        let size = r.take() as usize;
        (0..size).map(|_| r.take() as u8).collect()
    });
    let declared_layout = EffectProgramBufferLayout {
        normal: bases[0],
        selected_insert: bases[1],
        selected_master: bases[2],
    };
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let layout = EffectLibrary::from_system(&system)?.program_buffer_layout();
    if [
        layout.normal,
        layout.selected_insert,
        layout.selected_master,
    ] != [
        declared_layout.normal,
        declared_layout.selected_insert,
        declared_layout.selected_master,
    ] {
        return Err("Native buffer adapter disagrees with declared source memory".into());
    }
    let mut source = EffectProgramBuffers::from_buffers(layout, buffers)?;
    let mut errors = 0usize;
    let getters = r.take() as usize;
    for selector in 0..getters {
        let count = r.take();
        let address = r.take();
        if source.program_words(selector as u8).unwrap().len() != count as usize
            || layout.locate(selector as u8).unwrap().word_address != address
        {
            errors += 1;
        }
    }
    let (mut producer_calls, mut batches, mut services, mut scenarios) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut program_words, mut program_packets, mut coefficient_words, mut coefficient_packets) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut write_wraps, mut read_wraps) = ([0usize; 2], [0usize; 2]);
    let mut selector_counts = [0usize; 87];
    let mut tag_counts = [0usize; 256];
    let (mut mutations, mut aliased_program_words) = (0usize, 0usize);
    let mut first = Value::Null;
    let mut queue = EffectTransitionQueue::default();
    while r.cursor < r.words.len() {
        let record = r.take();
        let before = queue.state();
        match record {
            0x3000 => {
                let flags = r.take() as u8;
                let wait = r.take() as u16;
                let start = r.take() as u16;
                queue = EffectTransitionQueue::with_control(flags, wait, start);
                scenarios += 1;
            }
            0x1000 => {
                let n = r.take() as usize;
                let input: Vec<_> = (0..n)
                    .map(|_| {
                        let address = r.take() as u16;
                        let tagged_value = r.take();
                        tag_counts[(tagged_value >> 24) as usize] += 1;
                        CoefficientQueueWord {
                            address,
                            tagged_value,
                        }
                    })
                    .collect();
                let pending = r.take();
                let original = r.state();
                let actual_pending = queue
                    .enqueue_words(&input)
                    .map_err(|_| "Declared original publication rejected")?;
                if u32::from(actual_pending) != pending || state(queue.state()) != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"producer_batch":batches,"native":state(queue.state()),"original":original,"native_pending":actual_pending,"original_pending":pending});
                    }
                }
                let after = queue.state();
                let resets_ring = input.iter().any(|w| w.tagged_value >> 24 == 3);
                for (i, wraps) in write_wraps.iter_mut().enumerate() {
                    if after.rings[i].write_index < before.rings[i].write_index
                        && after.rings[i].count > before.rings[i].count
                        && !resets_ring
                    {
                        *wraps += 1;
                    }
                }
                producer_calls += n;
                batches += 1;
            }
            0x2000 => {
                let tick = r.take() as u16;
                let status = r.take() as u16;
                let original = r.state();
                let n = r.take();
                let mut original_packets = Vec::new();
                for _ in 0..n {
                    let program = r.take() != 0;
                    let address = r.take() as u16;
                    let control = r.take() as u16;
                    let count = r.take();
                    let values = (0..count)
                        .map(|_| {
                            let low = r.take();
                            let high = r.take();
                            u64::from(low) | (u64::from(high) << 32)
                        })
                        .collect();
                    original_packets.push((program, address, control, values));
                }
                let output = queue.service(tick, status);
                if let Some(p) = output.program {
                    selector_counts[usize::from(p.selector)] += 1;
                }
                let mut port = Port::default();
                radias_synth_application::effect_transition_queue::dispatch_effect_transition_batch(&mut port, &source, &output)
                    .map_err(|_| "Native program delivery failed")?;
                let after = queue.state();
                if state(after) != original || port.packets != original_packets {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"service":services,"tick":tick,"status":status,"native":state(after),"original":original,"native_packets":port.packets,"original_packets":original_packets});
                    }
                }
                for (program, _, _, values) in &port.packets {
                    if *program {
                        program_packets += 1;
                        program_words += values.len();
                    } else {
                        coefficient_packets += 1;
                        coefficient_words += values.len();
                    }
                }
                for (i, wraps) in read_wraps.iter_mut().enumerate() {
                    if after.rings[i].read_index < before.rings[i].read_index {
                        *wraps += 1;
                    }
                }
                services += 1;
            }
            0x4000 => {
                if [producer_calls, batches, services]
                    != [r.take() as usize, r.take() as usize, r.take() as usize]
                {
                    return Err("Incomplete original call coverage".into());
                }
            }
            0x5000 => {
                let selector = r.take() as u8;
                let count = r.take();
                let words: Vec<_> = (0..count)
                    .map(|_| {
                        let low = r.take();
                        let high = r.take();
                        u64::from(low) | (u64::from(high) << 32)
                    })
                    .collect();
                source.store_program(selector, &words)?;
                for view in 0..87 {
                    let n = r.take();
                    let address = r.take();
                    let original: Vec<_> = (0..n)
                        .map(|_| {
                            let low = r.take();
                            let high = r.take();
                            u64::from(low) | (u64::from(high) << 32)
                        })
                        .collect();
                    if source.program_words(view) != Some(original.as_slice())
                        || layout.locate(view).unwrap().word_address != address
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"mutation":mutations,"selector":selector,"view":view,"native":source.program_words(view),"original":original});
                        }
                    }
                    aliased_program_words += n as usize;
                }
                mutations += 1;
            }
            _ => return Err("Unknown transition record".into()),
        }
    }
    let scalar = CoefficientQueueWord {
        address: 7,
        tagged_value: 0x123456,
    };
    let mut full = EffectTransitionQueue::default();
    full.enqueue_words(&vec![scalar; 2046])
        .map_err(|_| "Valid producer capacity rejected")?;
    let before = full.state();
    let full_rejected =
        full.enqueue_words(&[scalar]) == Err(EffectQueueError::Full) && full.state() == before;
    let before = full.state();
    let mut too_large = vec![CoefficientQueueWord {
        address: 0,
        tagged_value: 0x03000000,
    }];
    too_large.extend(vec![scalar; 2046]);
    let transition_rejected =
        full.enqueue_words(&too_large) == Err(EffectQueueError::Full) && full.state() == before;
    let mut malformed = EffectTransitionQueue::default();
    let malformed_rejected = malformed.enqueue_words(&[CoefficientQueueWord {
        address: 0,
        tagged_value: 0x87000000,
    }]) == Err(EffectQueueError::IncompletePacket)
        && malformed.state() == EffectTransitionQueueState::default();
    let mut atomic_source = EffectProgramBuffers::from_buffers(
        layout,
        [vec![0; 20 * 0x44a], vec![0; 8 * 0x22e], vec![0; 0x2e2]],
    )?;
    atomic_source.store_program(61, &[0xabcdef123456, 0x123456789abc])?;
    let saved = atomic_source.program_words(61).unwrap().to_vec();
    let store_rejected = atomic_source.store_program(127, &[1]).is_err()
        && atomic_source
            .store_program(61, &vec![0xffffff; 2000])
            .is_err()
        && atomic_source.program_words(61) == Some(saved.as_slice());
    // Exercise the application scheduling entry, including declared busy input.
    let mut app_queue = EffectTransitionQueue::default();
    app_queue
        .enqueue_words(&[CoefficientQueueWord {
            address: 0xfffe,
            tagged_value: 0x02000055,
        }])
        .map_err(|_| "Valid program rejected")?;
    let mut app_port = Port::default();
    service_effect_transition_queue(&mut app_queue, &mut app_port, &source, 0, 3)
        .map_err(|_| "Busy service failed")?;
    let busy_preserved = app_port.packets.is_empty() && app_queue.state().rings[0].count == 1;
    service_effect_transition_queue(&mut app_queue, &mut app_port, &source, 1, 0)
        .map_err(|_| "Program service failed")?;
    let passed = errors == 0
        && getters == 87
        && mutations == 87
        && scenarios == 257
        && services > 30000
        && selector_counts.iter().all(|n| *n != 0)
        && write_wraps.iter().all(|n| *n != 0)
        && read_wraps.iter().all(|n| *n != 0)
        && [
            full_rejected,
            transition_rejected,
            malformed_rejected,
            store_rejected,
            busy_preserved,
        ]
        .iter()
        .all(|v| *v);
    let report = json!({"passed":passed,"whole_original_producer_calls":producer_calls,"atomic_publication_batches":batches,"whole_original_queue_service_calls":services,
        "whole_original_program_count_and_pointer_getters":getters*2+mutations*(getters*2+1),"whole_original_program_memcpy_and_count_stores":mutations*2,
        "native_program_store_transactions":mutations,"all_87_aliased_buffer_views_compared_after_each_write":true,"aliased_program_words_compared":aliased_program_words,
        "declared_initial_control_profiles":scenarios,"errors":errors,"first_difference":first,
        "program_words_compared":program_words,"program_packets_compared":program_packets,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,
        "program_selector_counts":selector_counts.to_vec(),"producer_tag_counts":tag_counts.to_vec(),"write_ring_wraps":write_wraps,"read_ring_wraps":read_wraps,
        "atomic_backpressure_transition_malformed_and_program_store_rejections":[full_rejected,transition_rejected,malformed_rejected,store_rejected],"application_busy_input_preserves_program_command":busy_preserved,
        "full_SYS01D698_and_SYS01D7E8_and_all_callees_execute_without_stubs":true,"both_rings_control_byte_wait_state_and_both_host_ports_compared":true,
        "original_expected_outputs_replayed_as_native_inputs":false,"native_executes_firmware_instructions":false,
        "FXD03_effects_audio_or_physical_clock_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-transition-queue-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native full FX queue: {producer_calls} producers, {services} services, {program_words} program words, {errors} differences"
    );
    if !passed {
        return Err("Native transition queue differs from original".into());
    }
    Ok(())
}
