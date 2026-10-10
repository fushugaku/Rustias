//! Whole original Reverb insert edits, buffer/template state and timed queue.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, reverb_effect::change_reverb_parameter,
};
use radias_synth_domain::{
    delay_time::DelayClock,
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    reverb_effect::{ReverbEffectRack, ReverbParameterEdit},
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
        let v = self.words[self.cursor];
        self.cursor += 1;
        v
    }
    fn array<const N: usize>(&mut self) -> [u32; N] {
        core::array::from_fn(|_| self.one())
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
    }
    fn objects(&mut self) -> [[u8; 136]; 8] {
        core::array::from_fn(|_| self.bytes())
    }
}
#[derive(Default)]
struct Port {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl EffectParameterQueue for Port {
    type Error = ();
    fn enqueue_parameter(&mut self, p: &EffectParameterBatch) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*p);
        Ok(())
    }
}
fn read32(raw: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(raw[offset..offset + 4].try_into().unwrap())
}
fn read16(raw: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(raw[offset..offset + 2].try_into().unwrap())
}
fn write32(raw: &mut [u8], offset: usize, value: u32) {
    raw[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn write16(raw: &mut [u8], offset: usize, value: u16) {
    raw[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}
fn decode(raw: &[u8; 136]) -> EffectBufferInstance {
    EffectBufferInstance {
        kind: read32(raw, 4) as u8,
        origin: read16(raw, 0x40),
        parameters: raw[0x0c..0x20].try_into().unwrap(),
        buffer_origin: read32(raw, 0x50),
        layout: EffectBufferSlice {
            offset: read32(raw, 0x4c),
            frames: read32(raw, 0x48),
        },
        cached_tempo: read16(raw, 0x44),
        ratio: read32(raw, 0x54),
        limited: read32(raw, 0x58),
        pending_coefficients: [read32(raw, 0x7c), read32(raw, 0x80)],
        pending_argument: read32(raw, 0x84),
    }
}
fn store(raw: &mut [u8; 136], v: EffectBufferInstance) {
    write16(raw, 0x44, v.cached_tempo);
    write32(raw, 0x48, v.layout.frames);
    write32(raw, 0x4c, v.layout.offset);
    write32(raw, 0x54, v.ratio);
    write32(raw, 0x58, v.limited);
    write32(raw, 0x7c, v.pending_coefficients[0]);
    write32(raw, 0x80, v.pending_coefficients[1]);
    write32(raw, 0x84, v.pending_argument);
}
fn assignment_words(v: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut words = v.order.map(u32::from).to_vec();
    for slot in v.slots {
        words.extend(slot.indices.map(u32::from));
        words.extend([slot.target, slot.last_value]);
    }
    words
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.reverb_effect_tables()?;
    let initial_assignments =
        EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let sys_word = |a: u32| read32(&source, (a - 0x0c000000) as usize + 0x1000);
    let raw = fs::read(root.join("runs/native-clone/reverb-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Reverb corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x52565031 {
        return Err("Wrong Reverb corpus".into());
    }
    let (
        mut calls,
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
    let mut first = Value::Null;
    let mut counts = [0usize; 11];
    let mut neighbor_counts = [0usize; 31];
    let tempos = [200, 400, 1200, 3000];
    for sequence in 0..16u32 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Reverb sequence differs".into());
        }
        let mut arena: [[u8; 136]; 8] = core::array::from_fn(|slot| {
            core::array::from_fn(|i| (0x35 + sequence * 7 + slot as u32 * 11 + i as u32 * 13) as u8)
        });
        for (slot, v) in arena.iter_mut().enumerate() {
            write32(v, 0, slot as u32);
            write32(v, 4, 0);
            write32(v, 8, sys_word(0x0c0cceac));
            write16(v, 0x44, tempos[(sequence as usize + slot) % 4]);
        }
        if read.objects() != arena {
            return Err("Initial Reverb objects differ".into());
        }
        let scratch =
            core::array::from_fn(|i| 0x01020304 ^ (sequence * 0x10101) ^ (i as u32 * 0x010d0307));
        if read.array::<49>() != scratch {
            return Err("Declared Reverb scratch differs".into());
        }
        let mut rack = ReverbEffectRack {
            instances: arena.map(|a| decode(&a)),
            assignments: initial_assignments,
            scratch,
        };
        let mut step = 0u32;
        for (parameter, range) in tables.ranges.iter().enumerate() {
            for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
            {
                let [
                    tag,
                    scene,
                    number,
                    slot,
                    arg_parameter,
                    arg_value,
                    direct,
                    secondary,
                    owner1,
                    owner2,
                    neighbor,
                ] = read.array::<11>();
                if tag != 0x2000
                    || scene != sequence
                    || number != step
                    || slot != (sequence + step) % 8
                    || arg_parameter != parameter as u32
                    || arg_value != value as u32
                    || neighbor != (sequence + step) % 31
                {
                    return Err("Reverb input profile differs".into());
                }
                let origin = read.one();
                let base = read.one();
                let peer_origin = read.one();
                let peer_base = read.one();
                let parameters = read.bytes::<20>();
                let peer_parameters = read.bytes::<20>();
                let [tempo, status] = read.array::<2>();
                for (target, kind, p, origin, base) in [
                    (slot, 11, parameters, origin, base),
                    (slot ^ 1, neighbor, peer_parameters, peer_origin, peer_base),
                ] {
                    let a = &mut arena[target as usize];
                    write32(a, 4, kind);
                    write32(a, 8, sys_word(0x0c0cceac + 4 * kind));
                    a[0x0c..0x20].copy_from_slice(&p);
                    write16(a, 0x40, origin as u16);
                    write32(a, 0x50, base);
                }
                write32(&mut arena[slot as usize], 0x34, owner1);
                write32(&mut arena[slot as usize], 0x38, owner2);
                rack.instances = arena.map(|a| decode(&a));
                let before = read.array::<63>();
                let before_scratch = read.array::<49>();
                let before_matches = assignment_words(&rack.assignments) == before
                    && rack.scratch == before_scratch
                    && read.objects() == arena;
                let after = read.array::<63>();
                let after_scratch = read.array::<49>();
                let after_arena = read.objects();
                let n = read.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: read.one() as u16,
                        tagged_value: read.one(),
                    })
                    .collect();
                let edit = ReverbParameterEdit {
                    slot: slot as u8,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters,
                    origin: origin as u16,
                    owners: [owner1, owner2],
                    direct_switch: direct,
                    secondary_switch: secondary,
                    clock: DelayClock {
                        tempo: tempo as u16,
                        status: status as u8,
                    },
                };
                let saved = rack;
                let mut rejected = Port {
                    reject: true,
                    ..Default::default()
                };
                if change_reverb_parameter(&mut rack, &mut rejected, edit, &tables).is_ok()
                    || rack != saved
                    || rejected.batch.is_some()
                {
                    return Err("Rejected Reverb edit changed state".into());
                }
                rejections += 1;
                let mut port = Port::default();
                change_reverb_parameter(&mut rack, &mut port, edit, &tables).map_err(|_| {
                    format!("Reverb rejected {sequence}/{step}, parameter {parameter}")
                })?;
                let batch = port.batch.ok_or("Reverb commands missing")?;
                for (a, v) in arena.iter_mut().zip(rack.instances) {
                    store(a, v);
                }
                if !before_matches
                    || assignment_words(&rack.assignments) != after
                    || rack.scratch != after_scratch
                    || arena != after_arena
                    || batch.words() != original
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"sequence":sequence,"step":step,"slot":slot,"parameter":parameter,"value":value,"neighbor":neighbor,"direct":direct,"secondary":secondary,"before_matches":before_matches,"native_instances":arena.iter().map(|a|a.as_slice()).collect::<Vec<_>>(),"original_instances":after_arena.iter().map(|a|a.as_slice()).collect::<Vec<_>>(),"native_scratch":rack.scratch.as_slice(),"original_scratch":after_scratch.as_slice(),"native_assignments":assignment_words(&rack.assignments),"original_assignments":after.as_slice(),"native_queue":batch.words().iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>()});
                    }
                }
                let mut active = EffectCommandQueue::default();
                active
                    .enqueue_words(batch.words())
                    .map_err(|_| "Native Reverb queue rejected commands")?;
                let service_count = read.one();
                for _ in 0..service_count {
                    let [
                        tick,
                        status,
                        write_index,
                        read_index,
                        count,
                        wait_ticks,
                        wait_started,
                    ] = read.array::<7>();
                    let packet_count = read.one();
                    let mut original_packets = Vec::new();
                    for _ in 0..packet_count {
                        let a = read.one();
                        let c = read.one();
                        let n = read.one();
                        original_packets.push((
                            a,
                            c,
                            (0..n).map(|_| read.one()).collect::<Vec<_>>(),
                        ));
                    }
                    let output = active.service(tick as u16, status & 3 != 0);
                    let s = active.state();
                    let native = [
                        u32::from(s.write_index),
                        u32::from(s.read_index),
                        u32::from(s.count),
                        u32::from(s.wait_ticks),
                        u32::from(s.wait_started),
                    ];
                    let packets: Vec<_> = output.packets[..usize::from(output.count)]
                        .iter()
                        .map(|p| {
                            (
                                u32::from(p.address),
                                1,
                                p.values[..usize::from(p.count)].to_vec(),
                            )
                        })
                        .collect();
                    if native != [write_index, read_index, count, wait_ticks, wait_started]
                        || packets != original_packets
                    {
                        queue_errors += 1;
                    }
                    services += 1;
                    host_packets += packets.len();
                    host_words += packets.iter().map(|p| p.2.len()).sum::<usize>();
                }
                if active.state().count != 0 || active.state().wait_ticks != 0 {
                    return Err("Native Reverb queue did not drain".into());
                }
                maximum = maximum.max(batch.words().len());
                calls += 1;
                counts[parameter] += 1;
                neighbor_counts[neighbor as usize] += 1;
                step += 1;
            }
        }
    }
    let passed = calls == 17520
        && errors == 0
        && queue_errors == 0
        && rejections == calls
        && read.cursor == read.words.len()
        && neighbor_counts.iter().all(|&n| n != 0);
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts,"neighbor_type_counts":neighbor_counts.as_slice(),"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_transaction_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_parameter_batch_words":maximum,"all_eleven_insert_parameter_domains_complete":true,"all_31_neighbor_types":true,"all_1088_insert_bytes_assignment_cache_and_49_scratch_words_compared":true,"source_output_states_or_templates_replayed_as_inputs":false,"all_original_callees_execute_without_stubs":true,"master_parameter_wrapper_physical_clock_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/reverb-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Reverb insert: {calls} original edits, {errors} state/command and {queue_errors} queue differences"
    );
    if !passed {
        return Err("Native Reverb insert differs".into());
    }
    Ok(())
}
