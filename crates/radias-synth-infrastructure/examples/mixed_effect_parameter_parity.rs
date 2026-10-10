//! Whole mixed parameter callbacks, including all stored dynamics mix bytes.
use radias_synth_application::{
    effect_header_event::{EffectHeaderEventPort, change_effect_header},
    effect_property::{EffectPropertyPort, set_effect_property},
    effect_transition_queue::dispatch_effect_transition_batch,
    effect_value_change::{EffectValueChangePort, EffectValueChangeTables, change_effect_value},
    effects::EffectProgramPort,
    insert_parameter_caller::{InsertParameterCallerPort, change_insert_parameter},
    insert_paired_initialization::{InsertPairedInitializationPort,initialize_insert_pair},
    master_initial_mask::MasterInitialMaskPort,
    master_parameter_caller::{MasterParameterCallerPort, change_master_parameter},
    master_type_value_change::{
        MasterTypeValuePort, MasterTypeValueTables, change_master_type_value,
    },
    mixed_effect_parameter::{
        MixedEffectParameterPort, change_mixed_effect_parameter,
        change_stored_mixed_effect_parameter,
    },
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_header_event::{EffectHeaderChange, EffectHeaderEvent},
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_modulation::GrainModulationHistory,
    effect_program_staging::EffectProgramStaging,
    effect_property::{EffectProperty, EffectPropertyChange, PreparedEffectProperty},
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{
        CoefficientChange, CoefficientQueueWord, CoefficientSlot, EffectCoefficientAssignments,
    },
    effect_value_change::PreparedEffectValueChange,
    filter_effect::FilterEffectCache,
    insert_effect_construction::InsertEffectInstance,
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlStep},
    insert_parameter_caller::InsertParameterChange,
    insert_paired_initialization::PreparedInsertPairedInitialization,
    master_effect_construction::MasterEffectInstance,
    master_effect_control::{MasterControlState, MasterMidiBinding},
    master_initial_mask::{MasterInitialMaskContext, PreparedMasterInitialMask},
    master_parameter_caller::PreparedMasterParameterCaller,
    master_type_value_change::{MasterTypeValueContext, PreparedMasterTypeValueChange},
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState},
    mixed_effect_parameter::{
        EffectParameterTarget, MixedEffectParameterEdit, PreparedMixedEffectParameter,
    },
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
fn decode_grain(b: [u8; 36]) -> GrainModulationHistory {
    GrainModulationHistory {
        left: core::array::from_fn(|i| short(&b, 2 * i) as i16),
        right: core::array::from_fn(|i| short(&b, 18 + 2 * i) as i16),
        left_read: b[16],
        left_write: b[17],
        right_read: b[34],
        right_write: b[35],
    }
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
impl MasterInitialMaskPort for Port {
    type Error = ();
    fn accept_master_initial_mask(&mut self, p: &PreparedMasterInitialMask) -> Result<(), ()> {
        self.accept_steps(p.steps())
    }
}
impl Port {
    fn accept_steps<'a>(
        &mut self,
        source: impl Iterator<Item = &'a InsertControlStep>,
    ) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        let mut steps = Vec::new();
        for s in source {
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
impl MixedEffectParameterPort for Port {
    type Error = ();
    fn accept_mixed_effect_parameter(
        &mut self,
        prepared: &PreparedMixedEffectParameter,
    ) -> Result<(), ()> {
        let mut steps = [None; 20];
        steps[0] = Some(prepared.step);
        self.accept_master_initial_mask(&PreparedMasterInitialMask {
            next: prepared.next,
            steps,
            step_count: 1,
        })
    }
}
impl MasterParameterCallerPort for Port {
    type Error = ();
    fn accept_master_parameter_caller(
        &mut self,
        prepared: &PreparedMasterParameterCaller,
    ) -> Result<(), ()> {
        let mut steps = [None; 20];
        steps[..prepared.step_count].copy_from_slice(&prepared.steps[..prepared.step_count]);
        self.accept_master_initial_mask(&PreparedMasterInitialMask {
            next: prepared.next,
            steps,
            step_count: prepared.step_count,
        })
    }
}
impl InsertParameterCallerPort for Port {
    type Error = ();
    fn accept_insert_parameter_caller(
        &mut self,
        prepared: &PreparedMasterParameterCaller,
    ) -> Result<(), ()> {
        self.accept_master_parameter_caller(prepared)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    if std::env::args().nth(2).as_deref() == Some("--properties") {
        return compare_properties(&root);
    }
    if std::env::args().nth(2).as_deref() == Some("--header-events") {
        return compare_header_events(&root, false);
    }
    if std::env::args().nth(2).as_deref() == Some("--header-value") {
        return compare_header_events(&root, true);
    }
    let pair_init = std::env::args().nth(2).as_deref() == Some("--paired-initialization");
    let raw_mix = std::env::args().nth(2).as_deref() == Some("--raw-dynamics-mix");
    let full_value = std::env::args().nth(2).as_deref() == Some("--value-change");
    let type_value = std::env::args().nth(2).as_deref() == Some("--master-type-value");
    let master_caller = std::env::args().nth(2).as_deref() == Some("--master-parameter-caller");
    let insert_caller = std::env::args().nth(2).as_deref() == Some("--insert-parameter-caller");
    let caller = master_caller || insert_caller || full_value || type_value;
    let apply = if raw_mix {
        change_stored_mixed_effect_parameter::<Port>
    } else {
        change_mixed_effect_parameter::<Port>
    };
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.insert_control_tables()?;
    let caller_tables = library.effect_parameter_caller_tables()?;
    let property_tables = library.effect_property_tables()?;
    let master_initialization = library.master_initialization_tables()?;
    let insert_initialization = library.insert_initialization_tables()?;
    let insert_programs = library.insert_program_initialization_tables()?;
    let output = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(root.join(if pair_init {"runs/native-clone/insert-paired-initialization-original.bin.gz"} else if type_value {
            "runs/native-clone/master-type-value-original.bin.gz"
        } else if full_value {
            "runs/native-clone/effect-value-change-original.bin.gz"
        } else if insert_caller {
            "runs/native-clone/insert-parameter-caller-original.bin.gz"
        } else if master_caller {
            "runs/native-clone/master-parameter-caller-original.bin.gz"
        } else if raw_mix {
            "runs/native-clone/raw-dynamics-mix-original.bin.gz"
        } else {
            "runs/native-clone/mixed-effect-parameter-original.bin.gz"
        }))
        .output()?;
    if !output.status.success() {
        return Err("Cannot decode mixed-parameter corpus".into());
    }
    let raw = output.stdout;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Insert initial mask corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one()
        != if pair_init {0x49504931} else if type_value {
            0x4d545631
        } else if full_value {
            0x56414c31
        } else {
            0x4d455031
        }
    {
        return Err("Wrong Insert initial mask corpus".into());
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
    let mut coverage = [[[0u16; 3]; 20]; 31];
    let mut raw_coverage = [[0u16; 256]; 3];
    let mut type_coverage = [[0u16; 31]; 31];
    let mut pair_coverage = [[0u16;31];31];
    let mut peer_initializations = 0usize;
    let mut types = [0usize; 31];
    let mut slots = [0usize; 9];
    let mut first = Value::Null;
    let mut first_transport = Value::Null;
    let mut errors_by_kind = [0usize; 31];
    let mut transport_by_kind = [0usize; 31];
    let mut caller_releases = 0usize;
    let mut caller_time_swaps = 0usize;
    let mut caller_owner_changes = 0usize;
    let mut caller_program_changes = 0usize;
    let mut caller_history_changes = 0usize;
    let mut master_grain_resets = 0usize;
    for sequence in 0..if caller { 16u32 } else { 4u32 } {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Insert initial mask sequence changed".into());
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
            return Err("Declared initial Insert mask state differs".into());
        }
        let mut sequence_step = 0u32;
        for slot in if master_caller || type_value {
            8usize..9
        } else if insert_caller || pair_init {
            0usize..8
        } else {
            0..9usize
        } {
            for kind in if raw_mix { 1..4u8 } else { 0..31u8 } {
                let parameter_count = if slot == 8 {
                    tables.common.definitions[usize::from(kind)].parameter_count
                } else {
                    tables.definitions[usize::from(kind)].parameter_count
                };
                for parameter in 0..if raw_mix || type_value || pair_init {
                    1
                } else {
                    parameter_count
                } {
                    for variation in 0..if type_value || pair_init {
                        31u32
                    } else if raw_mix {
                        256u32
                    } else {
                        3u32
                    } {
                        let args = r.array::<19>();
                        let step = sequence_step;
                        sequence_step += 1;
                        if args[..7]
                            != [
                                0x2000,
                                sequence,
                                step,
                                slot as u32,
                                u32::from(kind),
                                parameter as u32,
                                variation,
                            ]
                        {
                            return Err("Mixed parameter input changed".into());
                        }
                        let [
                            _,
                            _,
                            _,
                            _,
                            _,
                            _,
                            _,
                            value,
                            direct,
                            secondary,
                            clock_rate,
                            tempo,
                            status,
                            origin,
                            prefix,
                            body,
                            relocation,
                            owner1,
                            owner2,
                        ] = args;
                        let range = if slot == 8 {
                            tables.common.definitions[usize::from(kind)].ranges[parameter]
                        } else {
                            tables.definitions[usize::from(kind)].ranges[parameter]
                        };
                        let lo = i32::from(range.minimum) + i32::from(range.encoded_zero);
                        let hi = i32::from(range.maximum) + i32::from(range.encoded_zero);
                        let expected = if pair_init {0} else if type_value {
                            variation as i32
                        } else if full_value {
                            (if variation == 0 {
                                lo
                            } else if variation == 1 {
                                hi
                            } else {
                                (lo + hi) / 2
                            }) - i32::from(range.encoded_zero)
                        } else if raw_mix {
                            variation as i32
                        } else if variation == 0 {
                            lo
                        } else if variation == 1 {
                            hi
                        } else {
                            (lo + hi) / 2
                        };
                        if value != expected as u32 {
                            return Err("Mixed boundary value changed".into());
                        }
                        if pair_init {pair_coverage[kind as usize][variation as usize]+=1;} else if type_value {
                            type_coverage[usize::from(kind)][variation as usize] += 1;
                        } else if raw_mix {
                            raw_coverage[usize::from(kind - 1)][variation as usize] += 1;
                        } else {
                            coverage[usize::from(kind)][parameter][variation as usize] += 1;
                        }
                        let parameters = r.bytes();
                        let raw_patch = r.bytes::<24>();
                        let caller_fields = if caller { r.array::<3>() } else { [0; 3] };
                        let peer_fields = if pair_init {Some((r.one(),r.bytes::<20>(),r.array::<6>()))} else {None};
                        let requested = raw_patch[(if slot == 8 { 2 } else { 4 }) + parameter];
                        if !full_value && !type_value && u32::from(requested) != value {
                            return Err("Mixed requested byte differs".into());
                        }
                        let program = Program::from_bytes(&r.bytes::<1790>())
                            .map_err(|_| "Invalid initial-mask Program")?;
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
                        if slot == 8 {
                            state.midi.master.kind = kind;
                            state.midi.master.parameters = parameters;
                            state.midi.master.control.owner = owner1;
                            if master_caller || full_value || type_value {
                                state.midi.master.previous_parameters[parameter] =
                                    caller_fields[0] as u8;
                                state.midi.master.controller_offset = caller_fields[1] as u8;
                                state.midi.master.control.update_marker = caller_fields[2];
                            }
                        } else {
                            let instance = &mut state.midi.inserts[slot];
                            instance.buffer.kind = kind;
                            instance.buffer.parameters = parameters;
                            instance.buffer.origin = origin as u16;
                            instance.owners = [owner1, owner2];
                            if insert_caller || full_value {
                                instance.previous_parameters[parameter] = caller_fields[0] as u8;
                                instance.controller_offset = caller_fields[1] as u8;
                                state.midi.master.control.update_marker = caller_fields[2];
                            }
                            state.prefix_origins[slot] = prefix as u16;
                            state.body_origins[slot] = body as u16;
                            state.relocation_origins[slot] = relocation as u16;
                        }
                        if let Some((extended, parameters, [origin,prefix,body,relocation,owner1,owner2]))=peer_fields {
                            state.midi.inserts[slot].extended_program=extended;
                            let peer=slot^1;let instance=&mut state.midi.inserts[peer];
                            instance.buffer.kind=variation as u8;instance.buffer.parameters=parameters;instance.buffer.origin=origin as u16;instance.owners=[owner1,owner2];
                            state.prefix_origins[peer]=prefix as u16;state.body_origins[peer]=body as u16;state.relocation_origins[peer]=relocation as u16;
                        }
                        let n = r.one();
                        for i in 0..n {
                            let [target, value] = r.array();
                            let p =
                                state
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
                        if slot == 8 {
                            anchor.midi.master[0x34..0x36]
                                .copy_from_slice(&(prefix as u16).to_be_bytes());
                            anchor.midi.master[0x36..0x38]
                                .copy_from_slice(&(body as u16).to_be_bytes());
                            anchor.midi.master[0x38..0x3a]
                                .copy_from_slice(&(origin as u16).to_be_bytes());
                            anchor.midi.master[0x3a..0x3c]
                                .copy_from_slice(&(relocation as u16).to_be_bytes());
                        }
                        let before = r.snapshot();
                        if type_value {
                            state.midi.master.grain_history = decode_grain(r.bytes());
                        }
                        let expected_value = if full_value || type_value {
                            r.one() as i32
                        } else {
                            value as i32
                        };
                        let expected_switches = if type_value { r.array::<2>() } else { [0; 2] };
                        let after = r.snapshot();
                        let expected_grain = if type_value {
                            decode_grain(r.bytes())
                        } else {
                            state.midi.master.grain_history
                        };
                        let expected_program = if caller {
                            r.bytes::<1790>()
                        } else {
                            *program.bytes()
                        };
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
                        let common = InsertControlContext {
                            program: &program,
                            midi: MixedEffectMidiFrame {
                                midi,
                                polarity,
                                current_notes: notes,
                                master_origin: short(&anchor.midi.master, 0x38),
                                direct_switch: direct,
                                force_refresh: false,
                            },
                            secondary_switch: secondary,
                            clock_rate,
                            clock: DelayClock {
                                tempo: tempo as u16,
                                status: status as u8,
                            },
                        };
                        let context = MasterInitialMaskContext {
                            common,
                            prefix_origin: short(&anchor.midi.master, 0x34),
                            body_origin: short(&anchor.midi.master, 0x36),
                            relocation_origin: short(&anchor.midi.master, 0x3a),
                        };
                        let edit = MixedEffectParameterEdit {
                            target: if slot == 8 {
                                EffectParameterTarget::Master
                            } else {
                                EffectParameterTarget::Insert(slot as u8)
                            },
                            parameter: parameter as u8,
                            value: value as u8,
                        };
                        let saved = state;
                        let saved_phases = phases;
                        let buffers = port.buffers.clone();
                        let mut changed_program = program.clone();
                        port.reject = true;
                        port.steps.clear();
                        let type_context = MasterTypeValueContext {
                            parameters: context,
                            queue: EffectTransitionQueue::default().state(),
                            pressure_marker: sequence % 2,
                        };
                        let denied = if pair_init {initialize_insert_pair(&mut state,&mut port,&tables,&insert_initialization,&insert_programs,slot as u8,common).is_err()} else if type_value {
                            change_master_type_value(
                                &mut state,
                                &mut changed_program,
                                &mut port,
                                MasterTypeValueTables {
                                    control: &tables,
                                    properties: &property_tables,
                                    initialization: &master_initialization,
                                },
                                value as i32,
                                type_context,
                            )
                            .is_err()
                        } else if full_value {
                            change_effect_value(
                                &mut state,
                                &mut changed_program,
                                &mut port,
                                EffectValueChangeTables {
                                    control: &tables,
                                    properties: &property_tables,
                                    callers: &caller_tables,
                                },
                                EffectPropertyChange {
                                    target: edit.target,
                                    property: EffectProperty::Parameter(parameter as u8),
                                    value: value as i32,
                                },
                                context,
                            )
                            .is_err()
                        } else if master_caller {
                            change_master_parameter(
                                &mut state,
                                &mut changed_program,
                                &mut port,
                                &tables,
                                &caller_tables,
                                parameter as u8,
                                context,
                            )
                            .is_err()
                        } else if insert_caller {
                            change_insert_parameter(
                                &mut state,
                                &mut changed_program,
                                &mut port,
                                &tables,
                                &caller_tables,
                                InsertParameterChange {
                                    slot: slot as u8,
                                    parameter: parameter as u8,
                                },
                                common,
                            )
                            .is_err()
                        } else {
                            apply(&mut state, &mut port, &tables, edit, context).is_err()
                        };
                        if denied
                            && state == saved
                            && changed_program == program
                            && port.steps.is_empty()
                            && (0..3)
                                .all(|i| port.buffers.buffer_bytes(i) == buffers.buffer_bytes(i))
                        {
                            rejected += 1
                        } else {
                            errors += 1
                        }
                        port.reject = false;
                        let mut actual_value = value as i32;
                        let mut actual_switches = [0; 2];
                        if pair_init {
                            let active=initialize_insert_pair(&mut state,&mut port,&tables,&insert_initialization,&insert_programs,slot as u8,common).map_err(|_|format!("Native Insert pair initialization rejected {sequence}/{slot}/{kind}/{variation}"))?;peer_initializations+=usize::from(active);
                        } else if type_value {
                            let (v,d,p)=change_master_type_value(&mut state,&mut changed_program,&mut port,MasterTypeValueTables{control:&tables,properties:&property_tables,initialization:&master_initialization},value as i32,type_context).map_err(|_|format!("Native Master type value rejected {sequence}/{kind}/{variation}"))?;
                            actual_value = v;
                            actual_switches = [d, p];
                        } else if full_value {
                            actual_value=change_effect_value(&mut state,&mut changed_program,&mut port,EffectValueChangeTables{control:&tables,properties:&property_tables,callers:&caller_tables},EffectPropertyChange{target:edit.target,property:EffectProperty::Parameter(parameter as u8),value:value as i32},context).map_err(|_|format!("Native value setter rejected:{sequence}/{slot}/{kind}/{parameter}/{variation}"))?;
                        } else if master_caller {
                            change_master_parameter(&mut state, &mut changed_program, &mut port, &tables, &caller_tables, parameter as u8, context)
                                .map_err(|_| format!("Native Master caller rejected:{sequence}/{kind}/{parameter}/{variation}"))?;
                        } else if insert_caller {
                            change_insert_parameter(&mut state, &mut changed_program, &mut port, &tables, &caller_tables, InsertParameterChange { slot: slot as u8, parameter: parameter as u8 }, common)
                                .map_err(|_| format!("Native Insert caller rejected:{sequence}/{slot}/{kind}/{parameter}/{variation}"))?;
                        } else {
                            apply(
                        &mut state,
                        &mut port,
                        &tables,
                        edit,
                        context,
                    )
                    .map_err(|_| {
                        format!("Native mixed parameter rejected:{sequence}/{slot}/{kind}/{parameter}/{variation}")
                    })?;
                        }
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
                                    changes.push([
                                        bank as u32,
                                        i as u32,
                                        u32::from(old),
                                        u32::from(new),
                                    ]);
                                }
                            }
                        }
                        let actual = project(&state, anchor, phases, &system);
                        if master_caller {
                            caller_releases +=
                                usize::from(saved.midi.master.control.update_marker != 0);
                            let mode = caller_tables.time_parameters[usize::from(kind)][0];
                            caller_time_swaps += usize::from(
                                mode != 0
                                    && mode == parameter as u8
                                    && saved.midi.master.parameters[parameter]
                                        != saved.midi.master.previous_parameters[parameter],
                            );
                            caller_owner_changes += usize::from(
                                state.midi.master.control.owner != saved.midi.master.control.owner,
                            );
                            caller_history_changes += usize::from(
                                state.midi.master.previous_parameters[parameter]
                                    != saved.midi.master.previous_parameters[parameter],
                            );
                            caller_program_changes += program
                                .bytes()
                                .iter()
                                .zip(changed_program.bytes())
                                .filter(|(a, b)| a != b)
                                .count();
                        }
                        if insert_caller {
                            caller_releases +=
                                usize::from(saved.midi.master.control.update_marker != 0);
                            let mode = caller_tables.time_parameters[usize::from(kind)][0];
                            let old = saved.midi.inserts[slot];
                            let new = state.midi.inserts[slot];
                            caller_time_swaps += usize::from(
                                mode != 0
                                    && mode == parameter as u8
                                    && old.buffer.parameters[parameter]
                                        != old.previous_parameters[parameter],
                            );
                            caller_owner_changes += old
                                .owners
                                .iter()
                                .zip(new.owners)
                                .filter(|(a, b)| **a != *b)
                                .count();
                            caller_history_changes += usize::from(
                                new.previous_parameters[parameter]
                                    != old.previous_parameters[parameter],
                            );
                            caller_program_changes += program
                                .bytes()
                                .iter()
                                .zip(changed_program.bytes())
                                .filter(|(a, b)| a != b)
                                .count();
                        }
                        let native_unrelated_preserved = state
                            .midi
                            .inserts
                            .iter()
                            .zip(saved.midi.inserts)
                            .enumerate()
                            .all(|(s, (a, b))| {
                                a.previous_parameters
                                    .iter()
                                    .zip(b.previous_parameters)
                                    .enumerate()
                                    .all(|(p, (&x, y))| {
                                        ((insert_caller || full_value)
                                            && s == slot
                                            && p == parameter)
                                            || x == y
                                    })
                                    && a.grain_history == b.grain_history
                            })
                            && state
                                .midi
                                .master
                                .previous_parameters
                                .iter()
                                .zip(saved.midi.master.previous_parameters)
                                .enumerate()
                                .all(|(i, (&a, b))| {
                                    type_value
                                        || ((master_caller || (full_value && slot == 8))
                                            && i == parameter)
                                        || a == b
                                })
                            && state.midi.master.grain_history == expected_grain;
                        if type_value {
                            master_grain_resets +=
                                usize::from(saved.midi.master.grain_history != expected_grain);
                        }
                        let prior = project(&saved, anchor, saved_phases, &system);
                        if !native_unrelated_preserved
                            || prior != before
                            || actual != after
                            || words != original
                            || actual_lfos != expected_lfos
                            || changes != expected_changes
                            || changed_program.bytes() != &expected_program
                            || actual_value != expected_value
                            || actual_switches != expected_switches
                        {
                            errors += 1;
                            errors_by_kind[usize::from(kind)] += 1;
                            if first.is_null() {
                                first = json!({"case":calls,"input":[sequence,slot as u32,u32::from(kind),variation],"prior_matches":prior==before,"insert_differences":actual.midi.inserts.iter().flatten().zip(after.midi.inserts.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"master_matches":actual.midi.master==after.midi.master,"assignments_match":actual.midi.assignments==after.midi.assignments,"native_caches":actual.midi.caches,"original_caches":after.midi.caches,"scratch_differences":actual.scratch.iter().zip(after.scratch).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"cursor_marker":[actual.cursor,after.cursor,actual.marker,after.marker],"phase_matches":actual.phases==after.phases,"native_LFO":actual_lfos,"original_LFO":expected_lfos,"native_words":format!("{words:?}"),"original_words":format!("{original:?}"),"native_program_changes":changes.len(),"original_program_changes":expected_changes.len()});
                            }
                        }
                        if type_value && !first.is_null() && first.get("master_differences").is_none() {
                            first["master_differences"] = json!(actual.midi.master.iter().zip(after.midi.master).enumerate().filter_map(|(i,(&a,b))|(a!=b).then_some((i,a,b))).collect::<Vec<_>>());
                            first["Program_differences"] = json!(changed_program.bytes().iter().zip(expected_program).enumerate().filter_map(|(i,(&a,b))|(a!=b).then_some((i,a,b))).collect::<Vec<_>>());
                            first["return_and_switches"] = json!([actual_value as u32,expected_value as u32,actual_switches[0],expected_switches[0],actual_switches[1],expected_switches[1]]);
                            first["Grain_matches"] = json!(state.midi.master.grain_history == expected_grain);
                        }
                        changed_bytes += changes.len();
                        lfos += actual_lfos.len();
                        steps += port.steps.len();
                        maximum = maximum.max(words.len());
                        let mut queue = EffectTransitionQueue::default();
                        queue
                            .enqueue_words(&words)
                            .map_err(|_| "Native initial mask queue rejected")?;
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
                                .map_err(|_| "Native initial mask delivery failed")?;
                            if queue_state(queue.state()) != expected_state
                                || host.packets != expected
                            {
                                transport_errors += 1;
                                transport_by_kind[usize::from(kind)] += 1;
                                if first_transport.is_null() {
                                    first_transport = json!({"case":calls,"input":[sequence,slot as u32,u32::from(kind),variation],"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                                }
                                if first.is_null() {
                                    first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                                }
                            }
                            for p in host.packets {
                                if p.0 {
                                    program_words += p.3.len()
                                } else {
                                    coefficient_words += p.3.len()
                                }
                            }
                            services += 1;
                        }
                        if queue.state().rings.iter().any(|r| r.count != 0) {
                            transport_errors += 1
                        }
                        calls += 1;
                        types[usize::from(kind)] += 1;
                        slots[slot] += 1;
                    }
                }
            }
        }
    }
    let expected_per_slot: usize = if pair_init {4*31*31} else if type_value {
        0
    } else if full_value {
        tables
            .definitions
            .iter()
            .map(|d| d.parameter_count * 48)
            .sum()
    } else if master_caller {
        0
    } else if insert_caller {
        tables
            .definitions
            .iter()
            .map(|d| d.parameter_count * 48)
            .sum()
    } else if raw_mix {
        3 * 256 * 4
    } else {
        tables
            .definitions
            .iter()
            .map(|d| d.parameter_count * 12)
            .sum()
    };
    let expected_master: usize = if pair_init {0} else if type_value {
        15376
    } else if master_caller || full_value {
        tables
            .common
            .definitions
            .iter()
            .map(|d| d.parameter_count * 48)
            .sum()
    } else if insert_caller {
        0
    } else if raw_mix {
        expected_per_slot
    } else {
        tables
            .common
            .definitions
            .iter()
            .map(|d| d.parameter_count * 12)
            .sum()
    };
    let coverage_complete = if pair_init {pair_coverage.iter().flatten().all(|&n|n==32)} else if type_value {
        type_coverage.iter().flatten().all(|&n| n == 16)
    } else if full_value {
        coverage.iter().enumerate().all(|(k, ps)| {
            ps.iter().enumerate().all(|(p, vs)| {
                vs.iter().all(|&n| {
                    usize::from(n)
                        == 16
                            * ((if p < tables.definitions[k].parameter_count {
                                8
                            } else {
                                0
                            }) + usize::from(p < tables.common.definitions[k].parameter_count))
                })
            })
        })
    } else if master_caller {
        coverage.iter().enumerate().all(|(kind, parameters)| {
            parameters.iter().enumerate().all(|(parameter, values)| {
                values.iter().all(|&count| {
                    count
                        == if parameter < tables.common.definitions[kind].parameter_count {
                            16
                        } else {
                            0
                        }
                })
            })
        })
    } else if insert_caller {
        coverage.iter().enumerate().all(|(kind, parameters)| {
            parameters.iter().enumerate().all(|(p, values)| {
                values.iter().all(|&count| {
                    count
                        == if p < tables.definitions[kind].parameter_count {
                            128
                        } else {
                            0
                        }
                })
            })
        })
    } else if raw_mix {
        raw_coverage.iter().flatten().all(|&count| count == 36)
    } else {
        coverage.iter().enumerate().all(|(kind, parameters)| {
            parameters.iter().enumerate().all(|(parameter, values)| {
                let expected = 4
                    * ((if parameter < tables.definitions[kind].parameter_count {
                        8
                    } else {
                        0
                    }) + usize::from(
                        parameter < tables.common.definitions[kind].parameter_count,
                    ));
                values.iter().all(|&count| usize::from(count) == expected)
            })
        })
    };
    let passed = errors == 0
        && transport_errors == 0
        && coverage_complete
        && calls == expected_per_slot * 8 + expected_master
        && rejected == calls
        && slots[..8] == [expected_per_slot; 8]
        && slots[8] == expected_master
        && r.cursor == r.words.len();
    let mut report = json!({"passed":passed,"errors":errors,"transport_errors":transport_errors,"first_difference":first,"first_transport_difference":first_transport,
        "whole_original_mixed_parameter_calls":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,
        "type_counts":types.to_vec(),"target_counts":slots,"all_parameter_boundary_coverage_verified":!raw_mix && coverage_complete,"whole_aggregate_atomic_rejections":rejected,
        "whole_Master_parameter_caller":master_caller,"stored_program_mutations_compared":master_caller,"frontend_generic_value_setter_and_full_queue_service_verified":false,
        "whole_Insert_parameter_caller":insert_caller,"Insert_stored_program_mutations_compared":insert_caller,
        "Insert_caller_Master_assignment_releases":if insert_caller {caller_releases} else {0},"Insert_time_mode_swaps":if insert_caller {caller_time_swaps} else {0},
        "Insert_owner_changes":if insert_caller {caller_owner_changes} else {0},"Insert_stored_program_byte_changes":if insert_caller {caller_program_changes} else {0},"Insert_history_byte_changes":if insert_caller {caller_history_changes} else {0},
        "Master_assignment_releases":if master_caller {caller_releases} else {0},"Master_time_mode_swaps":if master_caller {caller_time_swaps} else {0},"Master_owner_changes":if master_caller {caller_owner_changes} else {0},
        "Master_stored_program_byte_changes":if master_caller {caller_program_changes} else {0},"Master_history_byte_changes":if master_caller {caller_history_changes} else {0},
        "raw_stored_dynamics_mix":raw_mix,"all256_dynamics_mix_bytes_on_all9_targets_verified":raw_mix && coverage_complete,
        "ordered_LFO_publications":lfos,"ordered_native_parameter_steps":steps,"changed_program_buffer_bytes_compared":changed_bytes,
        "coefficient_words_compared":coefficient_words,"program_words_compared":program_words,"maximum_live_parameter_queue_words":maximum,
        "errors_by_kind":errors_by_kind.to_vec(),"transport_errors_by_kind":transport_by_kind.to_vec(),
        "evolving_native_state_or_buffers_replayed_from_original":false,"full_mixed_MIDI_callbacks_verified":passed,"FXD03_sample_audio_verified":false});
    report["whole_high_parameter_setter"] = json!(full_value);
    report["high_parameter_return_values_compared"] = json!(full_value);
    report["type_and_header_events_in_high_setter_verified"] = json!(false);
    report["whole_high_Master_type_setter"] = json!(type_value);
    report["whole_Insert_paired_initialization_verified"]=json!(pair_init);
    report["conditional_peer_initializations"]=json!(peer_initializations);
    if pair_init {report["pair_type_coverage"]=json!(pair_coverage.iter().map(|r|r.to_vec()).collect::<Vec<_>>());}
    report["Master_Grain_history_resets_compared"] = json!(master_grain_resets);
    if type_value {
        report["transition_counts"] = json!(type_coverage.iter().map(|row| row.to_vec()).collect::<Vec<_>>());
        report["all_31_by_31_Master_type_transitions_verified"] = json!(coverage_complete);
        report["high_Master_type_return_and_switches_compared"] = json!(true);
        report["initial_queue_empty"] = json!(true);
        report["full_pressure_high_setter_verified"] = json!(false);
    }
    fs::write(
        root.join(if pair_init {"runs/native-clone/insert-paired-initialization-parity.json"} else if type_value {
            "runs/native-clone/master-type-value-parity.json"
        } else if full_value {
            "runs/native-clone/effect-value-change-parity.json"
        } else if insert_caller {
            "runs/native-clone/insert-parameter-caller-parity.json"
        } else if master_caller {
            "runs/native-clone/master-parameter-caller-parity.json"
        } else if raw_mix {
            "runs/native-clone/raw-dynamics-mix-parity.json"
        } else {
            "runs/native-clone/mixed-effect-parameter-parity.json"
        }),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native mixed parameters:{calls} calls,{services} services,{errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Initial Master mask differs".into());
    }
    Ok(())
}

struct PropertyPort {
    reject: bool,
}
impl EffectPropertyPort for PropertyPort {
    type Error = ();
    fn accept_effect_property(&mut self, _: &PreparedEffectProperty) -> Result<(), ()> {
        if self.reject { Err(()) } else { Ok(()) }
    }
}
fn snapshot_words(s: FullSnapshot) -> Vec<u32> {
    let mut words = Vec::new();
    for b in s.midi.inserts {
        words.extend(b.map(u32::from));
    }
    words.extend(s.midi.master.map(u32::from));
    words.extend(s.midi.assignments);
    words.extend(s.scratch);
    for p in s.phases {
        words.extend(p.map(u32::from));
    }
    words.extend(s.midi.caches);
    words.extend([s.cursor, s.marker]);
    words
}
fn compare_properties(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.effect_property_tables()?;
    let out = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(root.join("runs/native-clone/effect-property-original.bin.gz"))
        .output()?;
    if !out.status.success() || !out.stdout.len().is_multiple_of(4) {
        return Err("Invalid property capture".into());
    }
    let mut r = Reader {
        words: out
            .stdout
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x46505331 {
        return Err("Wrong property capture".into());
    }
    let mut buffers = core::array::from_fn(|_| Vec::new());
    for b in &mut buffers {
        let n = r.one();
        *b = (0..n).map(|_| r.one() as u8).collect();
    }
    let buffers = EffectProgramBuffers::from_buffers(library.program_buffer_layout(), buffers)?;
    let anchor = r.snapshot();
    let mut state = initialize(anchor, &buffers);
    let mut program =
        Program::from_bytes(&r.bytes::<1790>()).map_err(|_| "Bad initial property Program")?;
    let phases = anchor.phases;
    if project(&state, anchor, phases, &system) != anchor {
        return Err("Initial property state differs".into());
    }
    let values = [
        i32::MIN,
        -1000,
        -129,
        -128,
        -1,
        0,
        1,
        19,
        29,
        30,
        31,
        63,
        99,
        100,
        127,
        128,
        255,
        i32::MAX,
    ];
    let (mut calls, mut errors, mut rejected, mut runtime_changes, mut program_changes) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    let mut targets = [0usize; 9];
    let mut kinds = [0usize; 31];
    let mut rate_cases = 0usize;
    for slot in 0..9usize {
        for (kind, kind_count) in kinds.iter_mut().enumerate() {
            let headers: usize = if slot == 8 { 3 } else { 4 };
            for field in 0..headers + 20 {
                for mode in 0..4usize {
                    for (profile, &input_value) in values.iter().enumerate() {
                        let args = r.array::<9>();
                        if args[..7]
                            != [
                                0x2000,
                                calls as u32,
                                slot as u32,
                                kind as u32,
                                field as u32,
                                mode as u32,
                                profile as u32,
                            ]
                            || args[7] != input_value as u32
                        {
                            return Err("Property input changed".into());
                        }
                        let offset = if slot == 8 {
                            1038
                        } else {
                            168 + 228 * (slot / 2) + 24 * (slot % 2)
                        };
                        let p = field.saturating_sub(headers);
                        let byte = offset
                            + if field >= headers {
                                if slot == 8 { 2 + p } else { 4 + p }
                            } else if field <= 1 {
                                0
                            } else if slot == 8 {
                                1
                            } else {
                                field
                            };
                        let mut raw = *program.bytes();
                        raw[offset] = kind as u8 | 128;
                        raw[byte] = args[8] as u8;
                        if field >= headers && p != 0 {
                            raw[offset + if slot == 8 { 2 } else { 4 } + p - 1] =
                                [0, 2, 4, 5][mode];
                        }
                        program =
                            Program::from_bytes(&raw).map_err(|_| "Property Program invalid")?;
                        if slot == 8 {
                            state.midi.master.kind = kind as u8;
                        } else {
                            state.midi.inserts[slot].buffer.kind = kind as u8;
                        }
                        let original_value = r.one() as i32;
                        let n = r.one();
                        let expected_runtime: Vec<_> = (0..n).map(|_| r.array::<3>()).collect();
                        let n = r.one();
                        let expected_program: Vec<_> = (0..n).map(|_| r.array::<3>()).collect();
                        let edit = EffectPropertyChange {
                            target: if slot == 8 {
                                EffectParameterTarget::Master
                            } else {
                                EffectParameterTarget::Insert(slot as u8)
                            },
                            property: if field >= headers {
                                EffectProperty::Parameter(p as u8)
                            } else if field == 0 {
                                EffectProperty::Enabled
                            } else if field == 1 {
                                EffectProperty::Kind
                            } else {
                                EffectProperty::Owner((field - 2) as u8)
                            },
                            value: input_value,
                        };
                        let saved = state;
                        let old = program.clone();
                        let mut port = PropertyPort { reject: true };
                        if set_effect_property(&mut state, &mut program, &mut port, &tables, edit)
                            .is_err()
                            && state == saved
                            && program == old
                        {
                            rejected += 1;
                        } else {
                            errors += 1;
                        }
                        port.reject = false;
                        let value =
                            set_effect_property(&mut state, &mut program, &mut port, &tables, edit)
                                .map_err(|_| format!("Native property rejected {args:?}"))?;
                        let before = snapshot_words(project(&saved, anchor, phases, &system));
                        let after = snapshot_words(project(&state, anchor, phases, &system));
                        let actual_runtime: Vec<_> = before
                            .iter()
                            .zip(after)
                            .enumerate()
                            .filter_map(|(i, (&a, b))| (a != b).then_some([i as u32, a, b]))
                            .collect();
                        let actual_program: Vec<_> = old
                            .bytes()
                            .iter()
                            .zip(program.bytes())
                            .enumerate()
                            .filter_map(|(i, (&a, &b))| {
                                (a != b).then_some([i as u32, u32::from(a), u32::from(b)])
                            })
                            .collect();
                        if value != original_value
                            || actual_runtime != expected_runtime
                            || actual_program != expected_program
                        {
                            errors += 1;
                            if first.is_null() {
                                first = json!({"input":args,"native_value":value,"original_value":original_value,"native_runtime":actual_runtime,"original_runtime":expected_runtime,"native_program":actual_program,"original_program":expected_program});
                            }
                        }
                        if field >= headers {
                            let ix = if slot == 8 {
                                tables.master_range_indices[kind][p]
                            } else {
                                tables.insert_range_indices[kind][p]
                            };
                            rate_cases += usize::from(ix == 149);
                        }
                        runtime_changes += actual_runtime.len();
                        program_changes += actual_program.len();
                        calls += 1;
                        targets[slot] += 1;
                        *kind_count += 1;
                    }
                }
            }
        }
    }
    let passed = errors == 0
        && calls == 479880
        && rejected == calls
        && targets[..8] == [53568; 8]
        && targets[8] == 51336
        && kinds == [15480; 31]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_property_calls":calls,"errors":errors,"first_difference":first,"atomic_rejections":rejected,"runtime_byte_changes":runtime_changes,"stored_program_byte_changes":program_changes,"special_rate_cases":rate_cases,"target_counts":targets,"type_counts":kinds.to_vec(),"evolving_native_state_replayed_from_original":false,"FXD03_sample_audio_verified":false,"higher_setter_event_graph_and_type_changes_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-property-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native effect property:{calls} calls,{errors} differences");
    if !passed {
        return Err("Properties differ".into());
    }
    Ok(())
}

impl MasterTypeValuePort for Port {
    type Error = ();
    fn accept_master_type_value(&mut self, p: &PreparedMasterTypeValueChange) -> Result<(), ()> {
        self.accept_steps(p.steps())
    }
}
impl EffectValueChangePort for Port {
    type Error = ();
    fn accept_effect_value_change(
        &mut self,
        prepared: &PreparedEffectValueChange,
    ) -> Result<(), ()> {
        self.accept_master_parameter_caller(&prepared.call)
    }
}
impl EffectHeaderEventPort for Port {
    type Error = ();
    fn accept_effect_header_event(
        &mut self,
        prepared: &PreparedMasterParameterCaller,
    ) -> Result<(), ()> {
        self.accept_master_parameter_caller(prepared)
    }
}
fn compare_header_events(
    root: &std::path::Path,
    high: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.insert_control_tables()?;
    let properties = library.effect_property_tables()?;
    let callers = library.effect_parameter_caller_tables()?;
    let output = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(root.join(if high {
            "runs/native-clone/effect-header-value-original.bin.gz"
        } else {
            "runs/native-clone/effect-header-events-original.bin.gz"
        }))
        .output()?;
    if !output.status.success() || !output.stdout.len().is_multiple_of(4) {
        return Err("Incomplete original header capture".into());
    }
    let mut r = Reader {
        words: output
            .stdout
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != if high { 0x48564331 } else { 0x48454331 } {
        return Err("Wrong header event capture".into());
    }
    let (
        mut calls,
        mut services,
        mut prefills,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut steps,
        mut words_compared,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut coverage = [[[0u16; 256]; 3]; 9];
    let mut targets = [0usize; 9];
    let mut kinds = [0usize; 31];
    let mut first = Value::Null;
    for sequence in 0..12u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Header sequence changed".into());
        }
        let mut buffers = core::array::from_fn(|_| Vec::new());
        for b in &mut buffers {
            let n = r.one();
            *b = (0..n).map(|_| r.one() as u8).collect();
        }
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), buffers)?,
            steps: Vec::new(),
        };
        let anchor = r.snapshot();
        let mut state = initialize(anchor, &port.buffers);
        let phases = anchor.phases;
        if project(&state, anchor, phases, &system) != anchor {
            return Err("Header initial state changed".into());
        }
        let mut step = 0u32;
        for slot in 0..9usize {
            for event in 0..if slot == 8 { 2u32 } else { 3u32 } {
                for value in 0..256u32 {
                    let args = r.array::<9>();
                    let [_, _, _, _, _, _, kind, direct, marker] = args;
                    if args[..6] != [0x2000, sequence, step, slot as u32, event, value]
                        || kind >= 31
                    {
                        return Err("Header declared input changed".into());
                    }
                    let program = Program::from_bytes(&r.bytes::<1790>())
                        .map_err(|_| "Invalid header Program")?;
                    state.midi.master.control.update_marker = marker;
                    if slot == 8 {
                        state.midi.master.kind = kind as u8;
                    } else {
                        state.midi.inserts[slot].buffer.kind = kind as u8;
                    }
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
                    let before = r.snapshot();
                    let expected_value = if high { r.one() as i32 } else { value as i32 };
                    let after = r.snapshot();
                    let after_program = r.bytes::<1790>();
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let edit = EffectHeaderChange {
                        target: if slot == 8 {
                            EffectParameterTarget::Master
                        } else {
                            EffectParameterTarget::Insert(slot as u8)
                        },
                        event: if event == 0 {
                            EffectHeaderEvent::Enabled
                        } else {
                            EffectHeaderEvent::Owner((event - 1) as u8)
                        },
                    };
                    let saved = state;
                    let mut changed_program = program.clone();
                    let context = MasterInitialMaskContext {
                        common: InsertControlContext {
                            program: &program,
                            midi: MixedEffectMidiFrame {
                                midi: EffectMidiSources::default(),
                                polarity: EffectMidiPolarity {
                                    assignments: [0; 5],
                                },
                                current_notes: [0; 5],
                                master_origin: short(&anchor.midi.master, 0x38),
                                direct_switch: direct,
                                force_refresh: false,
                            },
                            secondary_switch: 0,
                            clock_rate: 0,
                            clock: DelayClock {
                                tempo: 0,
                                status: 0,
                            },
                        },
                        prefix_origin: 0,
                        body_origin: 0,
                        relocation_origin: 0,
                    };
                    let value_edit = EffectPropertyChange {
                        target: edit.target,
                        property: if event == 0 {
                            EffectProperty::Enabled
                        } else {
                            EffectProperty::Owner((event - 1) as u8)
                        },
                        value: value as i32,
                    };
                    let old_buffers = port.buffers.clone();
                    port.reject = true;
                    port.steps.clear();
                    let denied = if high {
                        change_effect_value(
                            &mut state,
                            &mut changed_program,
                            &mut port,
                            EffectValueChangeTables {
                                control: &tables,
                                properties: &properties,
                                callers: &callers,
                            },
                            value_edit,
                            context,
                        )
                        .is_err()
                    } else {
                        change_effect_header(&mut state, &mut port, &tables, &program, edit, direct)
                            .is_err()
                    };
                    if denied
                        && state == saved
                        && changed_program == program
                        && port.steps.is_empty()
                        && (0..3)
                            .all(|b| port.buffers.buffer_bytes(b) == old_buffers.buffer_bytes(b))
                    {
                        rejected += 1;
                    } else {
                        errors += 1;
                    }
                    port.reject = false;
                    let actual_value = if high {
                        change_effect_value(
                            &mut state,
                            &mut changed_program,
                            &mut port,
                            EffectValueChangeTables {
                                control: &tables,
                                properties: &properties,
                                callers: &callers,
                            },
                            value_edit,
                            context,
                        )
                        .map_err(|_| "Native header value rejected")?
                    } else {
                        change_effect_header(
                            &mut state, &mut port, &tables, &program, edit, direct,
                        )
                        .map_err(|_| "Native header event rejected")?;
                        value as i32
                    };
                    let actual: Vec<_> = port
                        .steps
                        .iter()
                        .flat_map(|s| s.batch.words().iter().copied())
                        .collect();
                    let no_other_output = port.steps.iter().all(|s| {
                        s.program_writes == [None; 2]
                            && s.body_program.is_none()
                            && s.batch.lfo_publication().is_none()
                    }) && (0..3)
                        .all(|b| port.buffers.buffer_bytes(b) == old_buffers.buffer_bytes(b));
                    if project(&saved, anchor, phases, &system) != before
                        || project(&state, anchor, phases, &system) != after
                        || actual != original
                        || changed_program.bytes() != &after_program
                        || actual_value != expected_value
                        || !no_other_output
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"input":args,"before_matches":project(&saved,anchor,phases,&system)==before,"after_matches":project(&state,anchor,phases,&system)==after,"words_match":actual==original,"program_match":changed_program.bytes()==&after_program,"native_program_diff":program.bytes().iter().zip(changed_program.bytes()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"original_program_diff":program.bytes().iter().zip(after_program).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"runtime_diff":snapshot_words(project(&state,anchor,phases,&system)).iter().zip(snapshot_words(after)).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>()});
                        }
                    }
                    steps += port.steps.len();
                    maximum = maximum.max(actual.len());
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(&actual)
                        .map_err(|_| "Header queue overflow")?;
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
                                    false,
                                    a as u16,
                                    c as u16,
                                    (0..n).map(|_| u64::from(r.one())).collect::<Vec<_>>(),
                                )
                            })
                            .collect();
                        let output = queue.service(tick, status);
                        let mut host = Host::default();
                        dispatch_effect_transition_batch(&mut host, &port.buffers, &output)
                            .map_err(|_| "Header delivery failed")?;
                        if queue_state(queue.state()) != expected_state || host.packets != expected
                        {
                            transport_errors += 1;
                        }
                        words_compared += host.packets.iter().map(|p| p.3.len()).sum::<usize>();
                        services += 1;
                    }
                    if queue.state().rings.iter().any(|r| r.count != 0) {
                        transport_errors += 1;
                    }
                    coverage[slot][event as usize][value as usize] += 1;
                    targets[slot] += 1;
                    kinds[kind as usize] += 1;
                    calls += 1;
                    step += 1;
                }
            }
        }
    }
    let coverage_complete = coverage.iter().enumerate().all(|(slot, events)| {
        events.iter().enumerate().all(|(event, values)| {
            values
                .iter()
                .all(|&n| n == if slot == 8 && event == 2 { 0 } else { 12 })
        })
    });
    let passed = errors == 0
        && transport_errors == 0
        && coverage_complete
        && calls == 79872
        && rejected == calls
        && r.cursor == r.words.len()
        && targets[..8] == [9216; 8]
        && targets[8] == 6144;
    let report = json!({"passed":passed,"whole_original_header_calls":calls,"whole_original_timed_services":services,"whole_original_assignment_prefills":prefills,"errors":errors,"transport_errors":transport_errors,"first_difference":first,"all256_header_bytes_and_all9_targets_verified":coverage_complete,"target_counts":targets,"type_counts":kinds.to_vec(),"atomic_rejections":rejected,"ordered_native_steps":steps,"coefficient_words_compared":words_compared,"maximum_queue_words":maximum,"evolving_native_state_replayed_from_original":false,"FXD03_sample_audio_verified":false,"full_queue_suspension_and_generic_setter_verified":false,"whole_high_header_setter_verified":high&&passed});
    fs::write(
        root.join(if high {
            "runs/native-clone/effect-header-value-parity.json"
        } else {
            "runs/native-clone/effect-header-events-parity.json"
        }),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native header events:{calls} calls,{services} services,{errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Header events differ".into());
    }
    Ok(())
}

impl InsertPairedInitializationPort for Port {
    type Error=();
    fn accept_insert_paired_initialization(&mut self,p:&PreparedInsertPairedInitialization)->Result<(),()>{self.accept_steps(p.steps())}
}
