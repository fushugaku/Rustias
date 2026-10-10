//! Complete original Chorus insert dispatch and continuous assignment state.
use radias_synth_application::{
    chorus_effect::change_chorus_parameter, effect_parameters::EffectParameterQueue,
};
use radias_synth_domain::{
    chorus_effect::{ChorusEffectRack, ChorusParameterEdit},
    delay_time::{DelayClock, DelayTimeState},
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
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
fn times(r: &mut Reader) -> [DelayTimeState; 8] {
    core::array::from_fn(|_| {
        let [cached, capacity, ratio, limited] = r.array();
        DelayTimeState {
            cached_tempo: cached as u16,
            capacity,
            ratio,
            limited,
        }
    })
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
    let tables = library.chorus_effect_tables()?;
    let indices = library.coefficient_update_indices()?;
    let equalizer = library.equalizer_tables()?;
    let raw = fs::read(root.join("runs/native-clone/chorus-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Chorus corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x43485231 {
        return Err("Wrong Chorus corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut queue_errors,
        mut rejections,
        mut words,
        mut packets,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut counts = [0usize; 9];
    let mut slots = [0usize; 8];
    let mut first = Value::Null;
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Chorus sequence changed".into());
        }
        let mut state = ChorusEffectRack {
            times: times(&mut r),
            assignments: EffectCoefficientAssignments::new(indices),
        };
        let mut step = 0u32;
        for (parameter, range) in tables.ranges.iter().enumerate() {
            for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
            {
                let [
                    tag,
                    seq,
                    num,
                    slot,
                    arg_param,
                    arg_value,
                    direct,
                    owner1,
                    owner2,
                    origin,
                    tempo,
                    clock_status,
                ] = r.array();
                if [tag, seq, num, slot, arg_param, arg_value]
                    != [
                        0x2000,
                        sequence,
                        step,
                        (sequence + step) % 8,
                        parameter as u32,
                        value as u32,
                    ]
                {
                    return Err("Declared Chorus input changed".into());
                }
                let parameters = r.array::<20>().map(|v| v as u8);
                let before = r.array::<63>();
                let before_times = times(&mut r);
                let after = r.array::<63>();
                let after_times = times(&mut r);
                let n = r.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                let edit = ChorusParameterEdit {
                    slot: slot as u8,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters,
                    origin: origin as u16,
                    owners: [owner1, owner2],
                    direct_switch: direct,
                    clock: DelayClock {
                        tempo: tempo as u16,
                        status: clock_status as u8,
                    },
                };
                let saved = state;
                let before_matches =
                    assignments(&state.assignments) == before && state.times == before_times;
                let mut q = Queue {
                    reject: true,
                    ..Default::default()
                };
                if change_chorus_parameter(&mut state, &mut q, &tables, &equalizer, edit).is_err()
                    && state == saved
                    && q.batch.is_none()
                {
                    rejections += 1;
                } else {
                    errors += 1;
                }
                q.reject = false;
                change_chorus_parameter(&mut state, &mut q, &tables, &equalizer, edit)
                    .map_err(|_| "Native Chorus rejected")?;
                let batch = q.batch.ok_or("Missing Chorus batch")?;
                if !before_matches
                    || assignments(&state.assignments) != after
                    || state.times != after_times
                    || batch.words() != original
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"case":calls,"input":[sequence,step,slot,parameter as u32,value as u32,direct],"before_matches":before_matches,"native_state":assignments(&state.assignments),"original_state":after.to_vec(),"native_times":format!("{:?}",state.times),"original_times":format!("{after_times:?}"),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                    }
                }
                maximum = maximum.max(batch.words().len());
                let mut queue = EffectTransitionQueue::default();
                queue
                    .enqueue_words(batch.words())
                    .map_err(|_| "Native Chorus queue failed")?;
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
                if queue.state().rings.iter().any(|p| p.count != 0) {
                    queue_errors += 1;
                }
                calls += 1;
                counts[parameter] += 1;
                slots[slot as usize] += 1;
                step += 1;
            }
        }
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls == 28288
        && rejections == calls
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts,"slot_counts":slots,
        "errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":words,"host_packets_compared":packets,"maximum_parameter_batch_words":maximum,
        "all_original_non_time_instance_guard_bytes_preserved":true,"all_eight_delay_states_compared_before_and_after_every_edit":true,
        "original_functions_and_all_callees_execute_without_stubs":true,"delay_and_EQ_reread_stored_snapshot_independently_of_argument":true,
        "native_prior_assignment_states_evolve_without_replaying_original_outputs":true,"master_parameter_wrapper_physical_clock_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/chorus-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Chorus: {calls} complete original edits, {services} queue services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native Chorus differs".into());
    }
    Ok(())
}
