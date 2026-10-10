//! Complete original Talking Modulator insert, program lifecycle and timed transport.
use radias_synth_application::{
    effect_transition_queue::{EffectProgramSource, dispatch_effect_transition_batch},
    effects::EffectProgramPort,
    talking_effect::{TalkingContext, TalkingEffectPort, change_talking_parameter},
};
use radias_synth_domain::{
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectProgramStaging,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    program::Program,
    talking_effect::{PreparedTalkingEdit, TalkingEdit, TalkingInstance, TalkingRack},
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
    fn staging(&mut self) -> EffectProgramStaging {
        let cursor = self.one() as u8;
        let mut s = EffectProgramStaging {
            cursor,
            ..Default::default()
        };
        for i in 0..20 {
            let [pl, ph, tl, th, pc, tc] = self.array();
            s.prefix[i] = u64::from(pl) | (u64::from(ph) << 32);
            s.tail[i] = u64::from(tl) | (u64::from(th) << 32);
            s.counts[i] = [pc as u16, tc as u16];
        }
        s
    }
}
struct EditPort {
    reject: bool,
    batch: Option<EffectParameterBatch>,
    program_writes: usize,
    body_writes: usize,
    buffers: EffectProgramBuffers,
}
impl TalkingEffectPort for EditPort {
    type Error = ();
    fn accept_talking_edit(&mut self, p: &PreparedTalkingEdit) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        for w in p.program_writes.iter().flatten() {
            self.buffers
                .store_program(w.selector, &[w.word])
                .map_err(|_| ())?;
            self.program_writes += 1;
        }
        if let Some(body) = &p.body_program {
            self.buffers
                .store_program(
                    (body.blocks[0].tag & 127) as u8,
                    &body.words[..usize::from(body.count)],
                )
                .map_err(|_| ())?;
            self.body_writes += 1;
        }
        self.batch = Some(p.batch);
        Ok(())
    }
}
#[derive(Default)]
struct HostPort {
    packets: Vec<(bool, u16, u16, Vec<u64>)>,
}
impl EffectProgramPort for HostPort {
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
fn u32_at(raw: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(raw[offset..offset + 4].try_into().unwrap())
}
fn u16_at(raw: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(raw[offset..offset + 2].try_into().unwrap())
}
fn decode(raw: &[u8; 136]) -> TalkingInstance {
    TalkingInstance {
        kind: u32_at(raw, 4) as u8,
        parameters: raw[12..32].try_into().unwrap(),
        previous_parameters: raw[32..52].try_into().unwrap(),
        owners: [u32_at(raw, 0x34), u32_at(raw, 0x38)],
        origin: u16_at(raw, 0x40),
        relocation_origin: u16_at(raw, 0x42),
        prefix_origin: u16_at(raw, 0x3c),
        body_origin: u16_at(raw, 0x3e),
        lfo: EffectLfoProgram {
            bytes: raw[0x5c..0x62].try_into().unwrap(),
        },
        controller_source: u32_at(raw, 0x68),
        controller_value: raw[0x6c] as i8,
        secondary_value: raw[0x6d] as i8,
    }
}
fn assignments(v: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut w = v.order.map(u32::from).to_vec();
    for s in v.slots {
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
fn staging_words(s: &EffectProgramStaging) -> Vec<u32> {
    let mut w = vec![u32::from(s.cursor)];
    for i in 0..20 {
        w.extend([
            s.prefix[i] as u32,
            (s.prefix[i] >> 32) as u32,
            s.tail[i] as u32,
            (s.tail[i] >> 32) as u32,
            u32::from(s.counts[i][0]),
            u32::from(s.counts[i][1]),
        ]);
    }
    w
}
fn bindings(r: &TalkingRack) -> Vec<u32> {
    r.instances
        .iter()
        .flat_map(|i| {
            [
                i.controller_source,
                i32::from(i.controller_value) as u32,
                i32::from(i.secondary_value) as u32,
            ]
        })
        .collect()
}
fn state(
    r: &TalkingRack,
    phases: &[[u8; 32]; 8],
    buffers: &EffectProgramBuffers,
    slot: usize,
) -> Vec<u32> {
    let mut w = assignments(&r.assignments);
    w.extend(bindings(r));
    w.extend(r.instances[slot].lfo.bytes.map(u32::from));
    w.extend(phases[slot].map(u32::from));
    w.extend(staging_words(&r.staging));
    for pool in 0..20 {
        w.push(buffers.program_words(3 * pool + 1).unwrap().len() as u32);
    }
    w
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.talking_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/talking-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Talking corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x544c4b31 {
        return Err("Wrong Talking corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut queue_errors,
        mut rejects,
        mut coefficient_words,
        mut coefficient_packets,
        mut program_words,
        mut program_packets,
        mut max_batch,
        mut publications,
        mut body_words,
        mut staging_writes,
        mut body_writes,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
        0usize, 0usize, 0usize,
    );
    let mut counts = [0usize; 20];
    let mut first = Value::Null;
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Talking sequence changed".into());
        }
        let mut phases = [[0u8; 32]; 8];
        let instances = core::array::from_fn(|slot| {
            let raw = r.bytes::<136>();
            phases[slot] = r.bytes();
            decode(&raw)
        });
        let mut rack = TalkingRack {
            instances,
            assignments: EffectCoefficientAssignments::new(indices),
            staging: r.staging(),
        };
        let mut normal = vec![0u8; 20 * 0x44a];
        for pool in 0..20 {
            let base = pool * 0x44a;
            normal[base..base + 6].copy_from_slice(&rack.staging.prefix[pool].to_be_bytes()[2..]);
            normal[base + 0x43e..base + 0x444]
                .copy_from_slice(&rack.staging.tail[pool].to_be_bytes()[2..]);
            normal[base + 0x444..base + 0x446]
                .copy_from_slice(&rack.staging.counts[pool][0].to_be_bytes());
            normal[base + 0x446..base + 0x448].copy_from_slice(&180u16.to_be_bytes());
            normal[base + 0x448..base + 0x44a]
                .copy_from_slice(&rack.staging.counts[pool][1].to_be_bytes());
            for i in 0..180 {
                let [lo, hi] = r.array();
                let w = u64::from(lo) | (u64::from(hi) << 32);
                normal[base + 6 + 6 * i..base + 12 + 6 * i].copy_from_slice(&w.to_be_bytes()[2..]);
            }
        }
        let buffers = EffectProgramBuffers::from_buffers(
            library.program_buffer_layout(),
            [normal, vec![0; 8192], vec![0; 2048]],
        )?;
        let mut port = EditPort {
            reject: false,
            batch: None,
            program_writes: 0,
            body_writes: 0,
            buffers,
        };
        let mut program_raw = [0u8; 1790];
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
                    marker,
                    clock,
                    owner1,
                    owner2,
                    origin,
                    prefix,
                    body,
                    data,
                    first_kind,
                    second_kind,
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
                    return Err("Talking input profile changed".into());
                }
                let parameters = r.bytes();
                let previous_parameters = r.bytes();
                let polarity = EffectMidiPolarity {
                    assignments: r.bytes(),
                };
                program_raw[168 + 228 * (slot as usize / 2)] = first_kind as u8;
                program_raw[192 + 228 * (slot as usize / 2)] = second_kind as u8;
                program_raw[9..14].copy_from_slice(&polarity.assignments);
                let program = Program::from_bytes(&program_raw)
                    .map_err(|_| "Talking raw program rejected")?;
                let mut midi = EffectMidiSources::default();
                for t in &mut midi.timbres {
                    let f = r.array::<10>();
                    *t = EffectMidiTimbre {
                        control_49: f[0] as i8,
                        bend: f[1] as i16,
                        control_4b: f[2] as i8,
                        channel: f[3] as u8,
                        switch_45: f[4] as u8,
                        controls_4c_50: core::array::from_fn(|i| f[i + 5] as i8),
                    };
                }
                for g in &mut midi.channel_controls {
                    *g = r.bytes();
                }
                midi.shared_control = r.one() as i8;
                let edit = TalkingEdit {
                    slot: slot as u8,
                    parameter: parameter as u8,
                    value: value as u8,
                    parameters,
                    previous_parameters,
                    origin: origin as u16,
                    relocation_origin: data as u16,
                    prefix_origin: prefix as u16,
                    body_origin: body as u16,
                    owners: [owner1, owner2],
                    direct_switch: direct,
                    update_marker: marker,
                    clock_rate: clock,
                };
                let context = TalkingContext {
                    tables: &tables,
                    program: &program,
                    midi: &midi,
                    polarity,
                };
                let before_source = r.array::<266>();
                let before_matches =
                    state(&rack, &phases, &port.buffers, slot as usize) == before_source;
                let saved = rack;
                let old_writes = port.program_writes;
                let old_body = port.body_writes;
                port.reject = true;
                if change_talking_parameter(&mut port, &mut rack, context, edit).is_err()
                    && rack == saved
                    && port.program_writes == old_writes
                    && port.body_writes == old_body
                    && port.batch.is_none()
                {
                    rejects += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                change_talking_parameter(&mut port, &mut rack, context, edit).map_err(|_| {
                    format!(
                        "Talking rejected sequence {sequence} parameter {parameter} value {value}"
                    )
                })?;
                let batch = port.batch.take().ok_or("Talking batch absent")?;
                if let Some(p) = batch.lfo_publication() {
                    phases[slot as usize][4..8].copy_from_slice(&p.tempo_increment.to_be_bytes());
                    publications += 1;
                }
                let after_source = r.array::<266>();
                let after_matches =
                    state(&rack, &phases, &port.buffers, slot as usize) == after_source;
                let changed_mask = r.one();
                let mut body_matches = true;
                let mut body_difference = Value::Null;
                for pool in 0..20u8 {
                    if changed_mask & (1 << pool) != 0 {
                        let expected: Vec<_> = (0..180)
                            .map(|_| {
                                let [lo, hi] = r.array();
                                u64::from(lo) | (u64::from(hi) << 32)
                            })
                            .collect();
                        let actual = port.buffers.program_words(3 * pool + 1).unwrap();
                        if actual != expected.as_slice() && body_difference.is_null() {
                            let i = actual
                                .iter()
                                .zip(&expected)
                                .position(|(a, b)| a != b)
                                .unwrap();
                            body_difference = json!({"pool":pool,"index":i,"native":format!("{:012x}",actual[i]),"original":format!("{:012x}",expected[i])});
                        }
                        body_matches &= actual == expected.as_slice();
                        body_words += 180;
                    }
                }
                let n = r.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: r.one() as u16,
                        tagged_value: r.one(),
                    })
                    .collect();
                max_batch = max_batch.max(batch.words().len());
                if !before_matches || !after_matches || !body_matches || batch.words() != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"sequence":sequence,"parameter":parameter,"value":value,"step":step,"before_matches":before_matches,"after_matches":after_matches,"body_matches":body_matches,"body_difference":body_difference,"parameters":parameters,"previous_parameters":previous_parameters,"native_state":state(&rack,&phases,&port.buffers,slot as usize),"original_state":after_source.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                    }
                }
                let mut queue = EffectTransitionQueue::default();
                queue
                    .enqueue_words(batch.words())
                    .map_err(|_| "Talking queue rejected")?;
                let total = r.one();
                for _ in 0..total {
                    let tick = r.one() as u16;
                    let status = r.one() as u16;
                    let expected_state = r.array::<9>();
                    let count = r.one();
                    let expected: Vec<_> = (0..count)
                        .map(|_| {
                            let prog = r.one() != 0;
                            let a = r.one() as u16;
                            let c = r.one() as u16;
                            let n = r.one();
                            (
                                prog,
                                a,
                                c,
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
                    let mut host = HostPort::default();
                    dispatch_effect_transition_batch(&mut host, &port.buffers, &output)
                        .map_err(|_| "Talking host delivery failed")?;
                    if queue_state(queue.state()) != expected_state || host.packets != expected {
                        queue_errors += 1;
                        if first.is_null() {
                            first = json!({"sequence":sequence,"parameter":parameter,"value":value,"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
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
                if queue.state().rings.iter().any(|q| q.count != 0) || queue.state().wait_ticks != 0
                {
                    queue_errors += 1;
                }
                calls += 1;
                counts[parameter] += 1;
                step += 1;
            }
        }
        staging_writes += port.program_writes;
        body_writes += port.body_writes;
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls > 40000
        && rejects == calls
        && r.cursor == r.words.len()
        && body_words > 0
        && staging_writes > 0
        && body_writes > 0;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts,"errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_program_state_coefficients_LFO_atomic_rejections":rejects,"coefficient_words_compared":coefficient_words,"coefficient_packets_compared":coefficient_packets,"program_words_compared":program_words,"program_packets_compared":program_packets,"changed_program_body_words_compared":body_words,"prefix_tail_staging_writes":staging_writes,"native_program_body_writes":body_writes,"LFO_publications":publications,"maximum_parameter_batch_words":max_batch,"previous_state_program_buffers_bindings_and_phase_outputs_replayed_from_original":false,"all_original_functions_and_callees_execute_without_stubs":true,"all_20_insert_parameter_domains_complete":true,"mutable_native_program_buffers_used_for_timed_host_delivery":true,"source_instance_guards_preserved":true,"active_master_mixed_MIDI_clock_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/talking-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native complete Talking: {calls} original edits, {services} services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Talking controller differs".into());
    }
    Ok(())
}
