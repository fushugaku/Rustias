//! Whole mixed Insert/Master MIDI rack, independent state and timed command delivery.
use radias_synth_application::{
    effect_transition_queue::{EffectProgramSource, dispatch_effect_transition_batch},
    effects::EffectProgramPort,
    mixed_effect_midi::{MixedEffectMidiPort, update_mixed_effect_midi},
};
use radias_synth_domain::{
    delay_time::DelayTimeState,
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_modulation::GrainModulationHistory,
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{
        CoefficientChange, CoefficientQueueWord, CoefficientSlot, EffectCoefficientAssignments,
    },
    filter_effect::FilterEffectCache,
    insert_effect_construction::InsertEffectInstance,
    master_effect_construction::MasterEffectInstance,
    master_effect_control::{MasterControlState, MasterMidiBinding},
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState, PreparedMixedEffectMidi},
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
            inserts: core::array::from_fn(|_| self.bytes()),
            master: self.bytes(),
            assignments: self.array(),
            caches: self.array(),
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
fn initialize(s: Snapshot) -> MixedEffectMidiState {
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
fn project(state: &MixedEffectMidiState, anchor: Snapshot, system: &[u8]) -> Snapshot {
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
#[derive(Default)]
struct Port {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl MixedEffectMidiPort for Port {
    type Error = ();
    fn accept_mixed_effect_midi(&mut self, p: &PreparedMixedEffectMidi) -> Result<(), ()> {
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
        panic!("Mixed MIDI uploaded a program")
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
    let tables = EffectLibrary::from_system(&system)?.master_control_tables()?;
    let raw = fs::read(root.join("runs/native-clone/mixed-effect-midi-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated mixed MIDI corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d584d32 {
        return Err("Wrong mixed MIDI corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut prefills,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut host_words,
        mut host_packets,
        mut maximum,
        mut blocked_odd,
        mut binding_changes,
        mut filter_changes,
        mut ring_changes,
        mut rotary_changes,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize, 0usize, 0usize,
    );
    let mut type_counts = [[0usize; 31]; 9];
    let mut pair_counts = [[[0usize; 31]; 31]; 4];
    let mut first = Value::Null;
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Mixed MIDI sequence changed".into());
        }
        let mut anchor = r.snapshot();
        let mut state = initialize(anchor);
        if project(&state, anchor, &system) != anchor {
            return Err("Declared initial mixed MIDI state differs".into());
        }
        let mut port = Port::default();
        for case_index in 0..2241u32 {
            let step = case_index;
            let current = if case_index < 961 {
                case_index / 31
            } else {
                [4, 5, 25, 29, 30][((case_index - 961) / 256) as usize]
            };
            let peer = if case_index < 961 {
                case_index % 31
            } else {
                current
            };
            let [tag, seq, arg_step, c, p, direct] = r.array();
            if [tag, seq, arg_step, c, p, direct]
                != [
                    0x2000,
                    sequence,
                    step,
                    current,
                    peer,
                    [0, 1, 0x10000, 0x80000000][sequence as usize],
                ]
            {
                return Err("Mixed MIDI input changed".into());
            }
            for i in &mut state.inserts {
                i.buffer.kind = r.one() as u8;
                i.buffer.parameters = r.bytes();
                i.buffer.origin = r.one() as u16;
                i.owners = r.array();
                i.controller_source = r.one();
            }
            state.master.kind = r.one() as u8;
            state.master.parameters = r.bytes();
            let master_origin = r.one() as u16;
            state.master.control.owner = r.one();
            state.master.control.midi_binding.source = r.one();
            anchor.master[0x38..0x3a].copy_from_slice(&master_origin.to_be_bytes());
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
            let note = r.one() as u8;
            notes[4] = note;
            let n = r.one();
            for i in 0..n {
                let [target, value] = r.array();
                let p = state.master.control.assignments.prepare(CoefficientChange {
                    direct_switch: direct,
                    standalone: false,
                    enabled_argument: 1,
                    mode: (i % 2) as u8,
                    target,
                    value,
                });
                state.master.control.assignments = p.next;
                prefills += 1;
            }
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
            port.reject = true;
            port.batch = None;
            let frame = MixedEffectMidiFrame {
                midi,
                polarity,
                current_notes: notes,
                master_origin,
                direct_switch: direct,
                force_refresh: false,
            };
            if update_mixed_effect_midi(&mut state, &mut port, &tables, frame).is_err()
                && state == saved
                && port.batch.is_none()
            {
                rejected += 1
            } else {
                errors += 1
            }
            port.reject = false;
            update_mixed_effect_midi(&mut state, &mut port, &tables, frame)
                .map_err(|_| format!("Native mixed MIDI rejected:{sequence}/{current}/{peer}"))?;
            let batch = port.batch.take().ok_or("Missing mixed MIDI batch")?;
            let actual = project(&state, anchor, &system);
            let prior = project(&saved, anchor, &system);
            let mut allowed = saved;
            for (a, b) in allowed.inserts.iter_mut().zip(state.inserts) {
                a.controller_values = b.controller_values;
                a.rotary_mode = b.rotary_mode;
                a.rotary_speed = b.rotary_speed;
            }
            allowed.insert_filter_caches = state.insert_filter_caches;
            allowed.master.control.assignments = state.master.control.assignments;
            allowed.master.control.midi_binding.values = state.master.control.midi_binding.values;
            allowed.master.control.rotary_mode = state.master.control.rotary_mode;
            allowed.master.control.rotary_speed = state.master.control.rotary_speed;
            allowed.master.control.filter_cache = state.master.control.filter_cache;
            if prior != before || actual != after || batch.words() != original || state != allowed {
                errors += 1;
                if first.is_null() {
                    first = json!({"case":calls,"input":[sequence,current,peer],"prior_matches":prior==before,"native_unrelated_preserved":state==allowed,"insert_differences":actual.inserts.iter().flatten().zip(after.inserts.iter().flatten()).enumerate().filter_map(|(i,(a,b))|(a!=b).then_some((i,*a,*b))).collect::<Vec<_>>(),"master_differences":actual.master.iter().zip(after.master).enumerate().filter_map(|(i,(a,b))|(*a!=b).then_some((i,*a,b))).collect::<Vec<_>>(),"native_assignments":actual.assignments.to_vec(),"original_assignments":after.assignments.to_vec(),"native_caches":actual.caches,"original_caches":after.caches,"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                }
            }
            for (slot, i) in state.inserts.iter().enumerate() {
                type_counts[slot][usize::from(i.buffer.kind)] += 1;
                binding_changes += i
                    .controller_values
                    .iter()
                    .zip(saved.inserts[slot].controller_values)
                    .filter(|(a, b)| **a != *b)
                    .count();
                if i.buffer.kind == 25
                    && i.controller_values[0] != saved.inserts[slot].controller_values[0]
                {
                    ring_changes += 1;
                }
                if i.rotary_mode != saved.inserts[slot].rotary_mode
                    || i.rotary_speed != saved.inserts[slot].rotary_speed
                {
                    rotary_changes += 1;
                }
                if slot % 2 == 1 && state.inserts[slot - 1].buffer.kind >= 29 {
                    blocked_odd += 1;
                }
            }
            type_counts[8][usize::from(state.master.kind)] += 1;
            binding_changes += state
                .master
                .control
                .midi_binding
                .values
                .iter()
                .zip(saved.master.control.midi_binding.values)
                .filter(|(a, b)| **a != *b)
                .count();
            if state.master.kind == 25
                && state.master.control.midi_binding.values[0]
                    != saved.master.control.midi_binding.values[0]
            {
                ring_changes += 1;
            }
            if state.master.control.rotary_mode != saved.master.control.rotary_mode
                || state.master.control.rotary_speed != saved.master.control.rotary_speed
            {
                rotary_changes += 1;
            }
            filter_changes += state
                .insert_filter_caches
                .iter()
                .zip(saved.insert_filter_caches)
                .filter(|(a, b)| **a != *b)
                .count()
                + usize::from(
                    state.master.control.filter_cache != saved.master.control.filter_cache,
                );
            for (part, pair) in state.inserts.chunks_exact(2).enumerate() {
                pair_counts[part][usize::from(pair[0].buffer.kind)]
                    [usize::from(pair[1].buffer.kind)] += 1;
            }
            maximum = maximum.max(batch.words().len());
            let mut queue = EffectTransitionQueue::default();
            queue
                .enqueue_words(batch.words())
                .map_err(|_| "Native mixed MIDI queue rejected")?;
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
                    .map_err(|_| "Native mixed MIDI host failed")?;
                if queue_state(queue.state()) != expected_state || host.packets != expected {
                    transport_errors += 1;
                    if first.is_null() {
                        first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                    }
                }
                host_words += host.packets.iter().map(|p| p.2.len()).sum::<usize>();
                host_packets += host.packets.len();
                services += 1;
            }
            if queue.state().rings.iter().any(|r| r.count != 0) {
                transport_errors += 1
            }
            calls += 1;
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 8964
        && rejected == calls
        && type_counts.iter().all(|row| {
            row.iter().enumerate().all(|(kind, &count)| {
                count
                    == if [4, 5, 25, 29, 30].contains(&kind) {
                        1148
                    } else {
                        124
                    }
            })
        })
        && pair_counts.iter().all(|pair| {
            pair.iter().enumerate().all(|(a, row)| {
                row.iter().enumerate().all(|(b, &count)| {
                    count
                        == if a == b && [4, 5, 25, 29, 30].contains(&a) {
                            1028
                        } else {
                            4
                        }
                })
            })
        })
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_mixed_Insert_Master_MIDI_sweeps":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,"all_nine_slot_type_counts":type_counts.iter().map(|r|r.to_vec()).collect::<Vec<_>>(),"all_four_pair_type_counts":pair_counts.iter().map(|p|p.iter().map(|r|r.to_vec()).collect::<Vec<_>>()).collect::<Vec<_>>(),"errors":errors,"transport_errors":transport_errors,"first_difference":first,"whole_rack_and_queue_atomic_rejections":rejected,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_mixed_MIDI_batch_words":maximum,"blocked_odd_slot_cases":blocked_odd,"controller_value_changes":binding_changes,"filter_cache_changes":filter_changes,"Ring_current_note_changes":ring_changes,"Rotary_mode_speed_changes":rotary_changes,"all_eight_Insert_and_Master_raw_objects_shared_assignments_and_nine_filter_caches_compared":true,"native_evolving_rack_cache_or_assignment_outputs_replayed_from_original":false,"original_all_phase_Grain_history_and_coefficient_scratch_guards_preserved":true,"global_notes_preserve_raw_byte_while_timbre_notes_mask_bit7":true,"all256_global_and_raw_timbre_note_bytes_with_all_nine_special_slots_active":true,"directed_special_slot_matrix_calls":5120,"initial_mask_busy_rack_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/mixed-effect-midi-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native mixed Insert/Master MIDI:{calls} calls,{services} services,{errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Mixed effect MIDI differs".into());
    }
    Ok(())
}
