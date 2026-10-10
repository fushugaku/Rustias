//! Whole SYS07E308 mixed rack load, mutable aliases and timed host delivery.
use radias_synth_application::{
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
    master_pressure_type_change::{
        MasterPressureTypeChangePort, rebuild_master_type_under_pressure,
    },
    timbre_output::restore_timbre_output,
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    dsp_control::DspEndpoint,
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_modulation::GrainModulationHistory,
    effect_program_staging::EffectProgramStaging,
    effect_rack_initialization::EffectRackInitializationContext,
    effect_rack_rebuild::{EffectRackRebuildContext, EffectRackRebuildStep},
    effect_transition_queue::{EffectRingState, EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{
        CoefficientChange, CoefficientQueueWord, CoefficientSlot, EffectCoefficientAssignments,
    },
    filter_effect::FilterEffectCache,
    insert_effect_construction::InsertEffectInstance,
    insert_effect_control::{InsertControlContext, InsertControlState},
    master_effect_construction::MasterEffectInstance,
    master_effect_control::{MasterControlState, MasterMidiBinding},
    master_pressure_type_change::PreparedMasterPressureTypeChange,
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState},
    program::Program,
    timbre_output::{TimbreOutputActor, TimbreOutputTables},
};
use radias_synth_infrastructure::{
    effect_program_buffers::EffectProgramBuffers,
    effects::EffectLibrary,
    timbre_output::{DspParameterMemory, NativeTimbreOutputPort},
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
            grain: core::array::from_fn(|_| self.bytes()),
            tempo: self.one() as u16,
            direct: self.one(),
            secondary: self.one(),
            pressure: self.one(),
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
    grain: [[u8; 36]; 9],
    tempo: u16,
    direct: u32,
    secondary: u32,
    pressure: u32,
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
    for (i, b) in midi.inserts.iter_mut().zip(s.grain) {
        i.grain_history = decode_grain(b);
    }
    midi.master.grain_history = decode_grain(s.grain[8]);
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
    for (b, i) in s.grain[..8].iter_mut().zip(state.midi.inserts) {
        *b = encode_grain(i.grain_history);
    }
    s.grain[8] = encode_grain(state.midi.master.grain_history);
    s.scratch = state.scratch;
    s.phases = phases;
    s.cursor = u32::from(state.staging.cursor);
    s.marker = state.midi.master.control.update_marker;
    s
}
struct Port {
    reject: bool,
    buffers: EffectProgramBuffers,
    steps: Vec<EffectRackRebuildStep>,
    tempo: u16,
    direct: u32,
    voice_port: NativeTimbreOutputPort,
    voice_tables: TimbreOutputTables,
    actors: [TimbreOutputActor; 24],
    bindings: [u32; 4],
    program: Program,
}
impl MasterPressureTypeChangePort for Port {
    type Error = ();
    fn accept_master_pressure_type_change(
        &mut self,
        p: &PreparedMasterPressureTypeChange,
    ) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        let mut voice_port = self.voice_port.clone();
        let mut steps = Vec::new();
        for step in p.steps() {
            let EffectRackRebuildStep::Effect(s) = &step else {
                let EffectRackRebuildStep::TimbreOutput { part, alternate } = step else {
                    unreachable!()
                };
                restore_timbre_output(
                    &mut voice_port,
                    &self.voice_tables,
                    &self.actors,
                    self.bindings,
                    &self.program,
                    part,
                    alternate,
                )
                .map_err(|_| ())?;
                steps.push(step);
                continue;
            };
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
            steps.push(step);
        }
        self.buffers = next;
        self.voice_port = voice_port;
        self.steps = steps;
        self.tempo = p.rebuild.tempo;
        self.direct = p.rebuild.direct_switch;
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

fn decode_grain(b: [u8; 36]) -> GrainModulationHistory {
    GrainModulationHistory {
        left: core::array::from_fn(|i| short(&b, i * 2) as i16),
        right: core::array::from_fn(|i| short(&b, 18 + i * 2) as i16),
        left_read: b[16],
        left_write: b[17],
        right_read: b[34],
        right_write: b[35],
    }
}
fn encode_grain(g: GrainModulationHistory) -> [u8; 36] {
    let mut b = [0u8; 36];
    for (i, v) in g.left.into_iter().enumerate() {
        b[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
    }
    for (i, v) in g.right.into_iter().enumerate() {
        b[18 + i * 2..20 + i * 2].copy_from_slice(&v.to_be_bytes());
    }
    b[16..18].copy_from_slice(&[g.left_read, g.left_write]);
    b[34..].copy_from_slice(&[g.right_read, g.right_write]);
    b
}
fn read_queue(
    r: &mut Reader,
) -> (
    EffectTransitionQueueState,
    [[CoefficientQueueWord; 2048]; 2],
) {
    let a = r.array::<9>();
    let state = EffectTransitionQueueState {
        rings: core::array::from_fn(|i| EffectRingState {
            write_index: a[i * 3] as u16,
            read_index: a[i * 3 + 1] as u16,
            count: a[i * 3 + 2] as u16,
        }),
        control: a[6] as u8,
        wait_ticks: a[7] as u16,
        wait_started: a[8] as u16,
    };
    let words = core::array::from_fn(|_| {
        core::array::from_fn(|_| CoefficientQueueWord {
            address: r.one() as u16,
            tagged_value: r.one(),
        })
    });
    (state, words)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.effect_rack_rebuild_tables()?;
    let raw = fs::read(root.join("runs/native-clone/master-pressure-type-change-original.bin"))?;
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
    if r.one() != 0x4d505331 {
        return Err("Wrong rack load corpus".into());
    }
    let bindings = r.array::<4>();
    let mut dsp_memories =
        core::array::from_fn(|_| DspParameterMemory::from_words(vec![0; 65536]).unwrap());
    for memory in &mut dsp_memories {
        for (base, count) in [(0x2000, 1920), (0x3000, 768), (0x3800, 256)] {
            for value in &mut memory.words_mut()[base..base + count] {
                *value = r.one() as u16;
            }
        }
    }
    let mut native_voices = NativeTimbreOutputPort::new(dsp_memories);
    let mut release_cases = 0usize;
    let mut release_crossings = 0usize;
    let mut pending_cases = 0usize;
    let mut incoming_counts = [0usize; 31];
    let mut dsp_receives = 0usize;
    let mut dsp_changes = 0usize;
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
            tempo: 0,
            direct: 0,
            voice_port: native_voices.clone(),
            voice_tables: library.timbre_output_tables()?,
            actors: [TimbreOutputActor {
                timbre_binding: 0,
                primary_selection: 0,
                flags: 0,
                endpoint: DspEndpoint::Master,
                parameter_origin: 0,
            }; 24],
            bindings,
            program: Program::from_bytes(&[0; 1790]).unwrap(),
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
                let incoming_kind = r.one();
                incoming_counts[incoming_kind as usize] += 1;
                state.midi.master.control.update_marker = (step + sequence) % 2;
                anchor.pressure = 0x80000000 | step;
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
                let raw_tempo = r.one() as u16;
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
                let program = Program::from_bytes(&r.bytes::<1790>())
                    .map_err(|_| "Invalid rebuild Program")?;
                port.program = program.clone();
                port.actors = core::array::from_fn(|_| {
                    let [binding, selection, flags, chip, origin] = r.array();
                    TimbreOutputActor {
                        timbre_binding: binding,
                        primary_selection: selection as u8,
                        flags: flags as u8,
                        endpoint: if chip == 0 {
                            DspEndpoint::Master
                        } else {
                            DspEndpoint::Slave
                        },
                        parameter_origin: origin as u16,
                    }
                });
                let n = r.one();
                for i in 0..n {
                    let [target, value] = r.array();
                    let p = state
                        .midi
                        .master
                        .control
                        .assignments
                        .prepare(CoefficientChange {
                            direct_switch: 0,
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
                anchor.tempo = tempo as u16;
                anchor.direct = direct;
                anchor.secondary = secondary;
                let before = r.snapshot();
                let (queue_before, queue_words_before) = read_queue(&mut r);
                release_cases += usize::from(state.midi.master.control.update_marker != 0);
                release_crossings += usize::from(
                    queue_before.rings[usize::from(queue_before.control & 1)].count < 1843,
                );
                pending_cases +=
                    usize::from((queue_before.control & 1) != ((queue_before.control >> 1) & 1));
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
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                let (queue_after, queue_words_after) = read_queue(&mut r);
                let n = r.one();
                let expected_dsp: Vec<_> = (0..n)
                    .map(|_| {
                        let chip = r.one();
                        let n = r.one();
                        (chip, (0..n).map(|_| r.one() as u16).collect::<Vec<_>>())
                    })
                    .collect();
                let n = r.one();
                let expected_outputs: Vec<_> = (0..n).map(|_| r.array::<3>()).collect();
                let n = r.one();
                let expected_dsp_changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
                let n = r.one();
                let extra_ram_changes: Vec<_> = (0..n).map(|_| r.array::<3>()).collect();
                let n = r.one();
                let expected_changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
                let rack_context = EffectRackInitializationContext {
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
                let context = EffectRackRebuildContext {
                    rack: rack_context,
                    raw_tempo,
                    queue: queue_before,
                };
                port.voice_port.clear_packets();
                let voices_before = port.voice_port.clone();
                let mut pressure_marker = anchor.pressure;
                let saved_pressure = pressure_marker;
                let saved = state;
                let saved_phases = phases;
                let buffers = port.buffers.clone();
                port.reject = true;
                port.steps.clear();
                if rebuild_master_type_under_pressure(
                    &mut state,
                    &mut pressure_marker,
                    &mut port,
                    &tables,
                    context,
                )
                .is_err()
                    && state == saved
                    && pressure_marker == saved_pressure
                    && port.steps.is_empty()
                    && port.voice_port == voices_before
                    && (0..3).all(|i| port.buffers.buffer_bytes(i) == buffers.buffer_bytes(i))
                {
                    rejected += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                rebuild_master_type_under_pressure(
                    &mut state,
                    &mut pressure_marker,
                    &mut port,
                    &tables,
                    context,
                )
                .map_err(|_| format!("Native rack load rejected {sequence}/{current}/{peer}"))?;
                let mut actual_lfos = Vec::new();
                let mut words = Vec::new();
                let mut outputs = Vec::new();
                for step in &port.steps {
                    let EffectRackRebuildStep::Effect(s) = step else {
                        let EffectRackRebuildStep::TimbreOutput { part, alternate } = step else {
                            unreachable!()
                        };
                        outputs.push([u32::from(*part), u32::from(*alternate), words.len() as u32]);
                        continue;
                    };
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
                let mut actual = project(&state, anchor, phases, &system);
                actual.tempo = port.tempo;
                actual.direct = port.direct;
                actual.pressure = pressure_marker;
                let prior = project(&saved, anchor, saved_phases, &system);
                let actual_dsp: Vec<_> = port
                    .voice_port
                    .packets()
                    .iter()
                    .map(|(endpoint, words)| {
                        (u32::from(*endpoint == DspEndpoint::Slave), words.clone())
                    })
                    .collect();
                let mut actual_dsp_changes = Vec::new();
                for (chip, memory) in voices_before.memories().iter().enumerate() {
                    for (base, count) in [(0x2000, 1920), (0x3000, 768), (0x3800, 256)] {
                        for address in base..base + count {
                            let old = memory.words()[address];
                            let new = port.voice_port.memories()[chip].words()[address];
                            if old != new {
                                actual_dsp_changes.push([
                                    chip as u32,
                                    address as u32,
                                    u32::from(old),
                                    u32::from(new),
                                ]);
                            }
                        }
                    }
                }
                dsp_receives += actual_dsp.len();
                dsp_changes += actual_dsp_changes.len();
                if actual_dsp != expected_dsp
                    || actual_dsp_changes != expected_dsp_changes
                    || outputs != expected_outputs
                    || prior != before
                    || actual != after
                    || words != original
                    || actual_lfos != expected_lfos
                    || changes != expected_changes
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"case":calls,"input":[sequence,current,peer,incoming_kind],"prior_matches":prior==before,"insert_differences":actual.midi.inserts.iter().flatten().zip(after.midi.inserts.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"master_differences":actual.midi.master.iter().zip(after.midi.master).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"assignments_match":actual.midi.assignments==after.midi.assignments,"caches_match":actual.midi.caches==after.midi.caches,"scratch_differences":actual.scratch.iter().zip(after.scratch).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"phase_differences":actual.phases.iter().flatten().zip(after.phases.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"grain_matches":actual.grain==after.grain,"cursor_marker":[actual.cursor,after.cursor,actual.marker,after.marker],"tempo":[actual.tempo,after.tempo],"pressure":[actual.pressure,after.pressure],"direct":[actual.direct,after.direct],"native_LFO":actual_lfos,"original_LFO":expected_lfos,"first_word_difference":words.iter().zip(&original).enumerate().find(|(_, (a,b))|a!=b).map(|(i,(a,b))|(i,format!("{a:?}"),format!("{b:?}"))),"native_outputs":outputs,"original_outputs":expected_outputs,"native_word_count":words.len(),"original_word_count":original.len(),"native_program_changes":changes.len(),"original_program_changes":expected_changes.len(),"native_DSP_packets":actual_dsp,"DSP_packets":expected_dsp,"native_DSP_changes":actual_dsp_changes,"original_DSP_changes":expected_dsp_changes,"extra_ram_changes":extra_ram_changes});
                    }
                }
                changed_bytes += changes.len();
                lfos += actual_lfos.len();
                steps += port.steps.len();
                maximum = maximum.max(words.len());
                let mut queue = EffectTransitionQueue::from_state(queue_before, queue_words_before)
                    .map_err(|_| "Invalid declared rebuild queue")?;
                queue
                    .enqueue_words(&words)
                    .map_err(|_| "Native rack queue rejected")?;
                if queue.state() != queue_after
                    || (0..2).any(|i| queue.ring_words(i).unwrap() != &queue_words_after[i])
                {
                    transport_errors += 1;
                    if first_transport.is_null() {
                        first_transport = json!({"case":calls,"queue_image_difference":true,"native_state":queue_state(queue.state()),"original_state":queue_state(queue_after)});
                    }
                }
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
                            first_transport = json!({"case":calls,"service":services,"input":[sequence,current,peer,incoming_kind],"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
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
                if queue.state().rings[usize::from((queue.state().control >> 1) & 1)].count != 0
                    || queue.state().wait_ticks != 0
                {
                    transport_errors += 1;
                }
                calls += 1;
            }
        }
        native_voices = port.voice_port;
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 3844
        && services == 1787167
        && prefills == 34596
        && rejected == calls
        && master_types == [124; 31]
        && insert_types == [[124; 31]; 8]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_pressure_Master_type_calls":calls,"optional_release_cases":release_cases,"release_threshold_crossing_cases":release_crossings,"pending_queue_switch_cases":pending_cases,"incoming_requested_kind_counts":incoming_counts.to_vec(),"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,"master_type_counts":master_types.to_vec(),"insert_type_counts":insert_types.map(|t|t.to_vec()),"errors":errors,"transport_errors":transport_errors,"first_difference":first,"first_transport_difference":first_transport,"whole_rack_program_and_queue_atomic_rejections":rejected,"ordered_LFO_publications":lfos,"ordered_native_rack_steps":steps,"changed_program_buffer_bytes_compared":changed_bytes,"coefficient_words_compared":coefficient_words,"program_words_compared":program_words,"maximum_rack_queue_words":maximum,"whole_original_SYS07C17E_pressure_branch_executed":true,"effect_rack_control_state_commands_buffers_and_queue_compared":true,"original_C55_command_receivers_execute_unchanged_images":true,"native_timbre_output_requests_and_order_verified":true,"timbre_output_requests_compared":calls*4,"native_active_voice_output_actions_and_DSP_memory_verified":true,"original_source_actor_bytes_and_flags_guards_preserved":true,"whole_original_active_voice_C55_receives":dsp_receives,"evolving_active_voice_DSP_word_changes_compared":dsp_changes,"FXD03_audio_and_physical_HPI_timing_verified":false,"native_control_outputs_replayed_from_original":false});
    fs::write(
        root.join("runs/native-clone/master-pressure-type-change-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native pressure Master type: {calls} calls, {services} services, {errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Pressure Master type branch differs".into());
    }
    Ok(())
}
