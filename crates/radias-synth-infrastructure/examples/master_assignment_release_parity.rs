//! Whole original SYS075316 release with intermediate pool/queue observations.
use radias_synth_application::{
    effect_transition_queue::{
        EffectPublicationError, EffectQueueServiceInputs,
        service_effect_transition_queue_with_context,
    },
    effects::EffectProgramPort,
    master_assignment_release::release_master_assignments_with_service,
};
use radias_synth_domain::{
    delay_time::DelayTimeState,
    effect_lfo_program::EffectLfoProgram,
    effect_transition_queue::{EffectRingState, EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientQueueWord, CoefficientSlot, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_assignment_release::MasterAssignmentRelease,
    master_effect_control::{MasterControlState, MasterMidiBinding},
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
fn master(words: [u32; 64]) -> MasterControlState {
    let assignments = EffectCoefficientAssignments {
        order: core::array::from_fn(|i| words[i] as u8),
        slots: core::array::from_fn(|slot| {
            let i = 9 + 6 * slot;
            CoefficientSlot {
                indices: core::array::from_fn(|n| words[i + n] as u16),
                target: words[i + 4],
                last_value: words[i + 5],
            }
        }),
    };
    MasterControlState {
        assignments,
        lfo: EffectLfoProgram {
            bytes: [1, 2, 3, 4, 5, 6],
        },
        delay: DelayTimeState {
            cached_tempo: 1111,
            capacity: 65536,
            ratio: 0x12345678,
            limited: 0xaabbccdd,
        },
        pending: [123, 456],
        pending_control: 789,
        owner: 7,
        update_marker: words[63],
        filter_cache: FilterEffectCache {
            frequency: 12345,
            dirty: 1,
        },
        midi_binding: MasterMidiBinding {
            source: 3,
            values: [-12, 34],
        },
        rotary_mode: 1,
        rotary_speed: 2,
        work_slot: 28,
        coefficient_scratch: [0x12345678; 73],
    }
}
fn pool_words(s: &MasterControlState) -> [u32; 64] {
    let mut result = [0; 64];
    for (i, &v) in s.assignments.order.iter().enumerate() {
        result[i] = u32::from(v);
    }
    for (slot, r) in s.assignments.slots.iter().enumerate() {
        let i = 9 + 6 * slot;
        for (n, &v) in r.indices.iter().enumerate() {
            result[i + n] = u32::from(v);
        }
        result[i + 4] = r.target;
        result[i + 5] = r.last_value;
    }
    result[63] = s.update_marker;
    result
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let raw = fs::read(root.join("runs/native-clone/master-assignment-release-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated assignment-release corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d415231 {
        return Err("Wrong assignment-release corpus".into());
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
        mut calls,
        mut services,
        mut changed,
        mut program_packets,
        mut producers,
        mut suspended,
        mut errors,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut family_counts = [0usize; 2];
    let mut direct_counts = [0usize; 4];
    let mut flags_seen = [0usize; 256];
    let mut mask_seen = [0usize; 512];
    let mut capacities = [0usize; 4];
    let mut wraps = [0usize; 3];
    for family in 0..2u32 {
        for f in 0..if family == 0 { 4 } else { 256 } {
            for direct_index in 0..4u32 {
                for m in 0..if family == 0 { 512 } else { 11 } {
                    if r.one() != 0x1000 {
                        return Err("Release scene tag changed".into());
                    }
                    let [
                        case,
                        declared_family,
                        declared_f,
                        declared_direct,
                        declared_m,
                        flags,
                        mask,
                        count,
                        wrap,
                        direct,
                        argument,
                    ] = r.array::<11>();
                    let expected_flags = if family == 0 {
                        [0, 3, 5, 6][f as usize]
                    } else {
                        f
                    };
                    let expected_mask = if family == 0 {
                        m
                    } else if m == 0 {
                        0
                    } else if m == 10 {
                        511
                    } else {
                        1 << (m - 1)
                    };
                    if [
                        case,
                        declared_family,
                        declared_f,
                        declared_direct,
                        declared_m,
                    ] != [scenes as u32, family, f, direct_index, m]
                        || flags != expected_flags
                        || mask != expected_mask
                        || count != 2043 + (f + direct_index + m) % 4
                        || wrap != (f + direct_index + m) % 3
                        || direct != [0, 1, 0x10000, 0x80000000][direct_index as usize]
                        || argument != [0, 1, 0x80000000, 0xffffffff][scenes % 4]
                    {
                        return Err("Declared release scene changed".into());
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
                    let initial_pool = r.array::<64>();
                    let saved = master(initial_pool);
                    let mut domain_state = saved;
                    let mut application_state = saved;
                    let mut whole_state = saved;
                    let mut queue = EffectTransitionQueue::from_state(before, words)
                        .map_err(|_| "Invalid declared release queue")?;
                    let mut app_queue = EffectTransitionQueue::from_state(before, words)
                        .map_err(|_| "Invalid declared app queue")?;
                    let mut whole_queue = EffectTransitionQueue::from_state(before, words)
                        .map_err(|_| "Invalid declared whole queue")?;
                    let mut release = MasterAssignmentRelease::new(direct);
                    let mut app_release = MasterAssignmentRelease::new(direct);
                    let mut whole_release = MasterAssignmentRelease::new(direct);
                    let n = r.one();
                    let mut all_inputs = Vec::new();
                    let mut all_packets = Vec::new();
                    let mut app_port = Port::default();
                    let mut no_inputs = Inputs {
                        values: Vec::new(),
                        cursor: 0,
                    };
                    let result = release_master_assignments_with_service(
                        &mut application_state,
                        &mut app_release,
                        &mut app_queue,
                        &mut app_port,
                        &buffers,
                        &mut no_inputs,
                    );
                    if (n == 0 && result != Ok(()))
                        || (n != 0 && result != Err(EffectPublicationError::ServiceInputRequired))
                    {
                        return Err("Application release suspension differs".into());
                    }
                    suspended += usize::from(n != 0);
                    for service_index in 0..n {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        let expected_queue = r.array::<9>();
                        let expected_pool = r.array::<64>();
                        let count = r.one();
                        let expected: Vec<_> = (0..count)
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
                        all_inputs.push((tick, status));
                        if release.publish_available(&mut domain_state, &mut queue) {
                            return Err("Native release returned before source service".into());
                        }
                        let mut port = Port::default();
                        service_effect_transition_queue_with_context(
                            &mut queue,
                            &mut port,
                            &buffers,
                            tick,
                            status,
                            release.host_context(),
                        )
                        .map_err(|_| "Native release delivery failed")?;
                        if state_words(queue.state()) != expected_queue
                            || pool_words(&domain_state) != expected_pool
                            || port.packets != expected
                        {
                            errors += 1;
                            if first.is_null() {
                                first = json!({"scene":scenes,"service":service_index,"native_queue":state_words(queue.state()),"original_queue":expected_queue,"native_pool":pool_words(&domain_state).to_vec(),"original_pool":expected_pool.to_vec(),"native_packets":port.packets,"original_packets":expected});
                            }
                        }
                        let mut one = Inputs {
                            values: vec![(tick, status)],
                            cursor: 0,
                        };
                        let result = release_master_assignments_with_service(
                            &mut application_state,
                            &mut app_release,
                            &mut app_queue,
                            &mut app_port,
                            &buffers,
                            &mut one,
                        );
                        if one.cursor != 1
                            || (service_index + 1 < n
                                && result != Err(EffectPublicationError::ServiceInputRequired))
                            || (service_index + 1 == n && result != Ok(()))
                        {
                            return Err("Resumable release application differs".into());
                        }
                        program_packets += expected.iter().filter(|p| p.0).count();
                        all_packets.extend(expected);
                        services += 1;
                    }
                    let expected_queue = r.array::<9>();
                    let expected_pool = r.array::<64>();
                    let n = r.one();
                    let original_emission: Vec<_> = (0..n).map(|_| r.word()).collect();
                    let mut native_emission = Vec::new();
                    for slot in &saved.assignments.slots {
                        if slot.target != 0x2f7 {
                            native_emission.push(CoefficientQueueWord {
                                address: slot.target as u16,
                                tagged_value: slot.last_value & 0xffffff,
                            });
                            for (i, v) in [0x2f7, 0, 0x7a9765, 0x5689a].into_iter().enumerate() {
                                native_emission.push(CoefficientQueueWord {
                                    address: slot.indices[i],
                                    tagged_value: v | if direct == 0 {
                                        (0x84 - i as u32) << 24
                                    } else {
                                        0
                                    },
                                });
                            }
                        }
                    }
                    let mut inputs = Inputs {
                        values: all_inputs,
                        cursor: 0,
                    };
                    let mut whole_port = Port::default();
                    let whole_result = release_master_assignments_with_service(
                        &mut whole_state,
                        &mut whole_release,
                        &mut whole_queue,
                        &mut whole_port,
                        &buffers,
                        &mut inputs,
                    );
                    if !release.publish_available(&mut domain_state, &mut queue)
                        || whole_result != Ok(())
                        || inputs.cursor != inputs.values.len()
                        || app_port.packets != all_packets
                        || whole_port.packets != all_packets
                        || original_emission != native_emission
                        || release.published_words() != native_emission.len()
                        || app_release.published_words() != native_emission.len()
                        || whole_release.published_words() != native_emission.len()
                        || [&queue, &app_queue, &whole_queue]
                            .iter()
                            .any(|q| state_words(q.state()) != expected_queue)
                        || [&domain_state, &application_state, &whole_state]
                            .iter()
                            .any(|s| pool_words(s) != expected_pool)
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"scene":scenes,"final_native_queue":state_words(queue.state()),"final_original_queue":expected_queue,"final_native_pool":pool_words(&domain_state).to_vec(),"final_original_pool":expected_pool.to_vec()});
                        }
                    }
                    for mut s in [domain_state, application_state, whole_state] {
                        s.assignments = saved.assignments;
                        s.update_marker = saved.update_marker;
                        if s != saved {
                            errors += 1;
                        }
                    }
                    let n = r.one();
                    let expected: Vec<[u32; 4]> = (0..n).map(|_| r.array()).collect();
                    for q in [&queue, &app_queue, &whole_queue] {
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
                    producers += original_emission.len();
                    scenes += 1;
                    calls += 1;
                    family_counts[family as usize] += 1;
                    direct_counts[direct_index as usize] += 1;
                    flags_seen[flags as usize] += 1;
                    mask_seen[mask as usize] += 1;
                    capacities[(count - 2043) as usize] += 1;
                    wraps[wrap as usize] += 1;
                }
            }
        }
    }
    let totals = r.array::<6>();
    let passed = errors == 0
        && totals
            == [
                scenes as u32,
                calls as u32,
                services as u32,
                changed as u32,
                program_packets as u32,
                producers as u32,
            ]
        && family_counts == [8192, 11264]
        && direct_counts == [4864; 4]
        && mask_seen.iter().all(|&n| n >= 16)
        && flags_seen.iter().all(|&n| n >= 44)
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"errors":errors,"first_difference":first,"whole_original_assignment_release_calls":calls,"nested_original_synchronous_services":services,"nested_original_word_producers":producers,"changed_ring_entries_compared":changed,"original_program_packets_compared":program_packets,"resumable_missing_input_suspensions":suspended,"family_counts":family_counts,"direct_switch_counts":direct_counts,"all512_occupancy_masks_verified":mask_seen.to_vec(),"all256_control_bytes_verified":flags_seen.to_vec(),"initial_capacity_counts":capacities,"ring_wrap_scenes":wraps,"pool_state_before_and_after_synchronous_service_verified":passed,"whole_application_release_scenes_verified":if passed{scenes}else{0},"unrelated_native_Master_fields_preserved":passed,"source_current_state_or_ring_outputs_replayed_as_native_inputs":false,"FXD03_sample_audio_verified":false,"physical_interrupt_or_host_wait_timing_verified":false});
    fs::write(
        root.join("runs/native-clone/master-assignment-release-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native assignment release:{calls} whole calls,{services} nested services,{errors} differences"
    );
    if !passed {
        return Err("Whole assignment release differs".into());
    }
    Ok(())
}
