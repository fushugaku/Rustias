//! Whole original initial Insert upload, evolving native rack/scratch and timed host delivery.
use radias_synth_application::{
    effect_transition_queue::{EffectProgramSource, dispatch_effect_transition_batch},
    effects::EffectProgramPort,
    insert_effect_initialization::{InsertInitializationPort, initialize_insert_effect},
};
use radias_synth_domain::{
    delay_time::DelayClock,
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_lfo_program::EffectLfoProgram,
    effect_modulation::GrainModulationHistory,
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::CoefficientQueueWord,
    insert_effect_construction::InsertEffectInstance,
    insert_effect_initialization::{
        InsertInitialization, InsertInitializationState, PreparedInsertInitialization,
    },
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
    fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            objects: core::array::from_fn(|_| self.bytes()),
            phases: core::array::from_fn(|_| self.bytes()),
            scratch: self.array(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    objects: [[u8; 136]; 8],
    phases: [[u8; 32]; 9],
    scratch: [u32; 73],
}
fn long(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}
fn short(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes(b[i..i + 2].try_into().unwrap())
}
fn put(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_be_bytes());
}
fn decode(b: [u8; 136]) -> InsertEffectInstance {
    InsertEffectInstance {
        slot: b[3],
        buffer: EffectBufferInstance {
            kind: b[7],
            origin: short(&b, 0x40),
            parameters: b[0x0c..0x20].try_into().unwrap(),
            buffer_origin: long(&b, 0x50),
            layout: EffectBufferSlice {
                frames: long(&b, 0x48),
                offset: long(&b, 0x4c),
            },
            cached_tempo: short(&b, 0x44),
            ratio: long(&b, 0x54),
            limited: long(&b, 0x58),
            pending_coefficients: [long(&b, 0x7c), long(&b, 0x80)],
            pending_argument: long(&b, 0x84),
        },
        previous_parameters: b[0x20..0x34].try_into().unwrap(),
        owners: [long(&b, 0x34), long(&b, 0x38)],
        lfo: EffectLfoProgram {
            bytes: b[0x5c..0x62].try_into().unwrap(),
        },
        controller_source: long(&b, 0x68),
        controller_values: [b[0x6c] as i8, b[0x6d] as i8],
        controller_offset: b[0x62],
        extended_program: long(&b, 0x64),
        enabled_argument: long(&b, 0x78),
        rotary_mode: long(&b, 0x70),
        rotary_speed: long(&b, 0x74),
        grain_history: GrainModulationHistory::default(),
    }
}
fn project(
    state: &InsertInitializationState,
    anchor: Snapshot,
    phases: [[u8; 32]; 9],
    system: &[u8],
) -> Snapshot {
    let mut objects = anchor.objects;
    for (b, i) in objects.iter_mut().zip(state.instances) {
        let v = i.buffer;
        put(b, 0, u32::from(i.slot));
        put(b, 4, u32::from(v.kind));
        put(
            b,
            8,
            long(system, 0x1000 + 0x0cceac + 4 * usize::from(v.kind)),
        );
        b[0x0c..0x20].copy_from_slice(&v.parameters);
        b[0x20..0x34].copy_from_slice(&i.previous_parameters);
        put(b, 0x34, i.owners[0]);
        put(b, 0x38, i.owners[1]);
        b[0x40..0x42].copy_from_slice(&v.origin.to_be_bytes());
        b[0x44..0x46].copy_from_slice(&v.cached_tempo.to_be_bytes());
        put(b, 0x48, v.layout.frames);
        put(b, 0x4c, v.layout.offset);
        put(b, 0x50, v.buffer_origin);
        put(b, 0x54, v.ratio);
        put(b, 0x58, v.limited);
        b[0x5c..0x62].copy_from_slice(&i.lfo.bytes);
        b[0x62] = i.controller_offset;
        put(b, 0x64, i.extended_program);
        put(b, 0x68, i.controller_source);
        b[0x6c] = i.controller_values[0] as u8;
        b[0x6d] = i.controller_values[1] as u8;
        put(b, 0x70, i.rotary_mode);
        put(b, 0x74, i.rotary_speed);
        put(b, 0x78, i.enabled_argument);
        put(b, 0x7c, v.pending_coefficients[0]);
        put(b, 0x80, v.pending_coefficients[1]);
        put(b, 0x84, v.pending_argument);
    }
    Snapshot {
        objects,
        phases,
        scratch: state.coefficient_scratch,
    }
}
#[derive(Default)]
struct Port {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl InsertInitializationPort for Port {
    type Error = ();
    fn accept_insert_initialization(&mut self, p: &PreparedInsertInitialization) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(p.batch);
        Ok(())
    }
}
struct NoPrograms;
impl EffectProgramSource for NoPrograms {
    fn program_words(&self, _: u8) -> Option<&[u64]> {
        None
    }
}
#[derive(Default)]
struct Host {
    packets: Vec<(u16, u16, Vec<u32>)>,
}
impl EffectProgramPort for Host {
    type Error = Infallible;
    fn upload_program(&mut self, _: u16, _: &[u64], _: u16) -> Result<(), Infallible> {
        panic!("Insert coefficient initialization uploaded a program")
    }
    fn write_coefficient(&mut self, a: u16, v: u32, c: u16) -> Result<(), Infallible> {
        self.write_coefficient_packet(a, &[v], c)
    }
    fn write_coefficient_packet(&mut self, a: u16, v: &[u32], c: u16) -> Result<(), Infallible> {
        self.packets.push((a, c, v.to_vec()));
        Ok(())
    }
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
    let tables = EffectLibrary::from_system(&system)?.insert_initialization_tables()?;
    let raw = fs::read(root.join("runs/native-clone/insert-initialization-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Insert initialization corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x49494e31 {
        return Err("Wrong Insert initial corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut lfos,
        mut coefficient_words,
        mut coefficient_packets,
        mut maximum,
        mut peer_changes,
        mut scratch_tail_changes,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut combinations = [[0usize; 31]; 31];
    let mut slots = [0usize; 8];
    let mut reverb_modes = [0usize; 3];
    let mut talking_modes = [0usize; 3];
    let mut first = Value::Null;
    for sequence in 0..2u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Insert initial sequence changed".into());
        }
        let anchor = r.snapshot();
        let mut phases = anchor.phases;
        let mut state = InsertInitializationState {
            instances: anchor.objects.map(decode),
            coefficient_scratch: anchor.scratch,
        };
        if project(&state, anchor, phases, &system) != anchor {
            return Err("Declared initial Insert state differs".into());
        }
        let mut port = Port::default();
        let mut step = 0;
        for slot in 0..8usize {
            for kind in 0..31u8 {
                for peer_kind in 0..31u8 {
                    if r.array::<6>()
                        != [
                            0x2000,
                            sequence,
                            step,
                            slot as u32,
                            u32::from(kind),
                            u32::from(peer_kind),
                        ]
                    {
                        return Err("Insert initial input changed".into());
                    }
                    for (target, k) in [(slot, kind), (slot ^ 1, peer_kind)] {
                        let v = &mut state.instances[target].buffer;
                        v.kind = k;
                        v.parameters = r.bytes();
                        v.origin = r.one() as u16;
                        v.buffer_origin = r.one();
                    }
                    let edit = InsertInitialization {
                        slot: slot as u8,
                        clock: DelayClock {
                            tempo: r.one() as u16,
                            status: r.one() as u8,
                        },
                        clock_rate: r.one(),
                    };
                    let before = r.snapshot();
                    let after = r.snapshot();
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let saved = state;
                    let saved_phases = phases;
                    port.reject = true;
                    port.batch = None;
                    if initialize_insert_effect(&mut state, &mut port, &tables, edit).is_err()
                        && state == saved
                        && port.batch.is_none()
                    {
                        rejected += 1
                    } else {
                        errors += 1
                    }
                    port.reject = false;
                    initialize_insert_effect(&mut state,&mut port,&tables,edit).map_err(|_|format!("Native Insert initial upload rejected: {sequence}/{slot}/{kind}/{peer_kind}"))?;
                    let batch = port.batch.take().ok_or("Missing Insert initial batch")?;
                    if let Some(p) = batch.lfo_publication() {
                        phases[usize::from(p.slot.raw())][4..8]
                            .copy_from_slice(&p.tempo_increment.to_be_bytes());
                        lfos += 1;
                    }
                    let prior = project(&saved, anchor, saved_phases, &system);
                    let actual = project(&state, anchor, phases, &system);
                    if prior != before
                        || actual != after
                        || batch.words() != original
                        || state
                            .instances
                            .iter()
                            .zip(saved.instances)
                            .any(|(a, b)| a.grain_history != b.grain_history)
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,slot as u32,u32::from(kind),u32::from(peer_kind)],"prior_matches":prior==before,"object_differences":actual.objects.iter().flatten().zip(after.objects.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"scratch_differences":actual.scratch.iter().zip(after.scratch).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"phases_match":actual.phases==after.phases,"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                        }
                    }
                    if state.instances[slot ^ 1].buffer != saved.instances[slot ^ 1].buffer {
                        peer_changes += 1;
                    }
                    let count = usize::from(match kind {
                        11 => {
                            tables.reverb[usize::from(state.instances[slot].buffer.parameters[1])]
                                .count
                        }
                        30 => {
                            tables.talking[usize::from(state.instances[slot].buffer.parameters[7])]
                                .count
                        }
                        _ => tables.coefficients[usize::from(kind)].count,
                    });
                    scratch_tail_changes += state.coefficient_scratch[count..]
                        .iter()
                        .zip(&saved.coefficient_scratch[count..])
                        .filter(|(a, b)| a != b)
                        .count();
                    maximum = maximum.max(batch.words().len());
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native Insert queue rejected")?;
                    let n = r.one();
                    for _ in 0..n {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        let expected_state = r.array::<9>();
                        let n = r.one();
                        let expected: Vec<_> = (0..n)
                            .map(|_| {
                                let [a, c, n] = r.array();
                                (
                                    a as u16,
                                    c as u16,
                                    (0..n).map(|_| r.one()).collect::<Vec<_>>(),
                                )
                            })
                            .collect();
                        let output = queue.service(tick, status);
                        let mut host = Host::default();
                        dispatch_effect_transition_batch(&mut host, &NoPrograms, &output)
                            .map_err(|_| "Native Insert host delivery failed")?;
                        if queue_state(queue.state()) != expected_state || host.packets != expected
                        {
                            transport_errors += 1;
                            if first.is_null() {
                                first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                            }
                        }
                        coefficient_words += host.packets.iter().map(|p| p.2.len()).sum::<usize>();
                        coefficient_packets += host.packets.len();
                        services += 1;
                    }
                    if queue.state().rings.iter().any(|r| r.count != 0) {
                        transport_errors += 1;
                    }
                    if kind == 11 {
                        reverb_modes[usize::from(state.instances[slot].buffer.parameters[1])] += 1;
                    }
                    if kind == 30 {
                        talking_modes[usize::from(state.instances[slot].buffer.parameters[7])] += 1;
                    }
                    combinations[usize::from(kind)][usize::from(peer_kind)] += 1;
                    slots[slot] += 1;
                    calls += 1;
                    step += 1;
                }
            }
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 15376
        && rejected == calls
        && combinations == [[16; 31]; 31]
        && slots == [1922; 8]
        && reverb_modes.iter().all(|&n| n > 0)
        && talking_modes.iter().all(|&n| n > 0)
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_insert_initial_upload_calls":calls,"whole_original_timed_queue_services":services,"all_pair_type_counts":combinations.iter().map(|r|r.to_vec()).collect::<Vec<_>>(),"slot_counts":slots,"Reverb_mode_counts":reverb_modes,"Talking_mode_counts":talking_modes,"errors":errors,"transport_errors":transport_errors,"first_difference":first,"whole_rack_state_and_queue_atomic_rejections":rejected,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,"LFO_publications":lfos,"maximum_initial_batch_words":maximum,"neighbor_state_changes":peer_changes,"scratch_tail_word_changes":scratch_tail_changes,"all_eight_instance_bytes_all_nine_phase_states_and_shared_scratch_compared":true,"native_evolving_instance_phase_or_scratch_outputs_replayed_from_original":false,"original_Master_and_all_Grain_history_guards_preserved":true,"program_preparation_initial_mask_full_rack_busy_rebuild_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/insert-initialization-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Insert initial coefficient upload: {calls} original calls, {services} services, {errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Insert initial upload differs".into());
    }
    Ok(())
}
