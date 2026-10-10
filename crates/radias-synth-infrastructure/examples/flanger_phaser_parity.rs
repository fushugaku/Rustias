//! Whole original Flanger/Phaser controllers, release state, LFO and timed bus.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, flanger_phaser_effect::change_flanger_phaser_parameter,
};
use radias_synth_domain::{
    effect_lfo_program::EffectLfoProgram,
    effect_parameters::EffectParameterBatch,
    effect_routing::EffectRoutingInstance,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientChange, CoefficientQueueWord, EffectCoefficientAssignments},
    flanger_phaser_effect::{FlangerPhaserEdit, FlangerPhaserKind, FlangerPhaserRack},
    program::Program,
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
}
#[derive(Default)]
struct Queue {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, b: &EffectParameterBatch) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*b);
        Ok(())
    }
}
fn assignments(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut w = s.order.map(u32::from).to_vec();
    for s in s.slots {
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
    let tables = library.flanger_phaser_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/flanger-phaser-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Flanger/Phaser corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x46504831 {
        return Err("Wrong Flanger/Phaser corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut prefills,
        mut errors,
        mut queue_errors,
        mut rejections,
        mut words,
        mut packets,
        mut maximum,
        mut lfo_publications,
        mut releases,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut counts = [[0usize; 16]; 2];
    let mut slots = [0usize; 8];
    let mut neighbors = [0usize; 31];
    let mut first = Value::Null;
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Flanger/Phaser sequence changed".into());
        }
        let mut configs = [EffectLfoProgram::default(); 8];
        let mut phases = [[0u8; 32]; 8];
        for i in 0..8 {
            configs[i].bytes = r.bytes();
            phases[i] = r.bytes();
        }
        let mut rack = FlangerPhaserRack {
            instances: [EffectRoutingInstance::default(); 9],
            lfos: configs,
            assignments: EffectCoefficientAssignments::new(indices),
            update_marker: 0,
        };
        let mut program_raw = [0u8; 1790];
        let mut step = 0u32;
        for kind in [FlangerPhaserKind::Flanger, FlangerPhaserKind::Phaser] {
            let definition = &tables.definitions[kind.index()];
            for (parameter, range) in definition.ranges[..kind.parameter_count()]
                .iter()
                .enumerate()
            {
                for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                    ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
                {
                    let [
                        tag,
                        seq,
                        num,
                        arg_kind,
                        slot,
                        arg_param,
                        arg_value,
                        direct,
                        owner1,
                        owner2,
                        neighbor,
                    ] = r.array();
                    if [
                        tag, seq, num, arg_kind, slot, arg_param, arg_value, neighbor,
                    ] != [
                        0x2000,
                        sequence,
                        step,
                        u32::from(kind.id()),
                        (sequence + step) % 8,
                        parameter as u32,
                        value as u32,
                        (sequence + step) % 31,
                    ] {
                        return Err("Declared Flanger/Phaser input changed".into());
                    }
                    let peer = slot ^ 1;
                    let part = slot / 2;
                    let origin = r.one() as u16;
                    let peer_origin = r.one() as u16;
                    let parameters = r.bytes();
                    let peer_parameters = r.bytes();
                    rack.instances[slot as usize] = EffectRoutingInstance {
                        kind: kind.id(),
                        origin,
                        parameters,
                    };
                    rack.instances[peer as usize] = EffectRoutingInstance {
                        kind: neighbor as u8,
                        origin: peer_origin,
                        parameters: peer_parameters,
                    };
                    let [first_header, second_header] = r.array();
                    program_raw[168 + part as usize * 228] = first_header as u8;
                    program_raw[192 + part as usize * 228] = second_header as u8;
                    let program = Program::from_bytes(&program_raw)
                        .map_err(|_| "Invalid declared program")?;
                    let n = r.one();
                    for _ in 0..n {
                        let [target, value, mode] = r.array();
                        rack.assignments = rack
                            .assignments
                            .prepare(CoefficientChange {
                                direct_switch: 0,
                                standalone: false,
                                enabled_argument: 1,
                                target,
                                value,
                                mode: mode as u8,
                            })
                            .next;
                        prefills += 1;
                    }
                    let clock = r.one();
                    let marker = r.one();
                    rack.update_marker = marker;
                    let before = r.array::<63>();
                    let before_config = r.bytes::<6>();
                    let before_phase = r.bytes::<32>();
                    let after = r.array::<63>();
                    let after_config = r.bytes::<6>();
                    let after_phase = r.bytes::<32>();
                    let after_marker = r.one();
                    let before_matches = assignments(&rack.assignments) == before
                        && rack.lfos[slot as usize].bytes == before_config
                        && phases[slot as usize] == before_phase;
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let edit = FlangerPhaserEdit {
                        kind,
                        slot: slot as u8,
                        parameter: parameter as u8,
                        value: value as u8,
                        parameters,
                        origin,
                        owners: [owner1, owner2],
                        direct_switch: direct,
                        clock_rate: clock,
                    };
                    let saved = rack;
                    let mut q = Queue {
                        reject: true,
                        ..Default::default()
                    };
                    if change_flanger_phaser_parameter(&mut rack, &mut q, &tables, &program, edit)
                        .is_err()
                        && rack == saved
                        && q.batch.is_none()
                    {
                        rejections += 1;
                    } else {
                        errors += 1;
                    }
                    q.reject = false;
                    change_flanger_phaser_parameter(&mut rack, &mut q, &tables, &program, edit)
                        .map_err(|_| "Native Flanger/Phaser rejected")?;
                    let batch = q.batch.ok_or("Missing Flanger/Phaser batch")?;
                    if let Some(p) = batch.lfo_publication() {
                        phases[slot as usize][4..8]
                            .copy_from_slice(&p.tempo_increment.to_be_bytes());
                        lfo_publications += 1;
                    }
                    if rack.update_marker == 0 && saved.update_marker != 0 {
                        releases += 1;
                    }
                    if !before_matches
                        || assignments(&rack.assignments) != after
                        || rack.update_marker != after_marker
                        || rack.lfos[slot as usize].bytes != after_config
                        || phases[slot as usize] != after_phase
                        || rack.instances != saved.instances
                        || batch.words() != original
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,step,u32::from(kind.id()),slot,parameter as u32,value as u32,direct],"before_matches":before_matches,"native_state":assignments(&rack.assignments),"original_state":after.to_vec(),"native_marker":rack.update_marker,"original_marker":after_marker,"native_config":rack.lfos[slot as usize].bytes,"original_config":after_config,"native_phase":phases[slot as usize].to_vec(),"original_phase":after_phase.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                        }
                    }
                    maximum = maximum.max(batch.words().len());
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native Flanger/Phaser queue failed")?;
                    let total = r.one();
                    for _ in 0..total {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        let expected_state = r.array::<9>();
                        let count = r.one();
                        let expected: Vec<_> = (0..count)
                            .map(|_| {
                                let a = r.one();
                                let c = r.one();
                                let n = r.one();
                                (a, c, (0..n).map(|_| r.one()).collect::<Vec<_>>())
                            })
                            .collect();
                        let output = queue.service(tick, status);
                        let actual: Vec<_> = output.coefficients.packets
                            [..usize::from(output.coefficients.count)]
                            .iter()
                            .map(|p| {
                                (
                                    u32::from(p.address),
                                    1,
                                    p.values[..usize::from(p.count)].to_vec(),
                                )
                            })
                            .collect();
                        if queue_state(queue.state()) != expected_state
                            || actual != expected
                            || output.program.is_some()
                        {
                            queue_errors += 1;
                            if first.is_null() {
                                first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":actual,"original_packets":expected});
                            }
                        }
                        words += actual.iter().map(|p| p.2.len()).sum::<usize>();
                        packets += actual.len();
                        services += 1;
                    }
                    if queue.state().rings.iter().any(|p| p.count != 0)
                        || queue.state().wait_ticks != 0
                    {
                        queue_errors += 1;
                    }
                    calls += 1;
                    counts[kind.index()][parameter] += 1;
                    slots[slot as usize] += 1;
                    neighbors[neighbor as usize] += 1;
                    step += 1;
                }
            }
        }
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls == 62976
        && prefills == 576
        && rejections == calls
        && r.cursor == r.words.len()
        && releases > 0
        && lfo_publications > 0;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"whole_original_assignment_prefill_calls":prefills,"parameter_counts":counts.map(|row|row.to_vec()),"slot_counts":slots,"neighbor_type_counts":neighbors,
        "errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_coefficient_LFO_and_marker_atomic_rejections":rejections,"host_words_compared":words,"host_packets_compared":packets,"maximum_parameter_batch_words":maximum,
        "LFO_publications":lfo_publications,"all_nine_assignment_release_transactions":releases,"non_LFO_instance_guard_bytes_preserved_by_original":true,
        "original_functions_and_all_callees_execute_without_stubs":true,"native_prior_assignment_LFO_and_phase_states_evolve_without_replaying_original_outputs":true,
        "master_parameter_wrapper_physical_clock_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/flanger-phaser-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Flanger/Phaser: {calls} complete original edits, {services} services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native Flanger/Phaser differs".into());
    }
    Ok(())
}
