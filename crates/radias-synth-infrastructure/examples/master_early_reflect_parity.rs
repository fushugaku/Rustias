//! Whole original Master Early Reflect state, buffer mutations and timed host output.
use radias_synth_application::{
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
    master_effect_control::{MasterEffectPort, change_master_effect},
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_effect_control::{
        MasterControlState, MasterEdit, MasterMidiBinding, PreparedMasterEdit,
    },
};
use radias_synth_infrastructure::{
    effect_program_buffers::EffectProgramBuffers, effects::EffectLibrary,
};
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
}
struct Port {
    reject: bool,
    buffers: EffectProgramBuffers,
    batch: Option<EffectParameterBatch>,
    bodies: usize,
    prefix_tails: usize,
}
impl MasterEffectPort for Port {
    type Error = ();
    fn accept_master_edit(&mut self, p: &PreparedMasterEdit) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        for w in p.program_writes.iter().flatten() {
            next.store_program(w.selector, &[w.word]).map_err(|_| ())?;
        }
        if let Some(body) = &p.body_program {
            next.store_program(
                (body.blocks[0].tag & 127) as u8,
                &body.words[..usize::from(body.count)],
            )
            .map_err(|_| ())?;
        }
        self.buffers = next;
        self.bodies += usize::from(p.body_program.is_some());
        self.prefix_tails += p.program_writes.iter().flatten().count();
        self.batch = Some(p.batch);
        Ok(())
    }
}
#[derive(Default)]
struct Host {
    packets: Vec<(bool, u16, u16, Vec<u64>)>,
}
impl EffectProgramPort for Host {
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
fn state(rack: &MasterControlState, phase: &[u8; 32]) -> Vec<u32> {
    let mut w = rack.assignments.order.map(u32::from).to_vec();
    for s in rack.assignments.slots {
        w.extend(s.indices.map(u32::from));
        w.extend([s.target, s.last_value]);
    }
    w.extend([
        rack.midi_binding.source,
        u32::from(rack.midi_binding.values[0] as u8),
        u32::from(rack.midi_binding.values[1] as u8),
    ]);
    w.extend(rack.lfo.bytes.map(u32::from));
    w.extend(phase.map(u32::from));
    w.push(u32::from(rack.work_slot));
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
    let tables = library.master_control_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/master-early-reflect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master Early Reflect corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d455231 {
        return Err("Wrong Master Early Reflect corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut queue_errors,
        mut rejected,
        mut coefficient_words,
        mut program_words,
        mut coefficient_packets,
        mut program_packets,
        mut maximum,
        mut lfos,
        mut bodies,
        mut staged,
        mut changed_bytes,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut counts = [0usize; 20];
    let mut work_slots = [0usize; 29];
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master Early Reflect sequence changed".into());
        }
        let mut buffers: [Vec<u8>; 3] = core::array::from_fn(|_| Vec::new());
        for buffer in &mut buffers {
            let n = r.one();
            *buffer = (0..n).map(|_| r.one() as u8).collect();
        }
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), buffers)?,
            batch: None,
            bodies: 0,
            prefix_tails: 0,
        };
        let initial = r.array::<105>();
        let mut phase: [u8; 32] = initial[72..104]
            .iter()
            .map(|&v| v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let mut rack = MasterControlState {
            assignments: EffectCoefficientAssignments::new(indices),
            midi_binding: MasterMidiBinding {
                source: initial[63],
                values: [initial[64] as u8 as i8, initial[65] as u8 as i8],
            },
            lfo: EffectLfoProgram {
                bytes: initial[66..72]
                    .iter()
                    .map(|&v| v as u8)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap(),
            },
            work_slot: initial[104] as u8,
            coefficient_scratch: [0; 73],
            delay: DelayTimeState::default(),
            pending: [0; 2],
            pending_control: 0,
            owner: 0,
            update_marker: 0,
            filter_cache: FilterEffectCache::default(),
            rotary_mode: 0,
            rotary_speed: 0,
        };
        if state(&rack, &phase) != initial {
            return Err("Declared initial Master Early Reflect state differs".into());
        }
        let mut step = 0u32;
        let definition = &tables.definitions[12];
        for (parameter, range) in definition.ranges[..9].iter().enumerate() {
            for value in i32::from(range.minimum) + i32::from(range.encoded_zero)
                ..=i32::from(range.maximum) + i32::from(range.encoded_zero)
            {
                let [
                    tag,
                    seq,
                    num,
                    param,
                    arg_value,
                    direct,
                    owner,
                    clock,
                    marker,
                    origin,
                    prefix,
                    body,
                    relocation,
                    stored,
                    enabled,
                ] = r.array();
                if [tag, seq, num, param, arg_value]
                    != [0x2000, sequence, step, parameter as u32, value as u32]
                {
                    return Err("Master Early Reflect declared input changed".into());
                }
                let parameters = r.bytes();
                let previous_parameters = r.bytes();
                let midi = EffectMidiSources {
                    global_controls: r.array::<12>().map(|v| v as u16),
                    shared_control: r.one() as u8 as i8,
                    ..Default::default()
                };
                let polarity = EffectMidiPolarity {
                    assignments: r.bytes(),
                };
                let before = r.array::<105>();
                let after = r.array::<105>();
                let n = r.one();
                let mut changes = Vec::new();
                for _ in 0..n {
                    changes.push(r.array::<4>());
                }
                let n = r.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                let before_matches = state(&rack, &phase) == before;
                let edit = MasterEdit {
                    kind: 12,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters,
                    previous_parameters,
                    stored_owner: 0,
                    stored_effect_type: stored as u8,
                    stored_enabled: enabled != 0,
                    update_marker: 0,
                    origin: origin as u16,
                    owner,
                    direct_switch: direct,
                    clock_rate: clock,
                    clock: DelayClock {
                        tempo: 1200,
                        status: 0,
                    },
                    current_note: 60,
                    midi,
                    polarity,
                    prefix_origin: prefix as u16,
                    body_origin: body as u16,
                    relocation_origin: relocation as u16,
                    transition_marker: marker,
                };
                work_slots[usize::from(rack.work_slot)] += 1;
                let saved = rack;
                let saved_buffers = port.buffers.clone();
                port.reject = true;
                port.batch = None;
                if change_master_effect(&mut rack, &mut port, &tables, edit).is_err()
                    && rack == saved
                    && port.batch.is_none()
                    && (0..3).all(|i| port.buffers.buffer_bytes(i) == saved_buffers.buffer_bytes(i))
                {
                    rejected += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                change_master_effect(&mut rack, &mut port, &tables, edit)
                    .map_err(|_| format!("Native Master Early Reflect rejected: sequence={sequence} step={step} parameter={parameter} value={value} direct={direct} work_slot={} parameters={parameters:?}", rack.work_slot))?;
                let batch = port
                    .batch
                    .take()
                    .ok_or("Missing Master Early Reflect batch")?;
                if let Some(p) = batch.lfo_publication() {
                    phase[4..8].copy_from_slice(&p.tempo_increment.to_be_bytes());
                    lfos += 1;
                }
                let mut actual_changes = Vec::new();
                for bank in 0..3 {
                    for (i, (&old, &new)) in saved_buffers
                        .buffer_bytes(bank)
                        .unwrap()
                        .iter()
                        .zip(port.buffers.buffer_bytes(bank).unwrap())
                        .enumerate()
                    {
                        if old != new {
                            actual_changes.push([
                                bank as u32,
                                i as u32,
                                u32::from(old),
                                u32::from(new),
                            ]);
                        }
                    }
                }
                changed_bytes += actual_changes.len();
                if !before_matches
                    || state(&rack, &phase) != after
                    || batch.words() != original
                    || actual_changes != changes
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"case":calls,"input":[sequence,step,parameter as u32,value as u32,direct,marker],"before_matches":before_matches,"native_state":state(&rack,&phase),"original_state":after.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}"),"native_changed_bytes":actual_changes.len(),"original_changed_bytes":changes.len()});
                    }
                }
                maximum = maximum.max(batch.words().len());
                let mut queue = EffectTransitionQueue::default();
                queue
                    .enqueue_words(batch.words())
                    .map_err(|_| "Native queue rejected")?;
                let n = r.one();
                for _ in 0..n {
                    let tick = r.one() as u16;
                    let status = r.one() as u16;
                    let expected_state = r.array::<9>();
                    let n = r.one();
                    let mut expected = Vec::new();
                    for _ in 0..n {
                        let [program, address, control, count] = r.array();
                        let values = (0..count)
                            .map(|_| {
                                let [lo, hi] = r.array();
                                u64::from(lo) | (u64::from(hi) << 32)
                            })
                            .collect::<Vec<_>>();
                        expected.push((program != 0, address as u16, control as u16, values));
                    }
                    let output = queue.service(tick, status);
                    let mut host = Host::default();
                    dispatch_effect_transition_batch(&mut host, &port.buffers, &output)
                        .map_err(|_| "Native program delivery failed")?;
                    if queue_state(queue.state()) != expected_state || host.packets != expected {
                        queue_errors += 1;
                        if first.is_null() {
                            first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                        }
                    }
                    for p in &host.packets {
                        if p.0 {
                            program_words += p.3.len();
                            program_packets += 1;
                        } else {
                            coefficient_words += p.3.len();
                            coefficient_packets += 1;
                        }
                    }
                    services += 1;
                }
                if queue.state().rings.iter().any(|q| q.count != 0) {
                    queue_errors += 1;
                }
                calls += 1;
                counts[parameter] += 1;
                step += 1;
            }
        }
        bodies += port.bodies;
        staged += port.prefix_tails;
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls > 20000
        && rejected == calls
        && r.cursor == r.words.len()
        && bodies == 0
        && staged > 0;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts.to_vec(),"work_slot_counts":work_slots.to_vec(),"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_program_state_LFO_MIDI_atomic_rejections":rejected,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,"program_words_compared":program_words,"program_packets_compared":program_packets,"body_program_writes":bodies,"prefix_tail_program_writes":staged,"changed_buffer_bytes_compared":changed_bytes,"LFO_publications":lfos,"maximum_parameter_batch_words":maximum,"native_state_or_program_buffer_outputs_replayed_from_original":false,"all_eight_inserts_inactive_during_original_Master_MIDI":true,"mutable_native_program_buffers_used_for_host_delivery":true,"mixed_active_MIDI_master_initialization_FXD03_audio_or_full_engine_verified":false});
    fs::write(
        root.join("runs/native-clone/master-early-reflect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Master Early Reflect: {calls} edits, {services} services, {errors}/{queue_errors} differences, {bodies} body writes"
    );
    if !passed {
        return Err("Master Early Reflect differs".into());
    }
    Ok(())
}
