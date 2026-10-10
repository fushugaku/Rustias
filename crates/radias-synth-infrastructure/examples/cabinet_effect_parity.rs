use radias_synth_application::{
    cabinet_effect::change_cabinet_parameter, effect_parameters::EffectParameterQueue,
};
use radias_synth_domain::{
    cabinet_effect::{CabinetEffectRack, CabinetParameterEdit},
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_routing::EffectRoutingInstance,
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    program::Program,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
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
fn assignment_words(assignments: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut words = assignments.order.map(u32::from).to_vec();
    for slot in assignments.slots {
        words.extend(slot.indices.map(u32::from));
        words.extend([slot.target, slot.last_value]);
    }
    words
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.cabinet_tables()?;
    let routing = library.routing_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/cabinet-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Cabinet corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x43415031 {
        return Err("Wrong Cabinet corpus".into());
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
    let mut counts = [0usize; 4];
    let mut first = Value::Null;
    for sequence in 0..16 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Cabinet sequence order differs".into());
        }
        let program =
            Program::from_bytes(&read.bytes::<1790>()).map_err(|_| "Declared program rejected")?;
        let mut rack = CabinetEffectRack {
            instances: [EffectRoutingInstance::default(); 9],
            assignments: initial,
        };
        for instance in &mut rack.instances {
            *instance = EffectRoutingInstance {
                kind: read.one() as u8,
                origin: read.one() as u16,
                parameters: read.bytes(),
            };
        }
        for step in 0..368 {
            if read.one() != 0x2000 || read.one() != sequence || read.one() != step {
                return Err("Cabinet edit order differs".into());
            }
            let slot = read.one() as u8;
            let parameter = read.one() as u8;
            let value = read.one() as u8;
            let origin = read.one() as u16;
            let direct_switch = read.one();
            let owners = read.array();
            let parameters = read.bytes();
            let edit = CabinetParameterEdit {
                slot,
                parameter,
                value,
                origin,
                direct_switch,
                owners,
                parameters,
            };
            let before = read.array::<63>();
            let after = read.array::<63>();
            if assignment_words(&rack.assignments) != before {
                return Err(
                    "Native Cabinet before-state differs from continuous source state".into(),
                );
            }
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
            if change_cabinet_parameter(&mut rack, &mut rejected, edit, &tables, &routing, &program)
                .is_ok()
                || rack != saved
                || rejected.batch.is_some()
            {
                return Err("Cabinet rejection changed native state".into());
            }
            rejections += 1;
            let mut queue = Queue::default();
            change_cabinet_parameter(&mut rack, &mut queue, edit, &tables, &routing, &program)
                .map_err(|_| "Native Cabinet rejected declared edit")?;
            let native = queue.batch.ok_or("Cabinet publication missing")?;
            if native.words() != original || assignment_words(&rack.assignments) != after {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"step":step,"slot":slot,"parameter":parameter,"value":value,"parameters":parameters,"native_queue":native.words().iter().map(|w| [u32::from(w.address), w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w| [u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"native_assignments":assignment_words(&rack.assignments),"original_assignments":after.as_slice()});
                }
            }
            maximum = maximum.max(native.words().len());
            let mut transport = EffectCommandQueue::default();
            transport
                .enqueue_words(native.words())
                .map_err(|_| "Cabinet batch rejected by native queue")?;
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
                        first = json!({"sequence":sequence,"step":step,"parameter":parameter,"tick":tick,"native_state":native_state,"source_state":source_state,"native_packets":native_packets,"source_packets":source_packets});
                    }
                }
                services += 1;
                host_packets += native_packets.len();
                host_words += native_packets.iter().map(|p| p.2.len()).sum::<usize>();
            }
            if transport.state().count != 0 || transport.state().wait_ticks != 0 {
                queue_errors += 1;
            }
            cases += 1;
            counts[usize::from(parameter)] += 1;
        }
    }
    let passed = cases == 5888
        && read.cursor == read.words.len()
        && counts == [1616, 176, 2048, 2048]
        && errors == 0
        && queue_errors == 0
        && rejections == cases;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":cases,"whole_original_timed_queue_services":services,"parameter_counts":counts,"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_parameter_batch_words":maximum,"all_four_parameter_domains_complete":true,"all_eight_insert_instances":true,"continuous_sequences":16,"original_cabinet_tables_and_raw_programs_are_declared_inputs":true,"requested_type_and_stored_type_are_separate_inputs":true,"mixed_peer_routing_uses_shared_native_compiler":true,"prior_coefficient_or_assignment_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,"master_parameter_wrapper_FXD03_sound_or_physical_timer_frequency_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/cabinet-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native CabinetSimltr: {cases} original edits, {services} timed queue services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native Cabinet controller differs".into());
    }
    Ok(())
}
