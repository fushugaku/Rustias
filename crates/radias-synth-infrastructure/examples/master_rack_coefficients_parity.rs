//! Whole SYS07BCFC Master coefficient-only loader, independent occupied allocator and scratch.
use radias_synth_application::{
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
    master_rack_coefficients::{MasterRackCoefficientsPort, load_master_rack_coefficients},
};
use radias_synth_domain::{
    delay_time::DelayTimeState,
    effect_lfo_program::EffectLfoProgram,
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientChange, CoefficientQueueWord, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_effect_control::{MasterControlState, MasterMidiBinding},
    master_rack_coefficients::{MasterRackCoefficientLoad, PreparedMasterRackCoefficients},
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
}
impl MasterRackCoefficientsPort for Port {
    type Error = ();
    fn accept_master_rack_coefficients(
        &mut self,
        p: &PreparedMasterRackCoefficients,
    ) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
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
    w.push(rack.delay.capacity);
    w.extend(rack.lfo.bytes.map(u32::from));
    w.extend(phase.map(u32::from));
    w.extend(rack.coefficient_scratch);
    w.extend([u32::from(rack.work_slot), rack.update_marker]);
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
    let initial_tables = library.master_initialization_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/master-rack-coefficients-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master initial corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d524331 {
        return Err("Wrong Master initial corpus".into());
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
        mut released,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut type_counts = [0usize; 31];
    let mut work_slot_counts = [0usize; 29];
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master initial sequence changed".into());
        }
        let mut buffers: [Vec<u8>; 3] = core::array::from_fn(|_| Vec::new());
        for b in &mut buffers {
            let n = r.one();
            *b = (0..n).map(|_| r.one() as u8).collect();
        }
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), buffers)?,
            batch: None,
        };
        let initial = r.array::<177>();
        let mut phase: [u8; 32] = initial[70..102]
            .iter()
            .map(|&v| v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let mut rack = MasterControlState {
            assignments: EffectCoefficientAssignments::new(indices),
            lfo: EffectLfoProgram {
                bytes: initial[64..70]
                    .iter()
                    .map(|&v| v as u8)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap(),
            },
            delay: DelayTimeState {
                capacity: initial[63],
                ..Default::default()
            },
            pending: [0; 2],
            pending_control: 0,
            owner: 0,
            update_marker: initial[176],
            filter_cache: FilterEffectCache::default(),
            midi_binding: MasterMidiBinding {
                source: 0,
                values: [0; 2],
            },
            rotary_mode: 0,
            rotary_speed: 0,
            work_slot: initial[175] as u8,
            coefficient_scratch: initial[102..175].try_into().unwrap(),
        };
        if state(&rack, &phase) != initial {
            return Err("Declared initial Master upload state differs".into());
        }
        for kind in 0..31u8 {
            for slot_index in 0..21u32 {
                for variation in 0..6u32 {
                    let [
                        tag,
                        seq,
                        arg_kind,
                        slot,
                        variant,
                        direct,
                        clock,
                        origin,
                        _body,
                        _relocation,
                    ] = r.array();
                    if [tag, seq, arg_kind, slot, variant]
                        != [0x2000, sequence, u32::from(kind), slot_index, variation]
                    {
                        return Err("Master initial declared input changed".into());
                    }
                    let parameters = r.bytes();
                    for _ in 0..9 {
                        let [target, value, mode] = r.array();
                        let p = rack.assignments.prepare(CoefficientChange {
                            direct_switch: 0,
                            standalone: false,
                            enabled_argument: 1,
                            mode: mode as u8,
                            target,
                            value,
                        });
                        rack.assignments = p.next;
                        prefills += 1;
                    }
                    rack.work_slot = if slot_index == 20 {
                        28
                    } else {
                        slot_index as u8
                    };
                    let before = r.array::<177>();
                    let after = r.array::<177>();
                    let n = r.one();
                    let changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let edit = MasterRackCoefficientLoad {
                        kind,
                        parameters,
                        origin: origin as u16,
                        clock_rate: clock,
                    };
                    let saved = rack;
                    let saved_phase = phase;
                    let saved_buffers = port.buffers.clone();
                    port.reject = true;
                    port.batch = None;
                    if load_master_rack_coefficients(
                        &mut rack,
                        &mut port,
                        &tables,
                        &initial_tables,
                        edit,
                    )
                    .is_err()
                        && rack == saved
                        && port.batch.is_none()
                        && (0..3)
                            .all(|i| port.buffers.buffer_bytes(i) == saved_buffers.buffer_bytes(i))
                    {
                        rejected += 1;
                    } else {
                        errors += 1;
                    }
                    port.reject = false;
                    load_master_rack_coefficients(
                        &mut rack,
                        &mut port,
                        &tables,
                        &initial_tables,
                        edit,
                    )
                    .map_err(|_| {
                        format!(
                            "Native Master initial upload rejected: {kind}/{slot_index}/{variation}"
                        )
                    })?;
                    let batch = port.batch.take().ok_or("Missing Master initial batch")?;
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
                    if rack.assignments != saved.assignments
                        || state(&saved, &saved_phase) != before
                        || state(&rack, &phase) != after
                        || batch.words() != original
                        || actual_changes != changes
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,u32::from(kind),slot_index,variation,direct],"prior_matches":state(&saved,&saved_phase)==before,"native_state":state(&rack,&phase),"original_state":after.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}"),"native_changed_bytes":actual_changes.len(),"original_changed_bytes":changes.len()});
                        }
                    }
                    maximum = maximum.max(batch.words().len());
                    released += saved
                        .assignments
                        .slots
                        .iter()
                        .filter(|s| s.target != 0x2f7)
                        .count();
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native initial queue rejected")?;
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
                            .map_err(|_| "Native initial host delivery failed")?;
                        if queue_state(queue.state()) != expected_state || host.packets != expected
                        {
                            transport_errors += 1;
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
                    if queue.state().rings.iter().any(|r| r.count != 0) {
                        transport_errors += 1;
                    }
                    calls += 1;
                    type_counts[usize::from(kind)] += 1;
                    work_slot_counts[usize::from(rack.work_slot)] += 1;
                }
            }
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 15624
        && prefills == 140616
        && rejected == calls
        && changed_bytes == 0
        && program_words == 0
        && program_packets == 0
        && type_counts == [504; 31]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_master_rack_coefficient_calls":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefills":prefills,"type_counts":type_counts.to_vec(),"work_slot_counts":work_slot_counts.to_vec(),"errors":errors,"transport_errors":transport_errors,"first_difference":first,"state_program_and_queue_atomic_rejections":rejected,"changed_program_buffer_bytes_compared":changed_bytes,"program_words_compared":program_words,"program_packets_compared":program_packets,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,"LFO_publications":lfos,"maximum_initial_batch_words":maximum,"assignment_records_retained":released,"evolving_native_state_or_program_buffers_replayed_from_original":false,"original_unrelated_Master_instance_bytes_preserved":true,"original_and_native_program_buffers_unchanged":true,"occupied_allocator_preserved_without_release":true,"no_program_routing_or_wait_commands_added":true,"complete_type_change_wrapper_busy_rack_reconstruction_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/master-rack-coefficients-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Master rack coefficients: {calls} original calls, {services} services, {errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Master initial upload differs".into());
    }
    Ok(())
}
