//! Whole SYS07E308 mixed rack load, mutable aliases and timed host delivery.
use radias_synth_application::{
    effect_rack_initialization::{EffectRackInitializationPort, initialize_effect_rack},
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_modulation::GrainModulationHistory,
    effect_program_staging::EffectProgramStaging,
    effect_rack_initialization::{
        EffectRackInitializationContext, PreparedEffectRackInitialization,
    },
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{
        CoefficientChange, CoefficientQueueWord, CoefficientSlot, EffectCoefficientAssignments,
    },
    filter_effect::FilterEffectCache,
    insert_effect_construction::InsertEffectInstance,
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlStep},
    master_effect_construction::MasterEffectInstance,
    master_effect_control::{MasterControlState, MasterMidiBinding},
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState},
    program::Program,
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
    fn snapshot(&mut self) -> FullSnapshot {
        let inserts = core::array::from_fn(|_| self.bytes());
        let master = self.bytes();
        let assignments = self.array();
        let scratch = self.array();
        let phases = core::array::from_fn(|_| self.bytes());
        let caches = self.array();
        FullSnapshot {
            midi: Snapshot {
                inserts,
                master,
                assignments,
                caches,
            },
            scratch,
            phases,
            cursor: self.one(),
            marker: self.one(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot {
    inserts: [[u8; 136]; 8],
    master: [u8; 116],
    assignments: [u32; 63],
    caches: [u32; 18],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FullSnapshot {
    midi: Snapshot,
    scratch: [u32; 73],
    phases: [[u8; 32]; 9],
    cursor: u32,
    marker: u32,
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

fn assignments(b: [u32; 63]) -> EffectCoefficientAssignments {
    EffectCoefficientAssignments {
        order: core::array::from_fn(|i| b[i] as u8),
        slots: core::array::from_fn(|i| {
            let j = 9 + 6 * i;
            CoefficientSlot {
                indices: core::array::from_fn(|n| b[j + n] as u16),
                target: b[j + 4],
                last_value: b[j + 5],
            }
        }),
    }
}
fn initialize_midi(s: Snapshot) -> MixedEffectMidiState {
    let b = s.master;
    let mut inserts = s.inserts.map(decode);
    for i in &mut inserts {
        i.grain_history = GrainModulationHistory::default();
    }
    MixedEffectMidiState {
        inserts,
        insert_filter_caches: core::array::from_fn(|i| FilterEffectCache {
            frequency: s.caches[2 * i],
            dirty: s.caches[2 * i + 1],
        }),
        master: MasterEffectInstance {
            kind: b[3],
            parameters: b[8..28].try_into().unwrap(),
            previous_parameters: b[28..48].try_into().unwrap(),
            controller_offset: b[0x52],
            enabled_argument: long(&b, 0x64),
            grain_history: GrainModulationHistory::default(),
            control: MasterControlState {
                assignments: assignments(s.assignments),
                lfo: EffectLfoProgram {
                    bytes: b[0x4c..0x52].try_into().unwrap(),
                },
                delay: DelayTimeState {
                    cached_tempo: short(&b, 0x3c),
                    capacity: long(&b, 0x40),
                    ratio: long(&b, 0x44),
                    limited: long(&b, 0x48),
                },
                pending: [long(&b, 0x68), long(&b, 0x6c)],
                pending_control: long(&b, 0x70),
                owner: long(&b, 0x30),
                update_marker: 0,
                filter_cache: FilterEffectCache {
                    frequency: s.caches[16],
                    dirty: s.caches[17],
                },
                midi_binding: MasterMidiBinding {
                    source: long(&b, 0x54),
                    values: [b[0x58] as i8, b[0x59] as i8],
                },
                rotary_mode: long(&b, 0x5c),
                rotary_speed: long(&b, 0x60),
                work_slot: 0,
                coefficient_scratch: [0; 73],
            },
        },
    }
}
fn project_midi(state: &MixedEffectMidiState, anchor: Snapshot, system: &[u8]) -> Snapshot {
    let mut s = anchor;
    for (b, i) in s.inserts.iter_mut().zip(state.inserts) {
        put(b, 4, u32::from(i.buffer.kind));
        put(
            b,
            8,
            long(system, 0x1000 + 0x0cceac + 4 * usize::from(i.buffer.kind)),
        );
        b[0x0c..0x20].copy_from_slice(&i.buffer.parameters);
        b[0x40..0x42].copy_from_slice(&i.buffer.origin.to_be_bytes());
        put(b, 0x34, i.owners[0]);
        put(b, 0x38, i.owners[1]);
        put(b, 0x68, i.controller_source);
        b[0x6c] = i.controller_values[0] as u8;
        b[0x6d] = i.controller_values[1] as u8;
        put(b, 0x70, i.rotary_mode);
        put(b, 0x74, i.rotary_speed);
    }
    let i = state.master;
    put(&mut s.master, 0, u32::from(i.kind));
    put(
        &mut s.master,
        4,
        long(system, 0x1000 + 0x0ccf28 + 4 * usize::from(i.kind)),
    );
    s.master[8..28].copy_from_slice(&i.parameters);
    put(&mut s.master, 0x30, i.control.owner);
    put(&mut s.master, 0x54, i.control.midi_binding.source);
    s.master[0x58] = i.control.midi_binding.values[0] as u8;
    s.master[0x59] = i.control.midi_binding.values[1] as u8;
    put(&mut s.master, 0x5c, i.control.rotary_mode);
    put(&mut s.master, 0x60, i.control.rotary_speed);
    s.assignments[..9].copy_from_slice(&i.control.assignments.order.map(u32::from));
    for (n, slot) in i.control.assignments.slots.iter().enumerate() {
        let at = 9 + 6 * n;
        s.assignments[at..at + 4].copy_from_slice(&slot.indices.map(u32::from));
        s.assignments[at + 4] = slot.target;
        s.assignments[at + 5] = slot.last_value;
    }
    for (n, c) in state.insert_filter_caches.iter().enumerate() {
        s.caches[2 * n] = c.frequency;
        s.caches[2 * n + 1] = c.dirty;
    }
    s.caches[16] = i.control.filter_cache.frequency;
    s.caches[17] = i.control.filter_cache.dirty;
    s
}

fn staging(buffers: &EffectProgramBuffers, cursor: u8) -> EffectProgramStaging {
    let raw = buffers.buffer_bytes(0).unwrap();
    let word = |at: usize| {
        raw[at..at + 6]
            .iter()
            .fold(0u64, |v, &b| (v << 8) | u64::from(b))
    };
    EffectProgramStaging {
        cursor,
        prefix: core::array::from_fn(|s| word(s * 0x44a)),
        tail: core::array::from_fn(|s| word(s * 0x44a + 0x43e)),
        counts: core::array::from_fn(|s| {
            [short(raw, s * 0x44a + 0x444), short(raw, s * 0x44a + 0x448)]
        }),
    }
}
fn initialize(s: FullSnapshot, buffers: &EffectProgramBuffers) -> InsertControlState {
    let mut midi = initialize_midi(s.midi);
    midi.master.control.update_marker = s.marker;
    InsertControlState {
        midi,
        scratch: s.scratch,
        staging: staging(buffers, s.cursor as u8),
        prefix_origins: s.midi.inserts.map(|b| short(&b, 0x3c)),
        body_origins: s.midi.inserts.map(|b| short(&b, 0x3e)),
        relocation_origins: s.midi.inserts.map(|b| short(&b, 0x42)),
    }
}
fn project(
    state: &InsertControlState,
    anchor: FullSnapshot,
    phases: [[u8; 32]; 9],
    system: &[u8],
) -> FullSnapshot {
    let mut s = anchor;
    s.midi = project_midi(&state.midi, anchor.midi, system);
    for (n, (b, i)) in s
        .midi
        .inserts
        .iter_mut()
        .zip(state.midi.inserts)
        .enumerate()
    {
        let v = i.buffer;
        b[0x20..0x34].copy_from_slice(&i.previous_parameters);
        b[0x3c..0x3e].copy_from_slice(&state.prefix_origins[n].to_be_bytes());
        b[0x3e..0x40].copy_from_slice(&state.body_origins[n].to_be_bytes());
        b[0x42..0x44].copy_from_slice(&state.relocation_origins[n].to_be_bytes());
        b[0x44..0x46].copy_from_slice(&v.cached_tempo.to_be_bytes());
        put(b, 0x48, v.layout.frames);
        put(b, 0x4c, v.layout.offset);
        put(b, 0x50, v.buffer_origin);
        put(b, 0x54, v.ratio);
        put(b, 0x58, v.limited);
        b[0x5c..0x62].copy_from_slice(&i.lfo.bytes);
        b[0x62] = i.controller_offset;
        put(b, 0x64, i.extended_program);
        put(b, 0x78, i.enabled_argument);
        put(b, 0x7c, v.pending_coefficients[0]);
        put(b, 0x80, v.pending_coefficients[1]);
        put(b, 0x84, v.pending_argument);
    }
    let i = state.midi.master;
    let b = &mut s.midi.master;
    b[28..48].copy_from_slice(&i.previous_parameters);
    b[0x3c..0x3e].copy_from_slice(&i.control.delay.cached_tempo.to_be_bytes());
    put(b, 0x40, i.control.delay.capacity);
    put(b, 0x44, i.control.delay.ratio);
    put(b, 0x48, i.control.delay.limited);
    b[0x4c..0x52].copy_from_slice(&i.control.lfo.bytes);
    b[0x52] = i.controller_offset;
    put(b, 0x64, i.enabled_argument);
    put(b, 0x68, i.control.pending[0]);
    put(b, 0x6c, i.control.pending[1]);
    put(b, 0x70, i.control.pending_control);
    s.scratch = state.scratch;
    s.phases = phases;
    s.cursor = u32::from(state.staging.cursor);
    s.marker = state.midi.master.control.update_marker;
    s
}
struct Port {
    reject: bool,
    buffers: EffectProgramBuffers,
    steps: Vec<InsertControlStep>,
}
impl EffectRackInitializationPort for Port {
    type Error = ();
    fn accept_effect_rack_initialization(
        &mut self,
        p: &PreparedEffectRackInitialization,
    ) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        let mut steps = Vec::new();
        for s in p.steps() {
            for w in s.program_writes.iter().flatten() {
                next.store_program(w.selector, &[w.word]).map_err(|_| ())?;
            }
            if let Some(b) = s.body_program {
                for block in &b.blocks[..usize::from(b.block_count)] {
                    let start = usize::from(block.word_start);
                    next.store_program(
                        (block.tag & 127) as u8,
                        &b.words[start..start + usize::from(block.count)],
                    )
                    .map_err(|_| ())?;
                }
            }
            steps.push(*s);
        }
        self.buffers = next;
        self.steps = steps;
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
    let tables = library.effect_rack_initialization_tables()?;
    let raw = fs::read(root.join("runs/native-clone/whole-rack-initial-load-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated rack load corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x52494c31 {
        return Err("Wrong rack load corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut prefills,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut changed_bytes,
        mut coefficient_words,
        mut program_words,
        mut lfos,
        mut steps,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize,
    );
    let mut master_types = [0usize; 31];
    let mut insert_types = [[0usize; 31]; 8];
    let mut first = Value::Null;
    let mut first_transport = Value::Null;
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Rack load sequence changed".into());
        }
        let mut bytes: [Vec<u8>; 3] = core::array::from_fn(|_| Vec::new());
        for b in &mut bytes {
            let n = r.one();
            *b = (0..n).map(|_| r.one() as u8).collect();
        }
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), bytes)?,
            steps: Vec::new(),
        };
        let mut anchor = r.snapshot();
        let mut phases = anchor.phases;
        let mut state = initialize(anchor, &port.buffers);
        if project(&state, anchor, phases, &system) != anchor {
            return Err("Initial rack differs".into());
        }
        for current in 0..31u32 {
            for peer in 0..31u32 {
                let [
                    tag,
                    seq,
                    step,
                    c,
                    p,
                    direct,
                    secondary,
                    clock_rate,
                    tempo,
                    status,
                ] = r.array();
                if [tag, seq, step, c, p] != [0x2000, sequence, current * 31 + peer, current, peer]
                {
                    return Err("Rack input changed".into());
                }
                for (slot, types) in insert_types.iter_mut().enumerate() {
                    let kind = r.one() as u8;
                    let parameters = r.bytes();
                    let [origin, prefix, body, relocation, owner1, owner2] = r.array();
                    let i = &mut state.midi.inserts[slot];
                    i.buffer.kind = kind;
                    i.buffer.parameters = parameters;
                    i.buffer.origin = origin as u16;
                    i.owners = [owner1, owner2];
                    state.prefix_origins[slot] = prefix as u16;
                    state.body_origins[slot] = body as u16;
                    state.relocation_origins[slot] = relocation as u16;
                    types[usize::from(kind)] += 1;
                }
                let master_kind = r.one() as u8;
                let master_parameters = r.bytes();
                let [origin, prefix, body, relocation, owner] = r.array();
                state.midi.master.kind = master_kind;
                state.midi.master.parameters = master_parameters;
                state.midi.master.control.owner = owner;
                master_types[usize::from(master_kind)] += 1;
                let program =
                    Program::from_bytes(&r.bytes::<1790>()).map_err(|_| "Invalid rack Program")?;
                let polarity = EffectMidiPolarity {
                    assignments: r.bytes(),
                };
                let mut midi = EffectMidiSources::default();
                let mut notes = [0u8; 5];
                for (part, t) in midi.timbres.iter_mut().enumerate() {
                    let a = r.array::<11>();
                    *t = EffectMidiTimbre {
                        control_49: a[0] as i8,
                        bend: a[1] as i16,
                        control_4b: a[2] as i8,
                        channel: a[3] as u8,
                        switch_45: a[4] as u8,
                        controls_4c_50: core::array::from_fn(|i| a[5 + i] as i8),
                    };
                    notes[part] = a[10] as u8;
                }
                for g in &mut midi.channel_controls {
                    *g = r.bytes();
                }
                midi.global_controls = r.array::<12>().map(|v| v as u16);
                midi.shared_control = r.one() as i8;
                notes[4] = r.one() as u8;
                let n = r.one();
                for i in 0..n {
                    let [target, value] = r.array();
                    let p = state
                        .midi
                        .master
                        .control
                        .assignments
                        .prepare(CoefficientChange {
                            direct_switch: direct,
                            standalone: false,
                            enabled_argument: 1,
                            mode: (i % 2) as u8,
                            target,
                            value,
                        });
                    state.midi.master.control.assignments = p.next;
                    prefills += 1;
                }
                anchor.midi.master[0x34..0x36].copy_from_slice(&(prefix as u16).to_be_bytes());
                anchor.midi.master[0x36..0x38].copy_from_slice(&(body as u16).to_be_bytes());
                anchor.midi.master[0x38..0x3a].copy_from_slice(&(origin as u16).to_be_bytes());
                anchor.midi.master[0x3a..0x3c].copy_from_slice(&(relocation as u16).to_be_bytes());
                let before = r.snapshot();
                let after = r.snapshot();
                let n = r.one();
                let expected_lfos: Vec<_> = (0..n)
                    .map(|_| {
                        let slot = r.one();
                        let b = r.bytes::<6>();
                        let increment = r.one();
                        (slot, b, increment)
                    })
                    .collect();
                let n = r.one();
                let expected_changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
                let n = r.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                let context = EffectRackInitializationContext {
                    common: InsertControlContext {
                        program: &program,
                        midi: MixedEffectMidiFrame {
                            midi,
                            polarity,
                            current_notes: notes,
                            master_origin: origin as u16,
                            direct_switch: direct,
                            force_refresh: false,
                        },
                        secondary_switch: secondary,
                        clock_rate,
                        clock: DelayClock {
                            tempo: tempo as u16,
                            status: status as u8,
                        },
                    },
                    master_prefix: prefix as u16,
                    master_body: body as u16,
                    master_relocation: relocation as u16,
                };
                let saved = state;
                let saved_phases = phases;
                let buffers = port.buffers.clone();
                port.reject = true;
                port.steps.clear();
                if initialize_effect_rack(&mut state, &mut port, &tables, context).is_err()
                    && state == saved
                    && port.steps.is_empty()
                    && (0..3).all(|i| port.buffers.buffer_bytes(i) == buffers.buffer_bytes(i))
                {
                    rejected += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                initialize_effect_rack(&mut state, &mut port, &tables, context).map_err(|_| {
                    format!("Native rack load rejected {sequence}/{current}/{peer}")
                })?;
                let mut actual_lfos = Vec::new();
                let mut words = Vec::new();
                for s in &port.steps {
                    words.extend_from_slice(s.batch.words());
                    if let Some(p) = s.batch.lfo_publication() {
                        phases[usize::from(p.slot.raw())][4..8]
                            .copy_from_slice(&p.tempo_increment.to_be_bytes());
                        actual_lfos.push((
                            u32::from(p.slot.raw()),
                            p.program.bytes,
                            p.tempo_increment,
                        ));
                    }
                }
                let mut changes = Vec::new();
                for bank in 0..3 {
                    for (i, (&old, &new)) in buffers
                        .buffer_bytes(bank)
                        .unwrap()
                        .iter()
                        .zip(port.buffers.buffer_bytes(bank).unwrap())
                        .enumerate()
                    {
                        if old != new {
                            changes.push([bank as u32, i as u32, u32::from(old), u32::from(new)]);
                        }
                    }
                }
                let actual = project(&state, anchor, phases, &system);
                let prior = project(&saved, anchor, saved_phases, &system);
                let histories_preserved =
                    state
                        .midi
                        .inserts
                        .iter()
                        .zip(saved.midi.inserts)
                        .all(|(a, b)| {
                            a.previous_parameters == b.previous_parameters
                                && a.grain_history == b.grain_history
                        })
                        && state.midi.master.previous_parameters
                            == saved.midi.master.previous_parameters
                        && state.midi.master.grain_history == saved.midi.master.grain_history;
                if !histories_preserved
                    || prior != before
                    || actual != after
                    || words != original
                    || actual_lfos != expected_lfos
                    || changes != expected_changes
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"case":calls,"input":[sequence,current,peer],"prior_matches":prior==before,"insert_differences":actual.midi.inserts.iter().flatten().zip(after.midi.inserts.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"master_differences":actual.midi.master.iter().zip(after.midi.master).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"assignments_match":actual.midi.assignments==after.midi.assignments,"native_caches":actual.midi.caches,"original_caches":after.midi.caches,"scratch_differences":actual.scratch.iter().zip(after.scratch).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"cursor_marker":[actual.cursor,after.cursor,actual.marker,after.marker],"phase_matches":actual.phases==after.phases,"native_LFO":actual_lfos,"original_LFO":expected_lfos,"native_words":format!("{words:?}"),"original_words":format!("{original:?}"),"native_program_changes":changes.len(),"original_program_changes":expected_changes.len()});
                    }
                }
                changed_bytes += changes.len();
                lfos += actual_lfos.len();
                steps += port.steps.len();
                maximum = maximum.max(words.len());
                let mut queue = EffectTransitionQueue::default();
                queue
                    .enqueue_words(&words)
                    .map_err(|_| "Native rack queue rejected")?;
                let n = r.one();
                for _ in 0..n {
                    let tick = r.one() as u16;
                    let status = r.one() as u16;
                    let expected_state = r.array::<9>();
                    let n = r.one();
                    let expected: Vec<_> = (0..n)
                        .map(|_| {
                            let [pr, a, c, n] = r.array();
                            (
                                pr != 0,
                                a as u16,
                                c as u16,
                                (0..n)
                                    .map(|_| {
                                        let [lo, hi] = r.array();
                                        u64::from(lo) | (u64::from(hi) << 32)
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect();
                    let output = queue.service(tick, status);
                    let mut host = Host::default();
                    dispatch_effect_transition_batch(&mut host, &port.buffers, &output)
                        .map_err(|_| "Native rack delivery failed")?;
                    if queue_state(queue.state()) != expected_state || host.packets != expected {
                        transport_errors += 1;
                        if first_transport.is_null() {
                            first_transport = json!({"case":calls,"service":services,"input":[sequence,current,peer],"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                        }
                    }
                    for p in host.packets {
                        if p.0 {
                            program_words += p.3.len();
                        } else {
                            coefficient_words += p.3.len();
                        }
                    }
                    services += 1;
                }
                if queue.state().rings.iter().any(|r| r.count != 0) || queue.state().wait_ticks != 0
                {
                    transport_errors += 1;
                }
                calls += 1;
            }
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 3844
        && services == 1088346
        && prefills == 2196
        && rejected == calls
        && master_types == [124; 31]
        && insert_types == [[124; 31]; 8]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_rack_initial_load_calls":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,"master_type_counts":master_types.to_vec(),"insert_type_counts":insert_types.map(|t|t.to_vec()),"errors":errors,"transport_errors":transport_errors,"first_difference":first,"first_transport_difference":first_transport,"whole_rack_program_and_queue_atomic_rejections":rejected,"ordered_LFO_publications":lfos,"ordered_native_rack_steps":steps,"changed_program_buffer_bytes_compared":changed_bytes,"coefficient_words_compared":coefficient_words,"program_words_compared":program_words,"maximum_rack_queue_words":maximum,"whole_original_SYS07E308_compared":true,"evolving_native_rack_phase_scratch_or_buffers_replayed_from_original":false,"native_previous_history_and_Grain_fields_preserved":true,"whole_original_Grain_history_guards_preserved":true,"mutable_aliases_read_from_final_buffers_at_timed_queue_delivery":true,"busy_rebuild_live_parameter_boundary_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-rack-initialization-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native whole rack load: {calls} calls, {services} services, {errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Whole rack load differs".into());
    }
    Ok(())
}
