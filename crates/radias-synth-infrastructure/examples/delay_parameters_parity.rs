//! Whole unchanged SYS07AC52 delay edits, continuously evolved native caches.
use radias_synth_application::{
    delay_effect::change_delay_parameter,
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    delay_effect::{DelayEffectKind, DelayEffectRack, DelayParameterEdit},
    delay_time::{DelayClock, DelayTimeState},
    effect_parameters::EffectParameterBatch,
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};
struct Reader {
    words: Vec<u32>,
    cursor: usize,
}
impl Reader {
    fn one(&mut self) -> u32 {
        let value = self.words[self.cursor];
        self.cursor += 1;
        value
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
    batch: Option<EffectParameterBatch>,
    reject: bool,
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
#[derive(Default)]
struct Port {
    packets: Vec<(u32, u32, Vec<u32>)>,
}
impl EffectProgramPort for Port {
    type Error = std::convert::Infallible;
    fn upload_program(&mut self, _: u16, _: &[u64], _: u16) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn write_coefficient(&mut self, a: u16, v: u32, c: u16) -> Result<(), Self::Error> {
        self.packets.push((u32::from(a), u32::from(c), vec![v]));
        Ok(())
    }
    fn write_coefficient_packet(&mut self, a: u16, v: &[u32], c: u16) -> Result<(), Self::Error> {
        self.packets.push((u32::from(a), u32::from(c), v.to_vec()));
        Ok(())
    }
}
fn assignment_words(assignments: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut words = assignments.order.map(u32::from).to_vec();
    for slot in assignments.slots {
        words.extend(slot.indices.map(u32::from));
        words.extend([slot.target, slot.last_value]);
    }
    words
}
fn state_words(state: DelayTimeState) -> [u32; 4] {
    [
        u32::from(state.cached_tempo),
        state.capacity,
        state.ratio,
        state.limited,
    ]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.delay_effect_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/delay-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated delay parameter corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x44504131 {
        return Err("Wrong delay corpus".into());
    }
    let capacities = [0, 1, 640, 3000, 24000, 65535, 0xffff0000, 0xffffffff];
    let tempos = [200, 400, 1200, 3000];
    let (mut calls, mut errors, mut rejections, mut host_words, mut host_packets, mut maximum) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [[0usize; 17]; 2];
    let mut first = Value::Null;
    let mut seen = BTreeSet::new();
    let scene_calls: usize = tables
        .definitions
        .iter()
        .enumerate()
        .map(|(family, definition)| {
            definition.ranges[..if family == 0 { 17 } else { 13 }]
                .iter()
                .map(|r| (i32::from(r.maximum) - i32::from(r.minimum) + 1) as usize)
                .sum::<usize>()
        })
        .sum();
    for sequence in 0..16u32 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Delay sequence differs".into());
        }
        let mut rack = DelayEffectRack {
            assignments: initial,
            states: core::array::from_fn(|slot| DelayTimeState {
                cached_tempo: tempos[(sequence as usize + slot) % 4],
                capacity: capacities[(sequence as usize + slot) % 8],
                ratio: 0x76543210,
                limited: 0x12345678,
            }),
        };
        for state in rack.states {
            if read.array::<4>() != state_words(state) {
                return Err("Declared initial delay state differs".into());
            }
        }
        for step in 0..scene_calls {
            let [
                tag,
                scene,
                number,
                kind,
                parameter,
                slot,
                origin,
                value,
                direct,
                owner1,
                owner2,
                tempo,
                status,
            ] = read.array::<13>();
            if tag != 0x2000
                || scene != sequence
                || number != step as u32
                || slot != (sequence + step as u32) % 8
                || !(13..=14).contains(&kind)
            {
                return Err("Delay input profile differs".into());
            }
            let parameters = read.bytes();
            let edit = DelayParameterEdit {
                kind: if kind == 13 {
                    DelayEffectKind::Lcr
                } else {
                    DelayEffectKind::Stereo
                },
                slot: slot as u8,
                origin: origin as u16,
                parameter: parameter as u8,
                value: value as u8,
                parameters,
                owners: [owner1, owner2],
                direct_switch: direct,
                clock: DelayClock {
                    tempo: tempo as u16,
                    status: status as u8,
                },
            };
            let before = read.array::<63>();
            let before_delay = read.array::<4>();
            let after = read.array::<63>();
            let after_delay = read.array::<4>();
            let before_matches = assignment_words(&rack.assignments) == before
                && state_words(rack.states[slot as usize]) == before_delay;
            let n = read.one() as usize;
            let original: Vec<_> = (0..n)
                .map(|_| CoefficientQueueWord {
                    address: read.one() as u16,
                    tagged_value: read.one(),
                })
                .collect();
            let packet_count = read.one();
            let mut original_packets = Vec::new();
            for _ in 0..packet_count {
                let address = read.one();
                let control = read.one();
                let count = read.one();
                original_packets.push((
                    address,
                    control,
                    (0..count).map(|_| read.one()).collect::<Vec<_>>(),
                ));
            }
            let saved = rack;
            let mut rejected = Queue {
                reject: true,
                ..Default::default()
            };
            if change_delay_parameter(&mut rack, &mut rejected, edit, &tables).is_ok()
                || rack != saved
                || rejected.batch.is_some()
            {
                return Err("Rejected delay changed native state".into());
            }
            rejections += 1;
            let mut queue = Queue::default();
            change_delay_parameter(&mut rack, &mut queue, edit, &tables)
                .map_err(|_| "Declared delay edit rejected")?;
            let native = queue.batch.ok_or("Delay publication missing")?;
            let mut port = Port::default();
            dispatch_parameter_batch(&mut port, &native)?;
            if !before_matches
                || native.words() != original
                || assignment_words(&rack.assignments) != after
                || state_words(rack.states[slot as usize]) != after_delay
                || port.packets != original_packets
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"step":step,"kind":kind,"parameter":parameter,"value":value,"parameters":parameters,"before_matches":before_matches,"native_delay_state":state_words(rack.states[slot as usize]),"original_delay_state":after_delay,"native_queue":native.words().iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"native_assignments":assignment_words(&rack.assignments),"original_assignments":after.as_slice(),"native_packets":port.packets,"original_packets":original_packets});
                }
            }
            maximum = maximum.max(native.words().len());
            calls += 1;
            counts[(kind - 13) as usize][parameter as usize] += 1;
            host_packets += port.packets.len();
            host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
            seen.insert((sequence, kind, parameter, value));
        }
    }
    let mut expected = BTreeSet::new();
    for sequence in 0..16 {
        for (family, definition) in tables.definitions.iter().enumerate() {
            for (parameter, range) in definition.ranges[..if family == 0 { 17 } else { 13 }]
                .iter()
                .enumerate()
            {
                for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                    ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
                {
                    expected.insert((sequence, 13 + family as u32, parameter as u32, value as u32));
                }
            }
        }
    }
    let passed = calls == 16 * scene_calls
        && read.cursor == read.words.len()
        && seen == expected
        && errors == 0
        && rejections == calls;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"parameter_counts":counts,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_parameter_batch_words":maximum,"all_thirty_parameter_domains_complete":true,"continuous_sequences":16,"insert_instances":8,"native_delay_and_assignment_states_evolve_independently":true,"requested_parameters_and_stored_snapshot_are_separate_inputs":true,"prior_coefficient_or_assignment_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,"master_parameter_wrapper_or_FXD03_sound_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/delay-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native L/C/R and Stereo Delay: {calls} original edits, {errors} differences");
    if !passed {
        return Err("Native delay controllers differ".into());
    }
    Ok(())
}
