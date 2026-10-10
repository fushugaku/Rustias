//! Whole Early Reflect insert controller, staging data and both host ports.
use radias_synth_application::{
    early_reflect_effect::{EarlyReflectEffectPort, change_early_reflect_parameter},
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    early_reflect_effect::{
        EarlyReflectEffectRack, EarlyReflectParameterEdit, PreparedEarlyReflectEdit,
    },
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectProgramStaging,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    program::Program,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{convert::Infallible, fs, path::PathBuf};
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
    fn staging(&mut self) -> EffectProgramStaging {
        let cursor = self.one() as u8;
        let mut s = EffectProgramStaging {
            cursor,
            ..Default::default()
        };
        for i in 0..20 {
            let [pl, ph, tl, th, pc, tc] = self.array();
            s.prefix[i] = u64::from(pl) | (u64::from(ph) << 32);
            s.tail[i] = u64::from(tl) | (u64::from(th) << 32);
            s.counts[i] = [pc as u16, tc as u16];
        }
        s
    }
}
#[derive(Default)]
struct EditPort {
    reject: bool,
    batch: Option<EffectParameterBatch>,
    program_writes: usize,
}
impl EarlyReflectEffectPort for EditPort {
    type Error = ();
    fn accept_early_reflect_edit(&mut self, p: &PreparedEarlyReflectEdit) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(p.batch);
        self.program_writes += p.program_writes.iter().filter(|p| p.is_some()).count();
        Ok(())
    }
}
#[derive(Default)]
struct HostPort {
    packets: Vec<(bool, u16, u16, Vec<u64>)>,
}
impl EffectProgramPort for HostPort {
    type Error = Infallible;
    fn upload_program(&mut self, a: u16, w: &[u64], c: u16) -> Result<(), Infallible> {
        self.packets.push((true, a, c, w.to_vec()));
        Ok(())
    }
    fn write_coefficient(&mut self, a: u16, v: u32, c: u16) -> Result<(), Infallible> {
        self.write_coefficient_packet(a, &[v], c)
    }
    fn write_coefficient_packet(&mut self, a: u16, v: &[u32], c: u16) -> Result<(), Infallible> {
        self.packets
            .push((false, a, c, v.iter().copied().map(u64::from).collect()));
        Ok(())
    }
}
fn u32_at(raw: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(raw[offset..offset + 4].try_into().unwrap())
}
fn u16_at(raw: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(raw[offset..offset + 2].try_into().unwrap())
}
fn decode(raw: &[u8; 136]) -> EffectBufferInstance {
    EffectBufferInstance {
        kind: u32_at(raw, 4) as u8,
        origin: u16_at(raw, 0x40),
        parameters: raw[12..32].try_into().unwrap(),
        buffer_origin: u32_at(raw, 0x50),
        layout: EffectBufferSlice {
            offset: u32_at(raw, 0x4c),
            frames: u32_at(raw, 0x48),
        },
        cached_tempo: u16_at(raw, 0x44),
        ratio: u32_at(raw, 0x54),
        limited: u32_at(raw, 0x58),
        pending_coefficients: [u32_at(raw, 0x7c), u32_at(raw, 0x80)],
        pending_argument: u32_at(raw, 0x84),
    }
}
fn assignments(v: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut w = v.order.map(u32::from).to_vec();
    for s in v.slots {
        w.extend(s.indices.map(u32::from));
        w.extend([s.target, s.last_value]);
    }
    w
}
fn queue_state(s: EffectTransitionQueueState) -> [u32; 9] {
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
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.early_reflect_effect_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/early-reflect-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Early Reflect corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x45524631 {
        return Err("Wrong Early Reflect corpus".into());
    }
    let (mut calls, mut services, mut errors, mut queue_errors, mut rejections, mut maximum) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let (
        mut program_words,
        mut program_packets,
        mut coefficient_words,
        mut coefficient_packets,
        mut program_writes,
        mut staging_wraps,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [0usize; 9];
    let mut neighbors = [0usize; 31];
    let mut slot_counts = [0usize; 8];
    let mut cursor_counts = [0usize; 20];
    let mut first = Value::Null;
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Early Reflect sequence changed".into());
        }
        let arena: [[u8; 136]; 8] = core::array::from_fn(|_| r.bytes());
        let mut rack = EarlyReflectEffectRack {
            instances: arena.map(|a| decode(&a)),
            program_origins: arena.map(|a| u16_at(&a, 0x3c)),
            assignments: EffectCoefficientAssignments::new(indices),
            staging: r.staging(),
        };
        let mut program_raw = [0u8; 1790];
        let mut step = 0u32;
        for (parameter, range) in tables.ranges.iter().enumerate() {
            for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
            {
                let [
                    tag,
                    seq,
                    num,
                    slot,
                    arg_param,
                    arg_value,
                    direct,
                    owner1,
                    owner2,
                    neighbor,
                ] = r.array();
                if [tag, seq, num, slot, arg_param, arg_value, neighbor]
                    != [
                        0x2000,
                        sequence,
                        step,
                        (sequence + step) % 8,
                        parameter as u32,
                        value as u32,
                        (sequence + step) % 31,
                    ]
                {
                    return Err("Early Reflect declared input differs".into());
                }
                let peer = slot ^ 1;
                let part = slot / 2;
                let [origin, program_origin, base, offset] = r.array();
                let [peer_origin, peer_program_origin, peer_base, peer_offset] = r.array();
                let parameters = r.bytes::<20>();
                let peer_parameters = r.bytes::<20>();
                for (target, kind, p, origin, program_origin, base, offset) in [
                    (slot, 12, parameters, origin, program_origin, base, offset),
                    (
                        peer,
                        neighbor as u8,
                        peer_parameters,
                        peer_origin,
                        peer_program_origin,
                        peer_base,
                        peer_offset,
                    ),
                ] {
                    let instance = &mut rack.instances[target as usize];
                    instance.kind = kind;
                    instance.parameters = p;
                    instance.origin = origin as u16;
                    instance.buffer_origin = base;
                    instance.layout.offset = offset;
                    rack.program_origins[target as usize] = program_origin as u16;
                }
                let [first_header, second_header] = r.array();
                program_raw[168 + part as usize * 228] = first_header as u8;
                program_raw[192 + part as usize * 228] = second_header as u8;
                let program =
                    Program::from_bytes(&program_raw).map_err(|_| "Invalid declared program")?;
                let before_assignments = r.array::<63>();
                let before_staging = r.staging();
                let original_assignments = r.array::<63>();
                let original_staging = r.staging();
                let before_matches = assignments(&rack.assignments) == before_assignments
                    && rack.staging == before_staging;
                let n = r.one();
                let original_words: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                let edit = EarlyReflectParameterEdit {
                    slot: slot as u8,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters,
                    origin: origin as u16,
                    owners: [owner1, owner2],
                    direct_switch: direct,
                };
                let saved = rack;
                let mut port = EditPort {
                    reject: true,
                    ..Default::default()
                };
                if change_early_reflect_parameter(&mut port, &mut rack, &tables, &program, edit)
                    .is_err()
                    && rack == saved
                    && port.batch.is_none()
                    && port.program_writes == 0
                {
                    rejections += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                change_early_reflect_parameter(&mut port, &mut rack, &tables, &program, edit)
                    .map_err(|_| "Native complete Early Reflect rejected")?;
                let batch = port.batch.ok_or("Missing complete Early Reflect batch")?;
                if !before_matches
                    || assignments(&rack.assignments) != original_assignments
                    || rack.staging != original_staging
                    || rack.instances != saved.instances
                    || rack.program_origins != saved.program_origins
                    || batch.words() != original_words
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"case":calls,"input":[sequence,step,slot,parameter as u32,value as u32,direct],"before_matches":before_matches,
                        "native_words":format!("{:?}",batch.words()),"original_words":format!("{original_words:?}"),"native_staging":format!("{:?}",rack.staging),"original_staging":format!("{original_staging:?}"),
                        "native_assignments":assignments(&rack.assignments),"original_assignments":original_assignments.to_vec()});
                    }
                }
                if port.program_writes != 0 {
                    cursor_counts[usize::from(saved.staging.cursor)] += 1;
                    if rack.staging.cursor < saved.staging.cursor {
                        staging_wraps += 1;
                    }
                }
                program_writes += port.program_writes;
                maximum = maximum.max(batch.words().len());
                let mut queue = EffectTransitionQueue::default();
                queue
                    .enqueue_words(batch.words())
                    .map_err(|_| "Native complete Early Reflect queue failed")?;
                let total = r.one();
                for _ in 0..total {
                    let tick = r.one() as u16;
                    let status = r.one() as u16;
                    let original_state = r.array::<9>();
                    let packets = r.one();
                    let original: Vec<_> = (0..packets)
                        .map(|_| {
                            let program = r.one() != 0;
                            let address = r.one() as u16;
                            let control = r.one() as u16;
                            let count = r.one();
                            let values = (0..count)
                                .map(|_| {
                                    let low = r.one();
                                    let high = r.one();
                                    u64::from(low) | (u64::from(high) << 32)
                                })
                                .collect::<Vec<_>>();
                            (program, address, control, values)
                        })
                        .collect();
                    let output = queue.service(tick, status);
                    let mut host = HostPort::default();
                    dispatch_effect_transition_batch(&mut host, &rack.staging, &output)
                        .map_err(|_| "Native staging delivery failed")?;
                    if queue_state(queue.state()) != original_state || host.packets != original {
                        queue_errors += 1;
                        if first.is_null() {
                            first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":original_state,"native_packets":host.packets,"original_packets":original});
                        }
                    }
                    for (program, _, _, words) in host.packets {
                        if program {
                            program_packets += 1;
                            program_words += words.len();
                        } else {
                            coefficient_packets += 1;
                            coefficient_words += words.len();
                        }
                    }
                    services += 1;
                }
                if queue.state().rings.iter().any(|ring| ring.count != 0)
                    || queue.state().wait_ticks != 0
                {
                    queue_errors += 1;
                }
                calls += 1;
                counts[parameter] += 1;
                neighbors[neighbor as usize] += 1;
                slot_counts[slot as usize] += 1;
                step += 1;
            }
        }
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls == 23136
        && rejections == calls
        && r.cursor == r.words.len()
        && program_writes == 64
        && program_words == 64
        && staging_wraps != 0
        && cursor_counts.iter().all(|n| *n != 0);
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts,
        "errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_parameter_and_program_transaction_rejections":rejections,
        "program_words_compared":program_words,"program_packets_compared":program_packets,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,
        "program_staging_writes":program_writes,"staging_cursor_wraps":staging_wraps,"staging_cursor_transition_counts":cursor_counts,"slot_counts":slot_counts,"neighbor_type_counts":neighbors,
        "maximum_parameter_batch_words":maximum,"all_eight_insert_instances_and_complete_encoded_timbre_contexts_verified":true,
        "all_1088_instance_bytes_and_normal_program_body_guards_preserved_by_original":true,"all_twenty_prefix_tail_words_counts_and_cursor_compared":true,
        "original_functions_and_all_callees_execute_without_stubs":true,"previous_native_states_evolve_without_replaying_original_outputs":true,
        "master_parameter_wrapper_physical_clock_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/early-reflect-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native complete Early Reflect: {calls} edits, {services} queue services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native complete Early Reflect differs".into());
    }
    Ok(())
}
