//! Whole selected Insert program preparation, independent mutable buffers and timed 48-bit host words.
use radias_synth_application::{
    effect_transition_queue::dispatch_effect_transition_batch,
    effects::EffectProgramPort,
    insert_program_initialization::{InsertProgramInitializationPort, initialize_insert_program},
};
use radias_synth_domain::{
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_control::EffectOrigins,
    effect_lfo_program::EffectLfoProgram,
    effect_modulation::GrainModulationHistory,
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::CoefficientQueueWord,
    insert_effect_construction::InsertEffectInstance,
    insert_program_initialization::PreparedInsertProgramInitialization,
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
    fn objects(&mut self) -> [[u8; 136]; 8] {
        core::array::from_fn(|_| self.bytes())
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

fn project(
    instances: &[InsertEffectInstance; 8],
    anchors: [[u8; 136]; 8],
    system: &[u8],
) -> [[u8; 136]; 8] {
    let mut objects = anchors;
    for (b, i) in objects.iter_mut().zip(instances) {
        put(b, 4, u32::from(i.buffer.kind));
        put(
            b,
            8,
            long(system, 0x1000 + 0x0cceac + 4 * usize::from(i.buffer.kind)),
        );
        b[0x0c..0x20].copy_from_slice(&i.buffer.parameters);
        b[0x40..0x42].copy_from_slice(&i.buffer.origin.to_be_bytes());
        put(b, 0x64, i.extended_program);
    }
    objects
}
struct Port {
    reject: bool,
    buffers: EffectProgramBuffers,
    batch: Option<EffectParameterBatch>,
}
impl InsertProgramInitializationPort for Port {
    type Error = ();
    fn accept_insert_program_initialization(
        &mut self,
        p: &PreparedInsertProgramInitialization,
    ) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        let mut next = self.buffers.clone();
        for b in &p.program.blocks[..usize::from(p.program.block_count)] {
            let start = usize::from(b.word_start);
            let end = start + usize::from(b.count);
            next.store_program((b.tag & 127) as u8, &p.program.words[start..end])
                .map_err(|_| ())?;
        }
        self.buffers = next;
        self.batch = Some(p.batch);
        Ok(())
    }
}
#[derive(Default)]
struct Host {
    packets: Vec<(u16, u16, Vec<u64>)>,
}
impl EffectProgramPort for Host {
    type Error = Infallible;
    fn upload_program(&mut self, a: u16, w: &[u64], c: u16) -> Result<(), Infallible> {
        self.packets.push((a, c, w.to_vec()));
        Ok(())
    }
    fn write_coefficient(&mut self, _: u16, _: u32, _: u16) -> Result<(), Infallible> {
        panic!("Selected Insert program wrote coefficients")
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
    let tables = library.insert_program_initialization_tables()?;
    let raw = fs::read(root.join("runs/native-clone/insert-program-initialization-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated selected Insert program corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x49504931 {
        return Err("Wrong selected Insert program corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut transport_errors,
        mut rejected,
        mut changed_bytes,
        mut program_words,
        mut program_packets,
        mut extended,
        mut alias_loads,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut types = [0usize; 31];
    let mut slots = [0usize; 8];
    let mut selectors = [0usize; 4];
    let mut first = Value::Null;
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Selected Insert program sequence changed".into());
        }
        let mut anchors = r.objects();
        let mut instances = anchors.map(decode);
        if project(&instances, anchors, &system) != anchors {
            return Err("Declared initial Insert program state differs".into());
        }
        let mut bytes: [Vec<u8>; 3] = core::array::from_fn(|_| Vec::new());
        for b in &mut bytes {
            let n = r.one();
            *b = (0..n).map(|_| r.one() as u8).collect();
        }
        let mut port = Port {
            reject: false,
            buffers: EffectProgramBuffers::from_buffers(library.program_buffer_layout(), bytes)?,
            batch: None,
        };
        for slot in 0..8usize {
            for kind in 0..31u8 {
                for variant in 0..4usize {
                    let [
                        tag,
                        seq,
                        arg_slot,
                        arg_kind,
                        arg_variant,
                        selector,
                        body,
                        data,
                        relocation,
                        _global_slot,
                    ] = r.array();
                    if [tag, seq, arg_slot, arg_kind, arg_variant]
                        != [
                            0x2000,
                            sequence,
                            slot as u32,
                            u32::from(kind),
                            variant as u32,
                        ]
                        || selector != [0, 1, 2, 255][variant]
                    {
                        return Err("Selected Insert program input changed".into());
                    }
                    instances[slot].buffer.kind = kind;
                    instances[slot].buffer.parameters[7] = selector as u8;
                    instances[slot].buffer.origin = data as u16;
                    anchors[slot][0x3e..0x40].copy_from_slice(&(body as u16).to_be_bytes());
                    anchors[slot][0x42..0x44].copy_from_slice(&(relocation as u16).to_be_bytes());
                    let origins = EffectOrigins {
                        program: body as u16,
                        data: data as u16,
                        coefficients: relocation as u16,
                    };
                    let before = r.objects();
                    let after = r.objects();
                    let n = r.one();
                    let expected_changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let saved = instances;
                    let buffers = port.buffers.clone();
                    port.reject = true;
                    port.batch = None;
                    if initialize_insert_program(&mut instances[slot], &mut port, &tables, origins)
                        .is_err()
                        && instances == saved
                        && port.batch.is_none()
                        && (0..3).all(|i| port.buffers.buffer_bytes(i) == buffers.buffer_bytes(i))
                    {
                        rejected += 1
                    } else {
                        errors += 1
                    }
                    port.reject = false;
                    initialize_insert_program(&mut instances[slot],&mut port,&tables,origins).map_err(|_|format!("Native selected Insert program rejected:{sequence}/{slot}/{kind}/{variant}"))?;
                    let batch = port.batch.take().ok_or("Missing Insert program batch")?;
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
                    changed_bytes += changes.len();
                    let mut only_flag = saved;
                    only_flag[slot].extended_program = instances[slot].extended_program;
                    if project(&saved, anchors, &system) != before
                        || project(&instances, anchors, &system) != after
                        || instances != only_flag
                        || batch.words() != original
                        || changes != expected_changes
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,slot as u32,u32::from(kind),variant as u32,selector],"prior_matches":project(&saved,anchors,&system)==before,"state_matches":project(&instances,anchors,&system)==after,"unrelated_native_state_preserved":instances==only_flag,"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}"),"native_changes":changes.len(),"original_changes":expected_changes.len(),"first_buffer_difference":changes.iter().zip(&expected_changes).find(|(a,b)|a!=b)});
                        }
                    }
                    maximum = maximum.max(batch.words().len());
                    if instances[slot].extended_program != 0 {
                        extended += 1;
                        if slot == 7 {
                            alias_loads += 1;
                        }
                    }
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native selected Insert program queue rejected")?;
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
                            .map_err(|_| "Native selected Insert program delivery failed")?;
                        if queue_state(queue.state()) != expected_state || host.packets != expected
                        {
                            transport_errors += 1;
                            if first.is_null() {
                                first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":host.packets,"original_packets":expected});
                            }
                        }
                        program_words += host.packets.iter().map(|p| p.2.len()).sum::<usize>();
                        program_packets += host.packets.len();
                        services += 1;
                    }
                    if queue.state().rings.iter().any(|r| r.count != 0) {
                        transport_errors += 1
                    }
                    calls += 1;
                    types[usize::from(kind)] += 1;
                    slots[slot] += 1;
                    selectors[variant] += 1;
                }
            }
        }
    }
    let passed = errors == 0
        && transport_errors == 0
        && calls == 3968
        && rejected == calls
        && types == [128; 31]
        && slots == [496; 8]
        && selectors == [992; 4]
        && extended == 256
        && alias_loads == 32
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_selected_insert_program_calls":calls,"whole_original_timed_queue_services":services,"type_counts":types.to_vec(),"slot_counts":slots,"selector_counts":selectors,"errors":errors,"transport_errors":transport_errors,"first_difference":first,"whole_instance_program_buffers_and_queue_atomic_rejections":rejected,"changed_program_buffer_bytes_compared":changed_bytes,"program_words_compared":program_words,"program_packets_compared":program_packets,"extended_program_loads":extended,"slot7_selected_Master_body_alias_loads":alias_loads,"maximum_program_batch_words":maximum,"native_evolving_instance_or_program_buffers_replayed_from_original":false,"physical_slot_used_independently_of_random_global_work_slot":true,"all_three_program_buffer_banks_and_counts_compared":true,"original_Master_scratch_and_all_phase_guards_preserved":true,"native_unrelated_instance_state_preserved":true,"initial_parameter_mask_full_rack_busy_rebuild_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/insert-program-initialization-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native selected Insert programs:{calls} original calls,{services} services,{errors}/{transport_errors} differences"
    );
    if !passed {
        return Err("Selected Insert program preparation differs".into());
    }
    Ok(())
}
