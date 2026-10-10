//! Whole original producer calls, including every nested full-ring service.
use radias_synth_application::{
    effect_transition_queue::{
        EffectPublicationError, EffectQueueServiceInputs, publish_effect_transition_words,
        service_effect_transition_queue_with_context,
    },
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    effect_transition_queue::{
        EffectQueuePublication, EffectRingState, EffectTransitionQueue, EffectTransitionQueueState,
    },
    effect_updates::CoefficientQueueWord,
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
    fn word(&mut self) -> CoefficientQueueWord {
        CoefficientQueueWord {
            address: self.one() as u16,
            tagged_value: self.one(),
        }
    }
}
fn state(words: [u32; 9]) -> EffectTransitionQueueState {
    EffectTransitionQueueState {
        rings: core::array::from_fn(|i| EffectRingState {
            write_index: words[3 * i] as u16,
            read_index: words[3 * i + 1] as u16,
            count: words[3 * i + 2] as u16,
        }),
        control: words[6] as u8,
        wait_ticks: words[7] as u16,
        wait_started: words[8] as u16,
    }
}
fn state_words(s: EffectTransitionQueueState) -> [u32; 9] {
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
type Packet = (bool, u16, u16, Vec<u64>);
#[derive(Default)]
struct Port {
    packets: Vec<Packet>,
}
impl EffectProgramPort for Port {
    type Error = Infallible;
    fn upload_program(&mut self, a: u16, w: &[u64], c: u16) -> Result<(), Self::Error> {
        self.packets.push((true, a, c, w.to_vec()));
        Ok(())
    }
    fn write_coefficient(&mut self, a: u16, w: u32, c: u16) -> Result<(), Self::Error> {
        self.packets.push((false, a, c, vec![u64::from(w)]));
        Ok(())
    }
    fn write_coefficient_packet(&mut self, a: u16, w: &[u32], c: u16) -> Result<(), Self::Error> {
        self.packets
            .push((false, a, c, w.iter().map(|v| u64::from(*v)).collect()));
        Ok(())
    }
}
struct Inputs {
    values: Vec<(u16, u16)>,
    cursor: usize,
}
impl EffectQueueServiceInputs for Inputs {
    fn next_service_inputs(&mut self) -> Option<(u16, u16)> {
        let value = *self.values.get(self.cursor)?;
        self.cursor += 1;
        Some(value)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let raw = fs::read(root.join("runs/native-clone/effect-queue-pressure-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated queue-pressure corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x51505231 {
        return Err("Wrong queue-pressure corpus".into());
    }
    let banks = core::array::from_fn(|_| {
        let n = r.one();
        (0..n).map(|_| r.one() as u8).collect::<Vec<_>>()
    });
    let buffers = EffectProgramBuffers::from_buffers(library.program_buffer_layout(), banks)?;
    let initial: [[CoefficientQueueWord; 2048]; 2] =
        core::array::from_fn(|_| core::array::from_fn(|_| r.word()));
    let (
        mut scenes,
        mut producers,
        mut services,
        mut changed,
        mut program_packets,
        mut suspended,
        mut errors,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    let mut flags_seen = [0usize; 256];
    let mut wraps = [0usize; 3];
    let mut counts = [0usize; 4];
    for flags in 0..256u32 {
        for count in 2043..=2046u32 {
            for wrap in 0..3u32 {
                if r.array::<5>() != [0x1000, scenes as u32, flags, count, wrap] {
                    return Err("Pressure scene order changed".into());
                }
                let before = state(r.array());
                let mut words = initial;
                let n = r.one();
                for _ in 0..n {
                    let [ring, index, address, value] = r.array();
                    words[ring as usize][index as usize] = CoefficientQueueWord {
                        address: address as u16,
                        tagged_value: value,
                    };
                }
                let mut queue = EffectTransitionQueue::from_state(before, words)
                    .map_err(|_| "Invalid declared pressure state")?;
                let mut application_queue = EffectTransitionQueue::from_state(before, words)
                    .map_err(|_| "Invalid declared application state")?;
                let mut whole_queue = EffectTransitionQueue::from_state(before, words)
                    .map_err(|_| "Invalid declared complete-publication state")?;
                let n = r.one();
                let publication: Vec<_> = (0..n).map(|_| r.word()).collect();
                let mut whole_inputs = Vec::new();
                let mut whole_packets = Vec::new();
                let mut source_return = false;
                for (word_index, word) in publication.iter().enumerate() {
                    let data = [*word];
                    let mut pending =
                        EffectQueuePublication::with_host_control(&data, word.address);
                    let mut app_pending =
                        EffectQueuePublication::with_host_control(&data, word.address);
                    let n = r.one();
                    let mut observed = Vec::new();
                    let mut app_port = Port::default();
                    // A missing external input suspends the already prepared producer
                    // call. Resume uses this same cursor, never prepares its ring again.
                    let mut no_inputs = Inputs {
                        values: Vec::new(),
                        cursor: 0,
                    };
                    let initial_result = publish_effect_transition_words(
                        &mut application_queue,
                        &mut app_pending,
                        &mut app_port,
                        &buffers,
                        &mut no_inputs,
                    );
                    if (n == 0 && !initial_result.is_ok())
                        || (n != 0
                            && initial_result != Err(EffectPublicationError::ServiceInputRequired))
                    {
                        return Err("Application pressure suspension differs".into());
                    }
                    suspended += usize::from(n != 0);
                    for service_index in 0..n {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        whole_inputs.push((tick, status));
                        let expected_state = r.array::<9>();
                        let packet_count = r.one();
                        let expected: Vec<_> = (0..packet_count)
                            .map(|_| {
                                let program = r.one() != 0;
                                let address = r.one() as u16;
                                let control = r.one() as u16;
                                let count = r.one();
                                let values = (0..count)
                                    .map(|_| {
                                        let [lo, hi] = r.array();
                                        u64::from(lo) | (u64::from(hi) << 32)
                                    })
                                    .collect();
                                (program, address, control, values)
                            })
                            .collect();
                        if pending.publish_available(&mut queue) {
                            return Err("Native producer returned before source service".into());
                        }
                        let mut port = Port::default();
                        service_effect_transition_queue_with_context(
                            &mut queue,
                            &mut port,
                            &buffers,
                            tick,
                            status,
                            pending.host_context(),
                        )
                        .map_err(|_| "Native pressure host delivery failed")?;
                        if state_words(queue.state()) != expected_state || port.packets != expected
                        {
                            errors += 1;
                            if first.is_null() {
                                first = json!({"scene":scenes,"word":word_index,"service":service_index,"native_state":state_words(queue.state()),"original_state":expected_state,"native_packets":port.packets,"original_packets":expected});
                            }
                        }
                        let mut one_input = Inputs {
                            values: vec![(tick, status)],
                            cursor: 0,
                        };
                        let result = publish_effect_transition_words(
                            &mut application_queue,
                            &mut app_pending,
                            &mut app_port,
                            &buffers,
                            &mut one_input,
                        );
                        if one_input.cursor != 1
                            || (service_index + 1 < n
                                && result != Err(EffectPublicationError::ServiceInputRequired))
                            || (service_index + 1 == n && !result.is_ok())
                        {
                            return Err("Resumable application producer differs".into());
                        }
                        program_packets += expected.iter().filter(|p| p.0).count();
                        observed.extend(expected);
                        services += 1;
                    }
                    let result = r.one() != 0;
                    source_return = result;
                    let after = r.array::<9>();
                    if !pending.publish_available(&mut queue)
                        || pending.pending_switch() != result
                        || app_pending.pending_switch() != result
                        || state_words(queue.state()) != after
                        || state_words(application_queue.state()) != after
                        || app_port.packets != observed
                        || pending.published_words() != 1
                        || app_pending.published_words() != 1
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"scene":scenes,"word":word_index,"native_state":state_words(queue.state()),"application_state":state_words(application_queue.state()),"original_state":after,"native_return":pending.pending_switch(),"original_return":result});
                        }
                    }
                    producers += 1;
                    whole_packets.extend(observed);
                }
                let mut whole_pending = EffectQueuePublication::new(&publication);
                let mut whole_port = Port::default();
                let mut inputs = Inputs {
                    values: whole_inputs,
                    cursor: 0,
                };
                let whole_result = publish_effect_transition_words(
                    &mut whole_queue,
                    &mut whole_pending,
                    &mut whole_port,
                    &buffers,
                    &mut inputs,
                );
                if whole_result != Ok(source_return)
                    || whole_queue.state() != queue.state()
                    || whole_pending.published_words() != publication.len()
                    || whole_port.packets != whole_packets
                    || inputs.cursor != inputs.values.len()
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"scene":scenes,"whole_publication_state":state_words(whole_queue.state()),"original_final_state":state_words(queue.state()),"whole_publication_packets":whole_port.packets,"original_packets":whole_packets});
                    }
                }
                let n = r.one();
                let expected: Vec<[u32; 4]> = (0..n).map(|_| r.array()).collect();
                for q in [&queue, &application_queue, &whole_queue] {
                    let mut actual = Vec::new();
                    for (ring, original) in words.iter().enumerate() {
                        for (index, (&old, &new)) in
                            original.iter().zip(q.ring_words(ring).unwrap()).enumerate()
                        {
                            if old != new {
                                actual.push([
                                    ring as u32,
                                    index as u32,
                                    u32::from(new.address),
                                    new.tagged_value,
                                ]);
                            }
                        }
                    }
                    if actual != expected {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"scene":scenes,"native_ring_changes":actual,"original_ring_changes":expected});
                        }
                    }
                }
                changed += expected.len();
                scenes += 1;
                flags_seen[flags as usize] += 1;
                wraps[wrap as usize] += 1;
                counts[(count - 2043) as usize] += 1;
            }
        }
    }
    let totals = r.array::<5>();
    let passed = errors == 0
        && totals
            == [
                scenes as u32,
                producers as u32,
                services as u32,
                changed as u32,
                program_packets as u32,
            ]
        && scenes == 3072
        && flags_seen == [12; 256]
        && wraps == [1024; 3]
        && counts == [768; 4]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"errors":errors,"first_difference":first,"pressure_scenes":scenes,"whole_original_producer_calls":producers,"nested_original_synchronous_services":services,"changed_ring_entries_compared":changed,"original_program_packets_compared":program_packets,"resumable_missing_input_suspensions":suspended,"all256_control_bytes_verified":flags_seen.to_vec(),"initial_capacity_counts":counts,"ring_wrap_scenes":wraps,"pinned_ring_retained_across_service":passed,"complete_application_publication_scenes_verified":if passed { scenes } else { 0 },"scalar_R4_commit_packed_commit_and_program_R4_clear_verified":passed,"source_current_state_or_ring_outputs_replayed_as_native_inputs":false,"FXD03_sample_audio_verified":false,"physical_interrupt_or_host_wait_timing_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-queue-pressure-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native queue pressure:{scenes} scenes,{producers} producer calls,{services} nested services,{errors} differences"
    );
    if !passed {
        return Err("Queue-pressure producer differs".into());
    }
    Ok(())
}
