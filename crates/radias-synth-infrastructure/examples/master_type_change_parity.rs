//! Whole original idle-queue Master type changes; no replay of evolving native outputs.
use radias_synth_application::{
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
    master_effect_type_change::{MasterTypeChangePort, change_master_effect_type},
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_control::EffectOrigins,
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    effect_modulation::GrainModulationHistory,
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientChange, CoefficientQueueWord, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_effect_construction::{MasterEffectInstance, MasterPatch},
    master_effect_control::{MasterControlState, MasterMidiBinding},
    master_effect_type_change::{
        MasterTypeChange, MasterTypeChangeError, PreparedMasterTypeChange,
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
fn long(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}
fn short(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes(b[i..i + 2].try_into().unwrap())
}
fn put(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_be_bytes());
}
fn project(instance: &MasterEffectInstance, anchor: [u8; 152], system: &[u8]) -> [u8; 152] {
    let mut b = anchor;
    put(&mut b, 0, u32::from(instance.kind));
    let offset = 0x0ccf28 + 0x1000 + usize::from(instance.kind) * 4;
    put(&mut b, 4, long(system, offset));
    b[8..28].copy_from_slice(&instance.parameters);
    b[28..48].copy_from_slice(&instance.previous_parameters);
    put(&mut b, 0x30, instance.control.owner);
    put(&mut b, 0x44, instance.control.delay.ratio);
    put(&mut b, 0x48, instance.control.delay.limited);
    put(&mut b, 0x40, instance.control.delay.capacity);
    b[0x3c..0x3e].copy_from_slice(&instance.control.delay.cached_tempo.to_be_bytes());
    b[0x4c..0x52].copy_from_slice(&instance.control.lfo.bytes);
    put(&mut b, 0x54, instance.control.midi_binding.source);
    b[0x58] = instance.control.midi_binding.values[0] as u8;
    b[0x59] = instance.control.midi_binding.values[1] as u8;
    put(&mut b, 0x68, instance.control.pending[0]);
    put(&mut b, 0x6c, instance.control.pending[1]);
    put(&mut b, 0x70, instance.control.pending_control);
    b[0x52] = instance.controller_offset;
    put(&mut b, 0x5c, instance.control.rotary_mode);
    put(&mut b, 0x60, instance.control.rotary_speed);
    put(&mut b, 0x64, instance.enabled_argument);
    for i in 0..8 {
        b[116 + 2 * i..118 + 2 * i].copy_from_slice(&instance.grain_history.left[i].to_be_bytes());
        b[134 + 2 * i..136 + 2 * i].copy_from_slice(&instance.grain_history.right[i].to_be_bytes());
    }
    b[132] = instance.grain_history.left_read;
    b[133] = instance.grain_history.left_write;
    b[150] = instance.grain_history.right_read;
    b[151] = instance.grain_history.right_write;
    b
}
struct Port {
    reject: bool,
    buffers: EffectProgramBuffers,
    batches: Vec<EffectParameterBatch>,
    direct: u32,
    pressure: u32,
}
impl MasterTypeChangePort for Port {
    type Error = ();
    fn accept_master_type_change(&mut self, p: &PreparedMasterTypeChange) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        let mut batches = Vec::new();
        for step in p.steps() {
            for w in step.program_writes.iter().flatten() {
                next.store_program(w.selector, &[w.word]).map_err(|_| ())?;
            }
            if let Some(body) = step.body_program {
                next.store_program(
                    (body.blocks[0].tag & 127) as u8,
                    &body.words[..usize::from(body.count)],
                )
                .map_err(|_| ())?;
            }
            batches.push(step.batch);
        }
        self.buffers = next;
        self.batches = batches;
        self.direct = p.direct_switch;
        self.pressure = p.pressure_marker;
        Ok(())
    }
}
fn state(
    instance: &MasterEffectInstance,
    anchor: [u8; 152],
    system: &[u8],
    phase: &[u8; 32],
    direct: u32,
    transition: u32,
    pressure: u32,
) -> Vec<u32> {
    let mut w = project(instance, anchor, system).map(u32::from).to_vec();
    w.extend(instance.control.assignments.order.map(u32::from));
    for s in instance.control.assignments.slots {
        w.extend(s.indices.map(u32::from));
        w.extend([s.target, s.last_value]);
    }
    w.extend(instance.control.coefficient_scratch);
    w.extend(phase.map(u32::from));
    w.extend([
        instance.control.filter_cache.frequency,
        instance.control.filter_cache.dirty,
        u32::from(instance.control.work_slot),
        instance.control.update_marker,
        direct,
        transition,
        pressure,
    ]);
    w
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
    let tables = library.master_control_tables()?;
    let initial_tables = library.master_initialization_tables()?;
    let raw = fs::read(root.join("runs/native-clone/master-type-change-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master type corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d545931 {
        return Err("Wrong Master type corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut prefills,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut changed_bytes,
        mut program_words,
        mut program_packets,
        mut coefficient_words,
        mut coefficient_packets,
        mut lfos,
        mut maximum,
        mut native_steps,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut transitions = [[0usize; 31]; 31];
    let mut work_slots = [0usize; 29];
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master type sequence changed".into());
        }
        let mut buffers: [Vec<u8>; 3] = core::array::from_fn(|_| Vec::new());
        for b in &mut buffers {
            let n = r.one();
            *b = (0..n).map(|_| r.one() as u8).collect();
        }
        let initial = r.array::<327>();
        let mut anchor: [u8; 152] = initial[..152]
            .iter()
            .map(|&v| v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let mut instance = MasterEffectInstance {
            kind: anchor[3],
            parameters: anchor[8..28].try_into().unwrap(),
            previous_parameters: anchor[28..48].try_into().unwrap(),
            controller_offset: anchor[0x52],
            enabled_argument: long(&anchor, 0x64),
            grain_history: GrainModulationHistory {
                left: core::array::from_fn(|i| short(&anchor, 116 + 2 * i) as i16),
                right: core::array::from_fn(|i| short(&anchor, 134 + 2 * i) as i16),
                left_read: anchor[132],
                left_write: anchor[133],
                right_read: anchor[150],
                right_write: anchor[151],
            },
            control: MasterControlState {
                assignments: EffectCoefficientAssignments::new(
                    library.coefficient_update_indices()?,
                ),
                lfo: EffectLfoProgram {
                    bytes: anchor[0x4c..0x52].try_into().unwrap(),
                },
                delay: DelayTimeState {
                    cached_tempo: short(&anchor, 0x3c),
                    capacity: long(&anchor, 0x40),
                    ratio: long(&anchor, 0x44),
                    limited: long(&anchor, 0x48),
                },
                owner: long(&anchor, 0x30),
                pending: [long(&anchor, 0x68), long(&anchor, 0x6c)],
                pending_control: long(&anchor, 0x70),
                update_marker: initial[323],
                filter_cache: FilterEffectCache {
                    frequency: initial[320],
                    dirty: initial[321],
                },
                midi_binding: MasterMidiBinding {
                    source: long(&anchor, 0x54),
                    values: [anchor[0x58] as i8, anchor[0x59] as i8],
                },
                rotary_mode: long(&anchor, 0x5c),
                rotary_speed: long(&anchor, 0x60),
                work_slot: initial[322] as u8,
                coefficient_scratch: initial[215..288].try_into().unwrap(),
            },
        };

        let mut phase: [u8; 32] = initial[288..320]
            .iter()
            .map(|&v| v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), buffers)?,
            batches: Vec::new(),
            direct: initial[324],
            pressure: initial[326],
        };
        if state(
            &instance,
            anchor,
            &system,
            &phase,
            port.direct,
            initial[325],
            port.pressure,
        ) != initial
        {
            return Err("Declared initial Master type state differs".into());
        }
        for old in 0..31u8 {
            for kind in 0..31u8 {
                for variation in 0..4u32 {
                    let [
                        tag,
                        seq,
                        arg_old,
                        arg_kind,
                        var,
                        slot,
                        origin,
                        prefix,
                        body,
                        relocation,
                        direct,
                        clock,
                        transition,
                        update,
                        pressure,
                        tempo,
                        status,
                        note,
                    ] = r.array();
                    if [tag, seq, arg_old, arg_kind, var]
                        != [0x2000, sequence, u32::from(old), u32::from(kind), variation]
                    {
                        return Err("Master type declared input changed".into());
                    }
                    instance.kind = old;
                    instance.parameters = r.bytes();
                    instance.previous_parameters = r.bytes();
                    put(&mut anchor, 0x34, (prefix << 16) | body);
                    put(&mut anchor, 0x38, (origin << 16) | relocation);
                    let patch_bytes = r.bytes::<22>();
                    let mut patch = MasterPatch {
                        header: patch_bytes[..2].try_into().unwrap(),
                        parameters: patch_bytes[2..].try_into().unwrap(),
                    };
                    let midi = EffectMidiSources {
                        global_controls: r.array::<12>().map(|v| v as u16),
                        shared_control: r.one() as u8 as i8,
                        ..Default::default()
                    };
                    let polarity = EffectMidiPolarity {
                        assignments: r.bytes(),
                    };
                    for _ in 0..9 {
                        let [target, value, mode] = r.array();
                        instance.control.assignments = instance
                            .control
                            .assignments
                            .prepare(CoefficientChange {
                                direct_switch: 0,
                                standalone: false,
                                enabled_argument: 1,
                                mode: mode as u8,
                                target,
                                value,
                            })
                            .next;
                        prefills += 1;
                    }
                    instance.control.work_slot = slot as u8;
                    instance.control.update_marker = update;
                    port.direct = direct;
                    port.pressure = pressure;
                    let before = r.array::<327>();
                    let after = r.array::<327>();
                    let expected_patch = r.bytes::<22>();
                    let count = r.one();
                    let expected_lfos: Vec<_> =
                        (0..count).map(|_| (r.bytes::<6>(), r.one())).collect();
                    let count = r.one();
                    let changes: Vec<_> = (0..count).map(|_| r.array::<4>()).collect();
                    let count = r.one();
                    let original: Vec<_> = (0..count)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let edit = MasterTypeChange {
                        kind,
                        origins: EffectOrigins {
                            program: body as u16,
                            data: origin as u16,
                            coefficients: relocation as u16,
                        },
                        prefix_origin: prefix as u16,
                        initial_direct_switch: direct,
                        transition_marker: transition,
                        pressure_marker: pressure,
                        queue: EffectTransitionQueueState::default(),
                        clock_rate: clock,
                        clock: DelayClock {
                            tempo: tempo as u16,
                            status: status as u8,
                        },
                        current_note: note as u8,
                        midi,
                        polarity,
                    };
                    let saved = instance;
                    let saved_patch = patch;
                    let saved_phase = phase;
                    let saved_buffers = port.buffers.clone();
                    port.reject = true;
                    port.batches.clear();
                    if change_master_effect_type(
                        &mut instance,
                        &mut patch,
                        &mut port,
                        &tables,
                        &initial_tables,
                        edit,
                    )
                    .is_err()
                        && instance == saved
                        && patch == saved_patch
                        && port.batches.is_empty()
                        && (0..3)
                            .all(|i| port.buffers.buffer_bytes(i) == saved_buffers.buffer_bytes(i))
                    {
                        rejected += 1;
                    } else {
                        errors += 1;
                    }
                    port.reject = false;
                    change_master_effect_type(
                        &mut instance,
                        &mut patch,
                        &mut port,
                        &tables,
                        &initial_tables,
                        edit,
                    )
                    .map_err(|e| {
                        format!("Native type change rejected: {old}->{kind}/{variation}: {e:?}")
                    })?;
                    let mut actual_lfos = Vec::new();
                    let mut native_words = Vec::new();
                    for b in &port.batches {
                        native_words.extend_from_slice(b.words());
                        if let Some(p) = b.lfo_publication() {
                            phase[4..8].copy_from_slice(&p.tempo_increment.to_be_bytes());
                            actual_lfos.push((p.program.bytes, p.tempo_increment));
                        }
                    }
                    lfos += actual_lfos.len();
                    native_steps += port.batches.len();
                    let mut actual_changes = Vec::new();
                    for bank in 0..3 {
                        for (i, (&a, &b)) in saved_buffers
                            .buffer_bytes(bank)
                            .unwrap()
                            .iter()
                            .zip(port.buffers.buffer_bytes(bank).unwrap())
                            .enumerate()
                        {
                            if a != b {
                                actual_changes.push([
                                    bank as u32,
                                    i as u32,
                                    u32::from(a),
                                    u32::from(b),
                                ]);
                            }
                        }
                    }
                    changed_bytes += actual_changes.len();
                    let mut native_patch = Vec::from(patch.header);
                    native_patch.extend(patch.parameters);
                    let prior_matches = state(
                        &saved,
                        anchor,
                        &system,
                        &saved_phase,
                        direct,
                        transition,
                        pressure,
                    ) == before;
                    if !prior_matches
                        || state(
                            &instance,
                            anchor,
                            &system,
                            &phase,
                            port.direct,
                            transition,
                            port.pressure,
                        ) != after
                        || native_words != original
                        || actual_changes != changes
                        || actual_lfos != expected_lfos
                        || native_patch != expected_patch
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,u32::from(old),u32::from(kind),variation],"prior_matches":prior_matches,"native_state":state(&instance,anchor,&system,&phase,port.direct,transition,port.pressure),"original_state":after.to_vec(),"native_words":format!("{native_words:?}"),"original_words":format!("{original:?}"),"native_changed_bytes":actual_changes.len(),"original_changed_bytes":changes.len(),"native_lfos":actual_lfos,"original_lfos":expected_lfos,"native_patch":native_patch,"original_patch":expected_patch.to_vec()});
                        }
                    }
                    maximum = maximum.max(native_words.len());
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(&native_words)
                        .map_err(|_| "Native type queue rejected")?;
                    let count = r.one();
                    for _ in 0..count {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        let expected_state = r.array::<9>();
                        let count = r.one();
                        let mut expected = Vec::new();
                        for _ in 0..count {
                            let [program, address, control, n] = r.array();
                            let values = (0..n)
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
                            .map_err(|_| "Native type host delivery failed")?;
                        if queue_state(queue.state()) != expected_state || host.packets != expected
                        {
                            transport_errors += 1;
                            if first.is_null() {
                                first = json!({"case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
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
                    if queue.state().rings.iter().any(|r| r.count != 0) {
                        transport_errors += 1;
                    }
                    calls += 1;
                    transitions[usize::from(old)][usize::from(kind)] += 1;
                    work_slots[usize::from(instance.control.work_slot)] += 1;
                    let mut busy = edit;
                    busy.queue.rings[0].count = 1843;
                    if tables
                        .prepare_master_type_change(&instance, patch, &initial_tables, busy)
                        .err()
                        != Some(MasterTypeChangeError::RackRebuildRequired)
                    {
                        return Err("Busy rack branch silently accepted".into());
                    }
                }
            }
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 15376
        && prefills == 138384
        && rejected == calls
        && transitions == [[16; 31]; 31]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_idle_master_type_change_calls":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,"transition_counts":transitions.map(|r|r.to_vec()).to_vec(),"work_slot_counts":work_slots.to_vec(),"errors":errors,"transport_errors":transport_errors,"first_difference":first,"whole_instance_patch_program_and_queue_atomic_rejections":rejected,"changed_program_buffer_bytes_compared":changed_bytes,"program_words_compared":program_words,"program_packets_compared":program_packets,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,"ordered_LFO_publications":lfos,"native_ordered_steps":native_steps,"maximum_queue_batch_words":maximum,"native_instance_scratch_bindings_phase_or_program_outputs_replayed_from_original":false,"all_eight_inserts_inactive_during_original_callbacks":true,"busy_queue_branch_explicitly_requires_rack_reconstruction":true,"busy_rack_reconstruction_mixed_active_inserts_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/master-type-change-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native idle Master type changes: {calls} original calls, {services} services, {errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Whole idle Master type change differs".into());
    }
    Ok(())
}
