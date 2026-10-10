//! Whole St.2BandEQ/Distortion dispatches, assignments, bindings and timed host packets.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, equalizer_effect::change_equalizer_parameter,
};
use radias_synth_domain::{
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    equalizer_effect::{
        EqualizerEffectInstance, EqualizerEffectKind, EqualizerEffectRack, EqualizerParameterEdit,
    },
    program::Program,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};
struct Reader {
    words: Vec<u32>,
    cursor: usize,
}
impl Reader {
    fn one(&mut self) -> u32 {
        let value = self.words[self.cursor];
        self.cursor += 1;
        value
    }
    fn array<const N: usize>(&mut self) -> [u32; N] {
        core::array::from_fn(|_| self.one())
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
    }
}
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, b: &EffectParameterBatch) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*b);
        Ok(())
    }
}
fn assignment_words(a: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut w = a.order.map(u32::from).to_vec();
    for slot in a.slots {
        w.extend(slot.indices.map(u32::from));
        w.extend([slot.target, slot.last_value]);
    }
    w
}
fn bindings(rack: &EqualizerEffectRack) -> Vec<u32> {
    rack.instances.iter().flat_map(|i| i.owners).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let core = library.equalizer_tables()?;
    let tables = library.equalizer_effect_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/equalizer-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated EQ dispatch corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x45515031 {
        return Err("Wrong EQ dispatch corpus".into());
    }
    let (
        mut cases,
        mut errors,
        mut queue_errors,
        mut services,
        mut rejections,
        mut host_words,
        mut host_packets,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut counts = [[0usize; 15]; 2];
    let mut first = Value::Null;
    let mut seen = BTreeSet::new();
    let mut six_word_packets = 0usize;
    for sequence in 0..16 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("EQ scene order differs".into());
        }
        let program =
            Program::from_bytes(&read.bytes::<1790>()).map_err(|_| "Declared program rejected")?;
        let mut rack = EqualizerEffectRack {
            instances: [EqualizerEffectInstance::default(); 8],
            assignments: initial,
        };
        for step in 0..1958 {
            let [
                tag,
                scene,
                number,
                kind,
                slot,
                parameter,
                value,
                origin,
                direct,
                owner1,
                owner2,
            ] = read.array::<11>();
            if tag != 0x2000 || scene != sequence || number != step || slot != (sequence + step) % 8
            {
                return Err("EQ edit order differs".into());
            }
            let parameters = read.bytes();
            let previous_parameters = read.bytes();
            let before = read.array::<63>();
            let before_bindings = read.array::<16>();
            let after = read.array::<63>();
            let after_bindings = read.array::<16>();
            let edit = EqualizerParameterEdit {
                kind: EqualizerEffectKind::from_type(kind as u8).ok_or("Unexpected EQ type")?,
                slot: slot as u8,
                parameter: parameter as u8,
                value: value as u8,
                origin: origin as u16,
                direct_switch: direct,
                owners: [owner1, owner2],
                parameters,
                previous_parameters,
            };
            // Explicit caller-side binding edit, not a prior source output.
            rack.instances[slot as usize].owners = edit.owners;
            let matches_before =
                assignment_words(&rack.assignments) == before && bindings(&rack) == before_bindings;
            let n = read.one() as usize;
            let original: Vec<_> = (0..n)
                .map(|_| CoefficientQueueWord {
                    address: read.one() as u16,
                    tagged_value: read.one(),
                })
                .collect();
            let saved = rack;
            let mut rejected = Queue {
                reject: true,
                ..Default::default()
            };
            if change_equalizer_parameter(&mut rack, &mut rejected, edit, &tables, &core, &program)
                .is_ok()
                || rack != saved
                || rejected.batch.is_some()
            {
                return Err("EQ queue rejection changed native state".into());
            }
            rejections += 1;
            let mut queue = Queue::default();
            change_equalizer_parameter(&mut rack, &mut queue, edit, &tables, &core, &program)
                .map_err(|_| "Native EQ rejected declared edit")?;
            let native = queue.batch.ok_or("EQ publication missing")?;
            if !matches_before
                || native.words() != original
                || assignment_words(&rack.assignments) != after
                || bindings(&rack) != after_bindings
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"step":step,"kind":kind,"slot":slot,"parameter":parameter,"value":value,"parameters":parameters,"previous_parameters":previous_parameters,"matches_before":matches_before,"native_queue":native.words().iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"native_assignments":assignment_words(&rack.assignments),"original_assignments":after.as_slice(),"native_bindings":bindings(&rack),"original_bindings":after_bindings});
                }
            }
            maximum = maximum.max(native.words().len());
            let mut transport = EffectCommandQueue::default();
            transport
                .enqueue_words(native.words())
                .map_err(|_| "EQ queue rejected publication")?;
            let total = read.one();
            for _ in 0..total {
                let tick = read.one() as u16;
                let status = read.one();
                let source_state = read.array::<5>();
                let packet_count = read.one();
                let mut source_packets = Vec::new();
                for _ in 0..packet_count {
                    let address = read.one();
                    let control = read.one();
                    let count = read.one();
                    source_packets.push((
                        address,
                        control,
                        (0..count).map(|_| read.one()).collect::<Vec<_>>(),
                    ));
                }
                let batch = transport.service(tick, status & 3 != 0);
                let state = transport.state();
                let native_state = [
                    state.write_index,
                    state.read_index,
                    state.count,
                    state.wait_ticks,
                    state.wait_started,
                ]
                .map(u32::from);
                let native_packets: Vec<_> = batch.packets[..usize::from(batch.count)]
                    .iter()
                    .map(|p| {
                        (
                            u32::from(p.address),
                            1,
                            p.values[..usize::from(p.count)].to_vec(),
                        )
                    })
                    .collect();
                if source_state != native_state || source_packets != native_packets {
                    queue_errors += 1;
                    if first.is_null() {
                        first = json!({"sequence":sequence,"step":step,"kind":kind,"parameter":parameter,"tick":tick,"native_state":native_state,"source_state":source_state,"native_packets":native_packets,"source_packets":source_packets});
                    }
                }
                services += 1;
                host_packets += native_packets.len();
                host_words += native_packets.iter().map(|p| p.2.len()).sum::<usize>();
                six_word_packets += native_packets.iter().filter(|p| p.2.len() == 6).count();
            }
            if transport.state().count != 0 || transport.state().wait_ticks != 0 {
                queue_errors += 1;
            }
            cases += 1;
            counts[kind as usize - 6][parameter as usize] += 1;
            seen.insert((sequence, kind, parameter, value));
        }
    }
    let mut expected = BTreeSet::new();
    for sequence in 0..16 {
        for (family, definition) in tables.definitions.iter().enumerate() {
            for (parameter, range) in definition.ranges[..usize::from(definition.parameter_count)]
                .iter()
                .enumerate()
            {
                for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                    ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
                {
                    expected.insert((sequence, 6 + family as u32, parameter as u32, value as u32));
                }
            }
        }
    }
    let passed = cases == 31328
        && read.cursor == read.words.len()
        && seen == expected
        && errors == 0
        && queue_errors == 0
        && rejections == cases
        && six_word_packets > 0;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":cases,"whole_original_timed_queue_services":services,"parameter_counts":counts,"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"six_word_host_packets_compared":six_word_packets,"maximum_parameter_batch_words":maximum,"all_25_EQ_and_distortion_parameter_domains_complete":true,"all_eight_insert_instances":true,"continuous_sequences":16,"current_requested_and_previous_parameter_bytes_are_separate_inputs":true,"owner_rebinding_and_zero_gain_transitions_compared":true,"all_original_callees_execute_without_stubs":true,"prior_coefficient_assignment_or_binding_outputs_replayed_as_native_inputs":false,"master_parameter_wrapper_or_FXD03_sound_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/equalizer-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native St.2BandEQ/Distortion: {cases} whole edits, {services} timed services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native EQ/Distortion dispatch differs".into());
    }
    Ok(())
}
