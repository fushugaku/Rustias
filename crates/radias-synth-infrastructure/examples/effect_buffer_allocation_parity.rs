//! Whole unchanged SYS074302 and all mixed neighbor publications.
use radias_synth_application::{
    effect_buffer_allocation::{EffectBufferAllocationPort, relocate_effect_buffers},
    effect_parameters::dispatch_parameter_batch,
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    delay_time::DelayClock,
    effect_buffer_allocation::{EffectBufferInstance, PreparedEffectBufferAllocation},
    effect_buffers::{EffectBufferSlice, PreparedEffectBufferTemplate},
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_updates::CoefficientQueueWord,
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
    fn objects(&mut self) -> [[u8; 136]; 8] {
        core::array::from_fn(|_| self.bytes())
    }
}
#[derive(Default)]
struct Port {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl EffectBufferAllocationPort for Port {
    type Error = ();
    fn accept_allocation(&mut self, p: &PreparedEffectBufferAllocation) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(p.batch);
        Ok(())
    }
}
#[derive(Default)]
struct Host {
    packets: Vec<(u32, u32, Vec<u32>)>,
}
impl EffectProgramPort for Host {
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
fn read32(raw: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(raw[offset..offset + 4].try_into().unwrap())
}
fn read16(raw: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(raw[offset..offset + 2].try_into().unwrap())
}
fn write32(raw: &mut [u8], offset: usize, value: u32) {
    raw[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn write16(raw: &mut [u8], offset: usize, value: u16) {
    raw[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}
fn decode(raw: &[u8; 136]) -> EffectBufferInstance {
    EffectBufferInstance {
        kind: read32(raw, 4) as u8,
        origin: read16(raw, 0x40),
        parameters: raw[0x0c..0x20].try_into().unwrap(),
        buffer_origin: read32(raw, 0x50),
        layout: EffectBufferSlice {
            offset: read32(raw, 0x4c),
            frames: read32(raw, 0x48),
        },
        cached_tempo: read16(raw, 0x44),
        ratio: read32(raw, 0x54),
        limited: read32(raw, 0x58),
        pending_coefficients: [read32(raw, 0x7c), read32(raw, 0x80)],
        pending_argument: read32(raw, 0x84),
    }
}
fn store(raw: &mut [u8; 136], v: EffectBufferInstance) {
    write16(raw, 0x44, v.cached_tempo);
    write32(raw, 0x48, v.layout.frames);
    write32(raw, 0x4c, v.layout.offset);
    write32(raw, 0x54, v.ratio);
    write32(raw, 0x58, v.limited);
    write32(raw, 0x7c, v.pending_coefficients[0]);
    write32(raw, 0x80, v.pending_coefficients[1]);
    write32(raw, 0x84, v.pending_argument);
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.buffer_allocation_tables()?;
    let sys_word = |a: u32| read32(&source, (a - 0x0c000000) as usize + 0x1000);
    let raw = fs::read(root.join("runs/native-clone/effect-buffer-allocation-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated allocation corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x45424131 {
        return Err("Wrong allocation corpus".into());
    }
    let (
        mut calls,
        mut errors,
        mut queue_errors,
        mut services,
        mut rejections,
        mut host_words,
        mut host_packets,
        mut maximum,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut slot_counts = [0usize; 8];
    let mut pairs = [[0usize; 31]; 31];
    let tempos = [200, 400, 1200, 3000];
    for sequence in 0..4u32 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Allocation sequence differs".into());
        }
        let mut arena: [[u8; 136]; 8] = core::array::from_fn(|slot| {
            core::array::from_fn(|i| (0x35 + sequence * 7 + slot as u32 * 11 + i as u32 * 13) as u8)
        });
        for (slot, v) in arena.iter_mut().enumerate() {
            write32(v, 0, slot as u32);
            write32(v, 4, 0);
            write32(v, 8, sys_word(0x0c0cceac));
            write16(v, 0x44, tempos[(sequence as usize + slot) % 4]);
        }
        if read.objects() != arena {
            return Err("Declared initial allocation objects differ".into());
        }
        let mut step = 0u32;
        for slot in 0..8 {
            for kind in 0..31 {
                for neighbor in 0..31 {
                    if read.array::<6>() != [0x2000, sequence, step, slot, kind, neighbor] {
                        return Err("Allocation input profile differs".into());
                    }
                    for (target, kind) in [(slot, kind), (slot ^ 1, neighbor)] {
                        let p = read.bytes::<20>();
                        let origin = read.one();
                        let base = read.one();
                        let a = &mut arena[target as usize];
                        write32(a, 4, kind);
                        write32(a, 8, sys_word(0x0c0cceac + 4 * kind));
                        a[0x0c..0x20].copy_from_slice(&p);
                        write16(a, 0x40, origin as u16);
                        write32(a, 0x50, base);
                    }
                    let [tempo, status] = read.array::<2>();
                    let words = read.array::<80>();
                    let before_matches = read.objects() == arena;
                    let original_arena = read.objects();
                    let original_template = read.array::<80>();
                    let n = read.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: read.one() as u16,
                            tagged_value: read.one(),
                        })
                        .collect();
                    let original_services = read.one();
                    let packet_count = read.one();
                    let mut original_packets = Vec::new();
                    for _ in 0..packet_count {
                        let a = read.one();
                        let c = read.one();
                        let n = read.one();
                        original_packets.push((
                            a,
                            c,
                            (0..n).map(|_| read.one()).collect::<Vec<_>>(),
                        ));
                    }
                    let mut instances = arena.map(|a| decode(&a));
                    let initial = instances;
                    let mut template = PreparedEffectBufferTemplate {
                        layout: instances[slot as usize].layout,
                        words,
                        count: 80,
                    };
                    let old_template = template;
                    let clock = DelayClock {
                        tempo: tempo as u16,
                        status: status as u8,
                    };
                    let mut rejected = Port {
                        reject: true,
                        ..Default::default()
                    };
                    if relocate_effect_buffers(
                        &mut instances,
                        &mut template,
                        &mut rejected,
                        slot as u8,
                        clock,
                        &tables,
                    )
                    .is_ok()
                        || instances != initial
                        || template != old_template
                        || rejected.batch.is_some()
                    {
                        return Err("Rejected allocation changed instances or template".into());
                    }
                    rejections += 1;
                    let mut port = Port::default();
                    relocate_effect_buffers(
                        &mut instances,
                        &mut template,
                        &mut port,
                        slot as u8,
                        clock,
                        &tables,
                    )
                    .map_err(|_| format!("Allocation rejected: sequence {sequence}, step {step}, slot {slot}, kind {kind}, neighbor {neighbor}, instances {instances:?}"))?;
                    let batch = port.batch.ok_or("Allocation commands missing")?;
                    for (a, v) in arena.iter_mut().zip(instances) {
                        store(a, v);
                    }
                    let mut host = Host::default();
                    dispatch_parameter_batch(&mut host, &batch)?;
                    if !before_matches
                        || arena != original_arena
                        || template.words != original_template
                        || batch.words() != original
                        || host.packets != original_packets
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"sequence":sequence,"step":step,"slot":slot,"kind":kind,"neighbor":neighbor,"before_matches":before_matches,"native_instances":arena.iter().map(|a|a.as_slice()).collect::<Vec<_>>(),"original_instances":original_arena.iter().map(|a|a.as_slice()).collect::<Vec<_>>(),"native_template":template.words.as_slice(),"original_template":original_template.as_slice(),"native_queue":batch.words().iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>()});
                        }
                    }
                    let mut active = EffectCommandQueue::default();
                    active
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native allocation queue rejected")?;
                    let mut packets = Vec::new();
                    for _ in 0..original_services {
                        let output = active.service(0, false);
                        packets.extend(output.packets[..usize::from(output.count)].iter().map(
                            |p| {
                                (
                                    u32::from(p.address),
                                    1,
                                    p.values[..usize::from(p.count)].to_vec(),
                                )
                            },
                        ));
                        services += 1;
                    }
                    if packets != original_packets || active.state().count != 0 {
                        queue_errors += 1;
                    }
                    maximum = maximum.max(batch.words().len());
                    calls += 1;
                    slot_counts[slot as usize] += 1;
                    pairs[kind as usize][neighbor as usize] += 1;
                    host_packets += host.packets.len();
                    host_words += host.packets.iter().map(|p| p.2.len()).sum::<usize>();
                    step += 1;
                }
            }
        }
    }
    let passed = calls == 30752
        && slot_counts == [3844; 8]
        && pairs.iter().flatten().all(|&n| n == 32)
        && errors == 0
        && queue_errors == 0
        && rejections == calls
        && read.cursor == read.words.len();
    let report = json!({"passed":passed,"whole_original_relocation_calls":calls,"whole_original_queue_service_calls":services,"slot_counts":slot_counts,"type_pair_counts":pairs.iter().map(|r|r.as_slice()).collect::<Vec<_>>(),"errors":errors,"queue_errors":queue_errors,"first_difference":first,"full_transaction_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_allocation_batch_words":maximum,"all_961_type_pairs_at_all_eight_insert_slots":true,"continuous_sequences":4,"all_1088_insert_state_bytes_and_80_template_words_compared":true,"native_states_evolve_without_source_output_replay":true,"all_original_callees_execute_without_stubs":true,"whole_SYS074302_insert_allocator_qualified":passed,"complete_effect_lifecycle_master_allocation_physical_clock_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-buffer-allocation-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native whole FX allocation: {calls} original calls, {errors} state/template/command and {queue_errors} queue differences"
    );
    if !passed {
        return Err("Native full buffer allocation differs".into());
    }
    Ok(())
}
