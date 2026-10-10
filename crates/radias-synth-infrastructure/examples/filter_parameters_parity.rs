//! All St.Filter insert property domains and timed native queue service.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, filter_effect::change_filter_parameter,
};
use radias_synth_domain::{
    effect_control::{EffectBank, EffectKind},
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiSources, EffectMidiTimbre},
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_updates::EffectCoefficientAssignments,
    filter_effect::FilterEffectCache,
    filter_effect_parameters::{
        FilterEffectInstance, FilterEffectRack, FilterParameterEdit, FilterParameterTables,
    },
    program::Program,
};
use radias_synth_infrastructure::{effects::EffectLibrary, firmware::lfo_tempo_tables};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, b: &EffectParameterBatch) -> Result<(), Self::Error> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*b);
        Ok(())
    }
}
fn state_words(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut r = s.order.map(u32::from).to_vec();
    for slot in s.slots {
        r.extend(slot.indices.map(u32::from));
        r.extend([slot.target, slot.last_value]);
    }
    r
}
fn caches(rack: &FilterEffectRack) -> Vec<u32> {
    rack.caches
        .iter()
        .flat_map(|c| [c.frequency, c.dirty])
        .collect()
}
fn bindings(rack: &FilterEffectRack) -> Vec<u32> {
    rack.instances
        .iter()
        .flat_map(|i| {
            [
                i.controller_source,
                i32::from(i.controller_value) as u32,
                i32::from(i.secondary_value) as u32,
            ]
        })
        .collect()
}
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
        let value = self.words[self.cursor..self.cursor + N].try_into().unwrap();
        self.cursor += N;
        value
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let coefficients = library.filter_effect_tables()?;
    let tempo = lfo_tempo_tables(&source)?;
    let routing = library.routing_tables()?;
    let tables = FilterParameterTables {
        routing: &routing,
        coefficients: &coefficients,
        tempo: &tempo,
        lfo_mapping: library.lfo_mapping(EffectBank::Insert, EffectKind::new(4).unwrap())?,
    };
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/filter-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated St.Filter parameter corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x46505031 {
        return Err("Wrong St.Filter parameter corpus".into());
    }
    let minimum = [0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0];
    let maximum = [
        100, 4, 127, 127, 127, 1, 127, 127, 1, 127, 16, 4, 127, 1, 18, 12,
    ];
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xffe2, 0xfffe, 0xffff,
    ];
    let switches = [0, 1, 0x10000, 0x80000000];
    let clocks = [
        0,
        1,
        1200 * 7158,
        3000 * 7158,
        0x7fffffff,
        0x80000000,
        0xffffffff,
    ];
    let (
        mut cases,
        mut errors,
        mut queue_errors,
        mut rejected,
        mut services,
        mut host_words,
        mut host_packets,
        mut lfo_publications,
        mut largest_batch,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut counts = [0usize; 16];
    let mut first = Value::Null;
    for sequence in 0..16 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Source parameter sequence order differs".into());
        }
        let program =
            Program::from_bytes(&read.bytes::<1790>()).map_err(|_| "Invalid program inputs")?;
        let mut midi = EffectMidiSources::default();
        for timbre in &mut midi.timbres {
            let f = read.array::<10>();
            *timbre = EffectMidiTimbre {
                control_49: f[0] as i8,
                bend: f[1] as i16,
                control_4b: f[2] as i8,
                channel: f[3] as u8,
                switch_45: f[4] as u8,
                controls_4c_50: core::array::from_fn(|i| f[i + 5] as i8),
            };
        }
        for group in &mut midi.channel_controls {
            *group = read.bytes::<16>();
        }
        midi.shared_control = read.one() as i8;
        let mut rack = FilterEffectRack {
            instances: [FilterEffectInstance::default(); 8],
            caches: [FilterEffectCache::default(); 9],
            assignments: initial,
        };
        let mut phases = [[0u32; 32]; 8];
        for (slot, instance) in rack.instances.iter_mut().enumerate() {
            instance.parameters = read.bytes::<20>();
            instance.origin = read.one() as u16;
            instance.owners = read.array::<2>();
            instance.controller_source = read.one();
            instance.controller_value = read.one() as i8;
            instance.secondary_value = read.one() as i8;
            instance.lfo = EffectLfoProgram {
                bytes: read.bytes::<6>(),
            };
            phases[slot] = read.array::<32>();
        }
        let mut step = 0;
        for parameter in 0..16 {
            for value in minimum[parameter]..=maximum[parameter] {
                let [
                    tag,
                    src_sequence,
                    src_step,
                    slot,
                    src_parameter,
                    src_value,
                    origin,
                    direct,
                    first_owner,
                    second_owner,
                    clock,
                ] = read.array::<11>();
                if tag != 0x2000
                    || src_sequence != sequence
                    || src_step != step
                    || src_parameter != parameter as u32
                    || src_value != value
                    || slot != (sequence + step) % 8
                    || origin != origins[step as usize % 16]
                    || direct != switches[sequence as usize / 4]
                    || clock != clocks[step as usize % 7]
                {
                    return Err("Source edit profile differs".into());
                }
                let request = FilterParameterEdit {
                    slot: slot as u8,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters: read.bytes::<20>(),
                    origin: origin as u16,
                    owners: [first_owner, second_owner],
                    direct_switch: direct,
                    clock_rate: clock,
                };
                let before = rack;
                let before_phase = phases[slot as usize];
                let mut no_room = Queue {
                    reject: true,
                    ..Default::default()
                };
                if change_filter_parameter(
                    &mut rack,
                    &mut no_room,
                    request,
                    &tables,
                    &midi,
                    &program,
                )
                .is_err()
                    && rack == before
                    && no_room.batch.is_none()
                {
                    rejected += 1;
                }
                let mut queue = Queue::default();
                change_filter_parameter(&mut rack,&mut queue,request,&tables,&midi,&program).map_err(|_|format!("Native rejected source parameter {parameter}, value {value}, sequence {sequence}"))?;
                let batch = queue.batch.ok_or("St.Filter batch absent")?;
                let before_assignments = state_words(&before.assignments) == read.array::<63>();
                let before_cache = caches(&before) == read.array::<18>();
                let before_bindings = bindings(&before) == read.array::<24>();
                let before_lfo =
                    before.instances[slot as usize].lfo.bytes.map(u32::from) == read.array::<6>();
                let before_phase_ok = before_phase == read.array::<32>();
                let after_assignments = state_words(&rack.assignments) == read.array::<63>();
                let after_cache = caches(&rack) == read.array::<18>();
                let after_bindings = bindings(&rack) == read.array::<24>();
                let after_lfo =
                    rack.instances[slot as usize].lfo.bytes.map(u32::from) == read.array::<6>();
                if let Some(p) = batch.lfo_publication() {
                    for (i, b) in p.tempo_increment.to_be_bytes().into_iter().enumerate() {
                        phases[slot as usize][4 + i] = u32::from(b);
                    }
                    lfo_publications += 1;
                }
                let after_phase = phases[slot as usize] == read.array::<32>();
                let n = read.one() as usize;
                largest_batch = largest_batch.max(n);
                let original_queue: Vec<_> = (0..n).map(|_| read.array::<2>()).collect();
                let native_queue: Vec<_> = batch
                    .words()
                    .iter()
                    .map(|w| [u32::from(w.address), w.tagged_value])
                    .collect();
                if !before_assignments
                    || !before_cache
                    || !before_bindings
                    || !before_lfo
                    || !before_phase_ok
                    || !after_assignments
                    || !after_cache
                    || !after_bindings
                    || !after_lfo
                    || !after_phase
                    || native_queue != original_queue
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"sequence":sequence,"parameter":parameter,"value":value,"step":step,"before_assignments":before_assignments,"before_cache":before_cache,"before_bindings":before_bindings,"before_lfo":before_lfo,"after_assignments":after_assignments,"after_cache":after_cache,"after_bindings":after_bindings,"after_lfo":after_lfo,"after_phase":after_phase,"native_queue":native_queue,"original_queue":original_queue,"native_bindings":bindings(&rack),"native_cache":caches(&rack)});
                    }
                }
                let mut transport = EffectCommandQueue::default();
                transport
                    .enqueue_words(batch.words())
                    .map_err(|_| "Generated filter commands rejected")?;
                let ns = read.one();
                for _ in 0..ns {
                    let [
                        tick,
                        status,
                        write_index,
                        read_index,
                        count,
                        wait_ticks,
                        wait_started,
                    ] = read.array::<7>();
                    let np = read.one();
                    let mut original_packets = Vec::new();
                    for _ in 0..np {
                        let address = read.one();
                        let control = read.one();
                        let len = read.one();
                        let values: Vec<_> = (0..len).map(|_| read.one()).collect();
                        original_packets.push((address, control, values));
                    }
                    let output = transport.service(tick as u16, status & 3 != 0);
                    let state = transport.state();
                    let native_packets: Vec<_> = output.packets[..usize::from(output.count)]
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
                    ] != [write_index, read_index, count, wait_ticks, wait_started]
                        || native_packets != original_packets
                    {
                        queue_errors += 1;
                        if first.is_null() {
                            first = json!({"sequence":sequence,"parameter":parameter,"value":value,"tick":tick,"native_packets":native_packets,"original_packets":original_packets,"native_queue_state":[state.write_index,state.read_index,state.count,state.wait_ticks,state.wait_started],"original_queue_state":[write_index,read_index,count,wait_ticks,wait_started]});
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
                counts[parameter] += 1;
                step += 1;
            }
        }
        if step != 1060 {
            return Err("Source parameter domain incomplete".into());
        }
    }
    let passed = cases == 16960
        && read.cursor == read.words.len()
        && errors == 0
        && queue_errors == 0
        && rejected == cases;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":cases,"parameter_counts":counts,"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,
        "full_queue_atomic_rejections":rejected,"whole_original_timed_queue_services":services,"host_words_compared":host_words,"host_packets_compared":host_packets,"LFO_publications":lfo_publications,"maximum_parameter_batch_words":largest_batch,
        "all_sixteen_parameter_domains_complete":true,"continuous_sequences":16,"St_Filter_insert_instances":8,"original_program_bodies_are_declared_inputs":true,
        "external_host_status_and_16bit_timer_are_declared_inputs":true,"busy_masks_timer_expiration_and_timer_wrap_compared":true,
        "previous_controller_cache_and_binding_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,
        "mixed_effect_type_racks_master_wrapper_FXD03_sound_or_physical_timer_frequency_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/filter-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native St.Filter full controller: {cases} edits, {errors} differences; {services} queue services, {queue_errors} differences"
    );
    if !passed {
        return Err("Full native St.Filter controller differs".into());
    }
    Ok(())
}
