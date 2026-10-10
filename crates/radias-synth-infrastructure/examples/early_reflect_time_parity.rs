//! Complete original Early Reflect tap calculation and four queue services.
use radias_synth_application::{
    early_reflect_time::change_early_reflect_time,
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    early_reflect_time::EarlyReflectTimeEdit, effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue, effect_updates::CoefficientQueueWord,
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
        let value = self.words[self.cursor];
        self.cursor += 1;
        value
    }
    fn array<const N: usize>(&mut self) -> [u32; N] {
        core::array::from_fn(|_| self.one())
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&source)?.early_reflect_time_tables()?;
    let raw = fs::read(root.join("runs/native-clone/early-reflect-time-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Early Reflect Time corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x45525431 {
        return Err("Wrong Early Reflect Time corpus".into());
    }
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xffe4, 0xfffe, 0xffff,
    ];
    let buffers = [0, 1, 640, 65535, 0x80000000, 0xfffffffe, 0xffffffff, 0x2f7];
    let (
        mut calls,
        mut errors,
        mut queue_errors,
        mut rejections,
        mut host_words,
        mut host_packets,
        mut services,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    for (profile, origin) in origins.into_iter().enumerate() {
        for size in 0..88 {
            for delay in 0..78 {
                let buffer = buffers[profile % 8];
                if read.array::<5>() != [profile as u32, size, delay, origin, buffer] {
                    return Err("Early Reflect Time input profile changed".into());
                }
                let edit = EarlyReflectTimeEdit {
                    origin: origin as u16,
                    buffer_origin: buffer,
                    size: size as u8,
                    pre_delay: delay as u8,
                };
                let n = read.one();
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: read.one() as u16,
                        tagged_value: read.one(),
                    })
                    .collect();
                let packet_count = read.one();
                let mut original_packets = Vec::new();
                for _ in 0..packet_count {
                    let a = read.one();
                    let c = read.one();
                    let n = read.one();
                    original_packets.push((a, c, (0..n).map(|_| read.one()).collect::<Vec<_>>()));
                }
                let mut rejected = Queue {
                    reject: true,
                    ..Default::default()
                };
                if change_early_reflect_time(&mut rejected, edit, &tables).is_ok()
                    || rejected.batch.is_some()
                {
                    return Err("Rejected Early Reflect Time submitted commands".into());
                }
                rejections += 1;
                let mut queue = Queue::default();
                change_early_reflect_time(&mut queue, edit, &tables)
                    .map_err(|_| "Declared Early Reflect Time rejected")?;
                let batch = queue
                    .batch
                    .ok_or("Early Reflect Time publication missing")?;
                let mut port = Port::default();
                dispatch_parameter_batch(&mut port, &batch)?;
                if batch.words() != original || port.packets != original_packets {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"profile":profile,"size":size,"pre_delay":delay,"origin":origin,"buffer_origin":buffer,"native_queue":batch.words().iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w|[u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"native_packets":port.packets,"original_packets":original_packets});
                    }
                }
                let mut active = EffectCommandQueue::default();
                active
                    .enqueue_words(batch.words())
                    .map_err(|_| "Native Early Reflect Time queue rejected commands")?;
                let mut packets = Vec::new();
                for i in 0..4 {
                    let output = active.service(0, false);
                    packets.extend(output.packets[..usize::from(output.count)].iter().map(|p| {
                        (
                            u32::from(p.address),
                            1,
                            p.values[..usize::from(p.count)].to_vec(),
                        )
                    }));
                    if output.count != 4 || active.state().count != 12 - 4 * i {
                        queue_errors += 1;
                    }
                    services += 1;
                }
                if packets != original_packets || active.state().count != 0 {
                    queue_errors += 1;
                }
                calls += 1;
                host_packets += port.packets.len();
                host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
            }
        }
    }
    let passed = calls == 109824
        && read.cursor == read.words.len()
        && errors == 0
        && queue_errors == 0
        && rejections == calls
        && host_words == 1757184
        && host_packets == 1757184
        && services == 439296;
    let report = json!({"passed":passed,"whole_original_time_calls":calls,"whole_original_queue_service_calls":services,"errors":errors,"queue_errors":queue_errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"all_88_size_by_78_pre_delay_pairs":true,"sixteen_taps_and_coefficient_origin_wrap":true,"declared_32_bit_buffer_origin_boundary_profiles":true,"whole_queue_service_boundaries_compared":true,"native_uses_immutable_delay_scale_and_tap_tables":true,"original_queue_or_coefficient_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,"complete_Early_Reflect_parameter_wrapper_allocation_physical_clock_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/early-reflect-time-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Early Reflect Time: {calls} original calls and {services} queue services, {errors} coefficient and {queue_errors} queue differences"
    );
    if !passed {
        return Err("Native Early Reflect Time differs".into());
    }
    Ok(())
}
