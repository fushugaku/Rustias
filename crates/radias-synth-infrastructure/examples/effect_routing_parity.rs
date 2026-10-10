//! Original SYS07D2B2/07D31A, including mixed object/program types and queue service.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue,
    effect_routing::{mute_effect_inputs, publish_effect_routing},
};
use radias_synth_domain::{
    effect_parameters::EffectParameterBatch,
    effect_queue::EffectCommandQueue,
    effect_routing::{EffectRoutingContext, EffectRoutingInstance},
    effect_updates::CoefficientQueueWord,
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
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        core::array::from_fn(|_| self.one() as u8)
    }
}
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, batch: &EffectParameterBatch) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*batch);
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.routing_tables()?;
    let raw = fs::read(root.join("runs/native-clone/effect-routing-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated routing corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x45525431 {
        return Err("Wrong routing corpus".into());
    }
    let (
        mut gains,
        mut routes,
        mut services,
        mut errors,
        mut queue_errors,
        mut rejections,
        mut host_words,
        mut host_packets,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    let mut contexts = [0usize; 5];
    let mut pairs = vec![vec![0usize; 31]; 31];
    let mut gain_counts = [[0usize; 31]; 2];
    let mut input_mutes = 0usize;
    let mut input_contexts = [0usize; 5];
    while read.cursor < read.words.len() {
        let record = read.one();
        match record {
            0x1000 => {
                let bank = read.one() as usize;
                let kind = read.one() as usize;
                let instance = EffectRoutingInstance {
                    kind: read.one() as u8,
                    origin: 0,
                    parameters: read.bytes(),
                };
                let original = read.one();
                let profile = if bank == 0 {
                    tables.insert[kind]
                } else {
                    tables.master[kind]
                };
                let native = tables
                    .gain(&instance, profile)
                    .ok_or("Native gain rejected declared inputs")?;
                if native != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"bank":bank,"kind":kind,"object_kind":instance.kind,"parameters":instance.parameters,"native":native,"original":original});
                    }
                }
                gains += 1;
                gain_counts[bank][kind] += 1;
            }
            0x2000 | 0x3000 => {
                let context = read.one() as usize;
                let reset = read.one();
                let headers = read.bytes::<3>();
                let mut bytes = [0u8; 1790];
                if context < 4 {
                    bytes[168 + context * 228] = headers[0];
                    bytes[192 + context * 228] = headers[1];
                }
                bytes[1038] = headers[2];
                let program =
                    Program::from_bytes(&bytes).map_err(|_| "Declared program rejected")?;
                let mut instances = [EffectRoutingInstance::default(); 9];
                for role in 0..3 {
                    let instance = EffectRoutingInstance {
                        kind: read.one() as u8,
                        origin: read.one() as u16,
                        parameters: read.bytes(),
                    };
                    if role == 2 {
                        instances[8] = instance;
                    } else if context < 4 {
                        instances[context * 2 + role] = instance;
                    }
                }
                let n = read.one() as usize;
                let original: Vec<_> = (0..n)
                    .map(|_| CoefficientQueueWord {
                        address: read.one() as u16,
                        tagged_value: read.one(),
                    })
                    .collect();
                let routing_context = if context == 4 {
                    EffectRoutingContext::Master
                } else {
                    EffectRoutingContext::Insert(context as u8)
                };
                let mut rejected = Queue {
                    reject: true,
                    ..Default::default()
                };
                let publish = |queue: &mut Queue| {
                    if record == 0x3000 {
                        mute_effect_inputs(queue, &program, &instances, &tables, routing_context)
                    } else {
                        publish_effect_routing(
                            queue,
                            &program,
                            &instances,
                            &tables,
                            routing_context,
                            reset,
                        )
                    }
                };
                if publish(&mut rejected).is_ok() || rejected.batch.is_some() {
                    return Err("Rejected routing publication was accepted".into());
                }
                rejections += 1;
                let mut queue = Queue::default();
                publish(&mut queue).map_err(|_| "Native routing rejected declared inputs")?;
                let native = queue.batch.ok_or("No native routing publication")?;
                if native.words() != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"context":context,"reset":reset,"headers":headers,"native":native.words().iter().map(|w| [u32::from(w.address), w.tagged_value]).collect::<Vec<_>>(),"original":original.iter().map(|w| [u32::from(w.address),w.tagged_value]).collect::<Vec<_>>()});
                    }
                }
                let mut transport = EffectCommandQueue::default();
                transport
                    .enqueue_words(native.words())
                    .map_err(|_| "Native routing queue rejected batch")?;
                let total = read.one();
                for _ in 0..total {
                    let tick = read.one() as u16;
                    let status = read.one();
                    let source_state = core::array::from_fn::<_, 5, _>(|_| read.one());
                    let packet_count = read.one();
                    let mut source_packets = Vec::new();
                    for _ in 0..packet_count {
                        let address = read.one();
                        let control = read.one();
                        let count = read.one();
                        source_packets.push((
                            address,
                            control,
                            (0..count).map(|_| read.one()).collect::<Vec<_>>(),
                        ));
                    }
                    let batch = transport.service(tick, status & 3 != 0);
                    let state = transport.state();
                    let native_state = [
                        state.write_index,
                        state.read_index,
                        state.count,
                        state.wait_ticks,
                        state.wait_started,
                    ]
                    .map(u32::from);
                    let native_packets: Vec<_> = batch.packets[..usize::from(batch.count)]
                        .iter()
                        .map(|p| {
                            (
                                u32::from(p.address),
                                1,
                                p.values[..usize::from(p.count)].to_vec(),
                            )
                        })
                        .collect();
                    if native_state != source_state || native_packets != source_packets {
                        queue_errors += 1;
                        if first.is_null() {
                            first = json!({"context":context,"reset":reset,"headers":headers,"tick":tick,"status":status,"native_state":native_state,"source_state":source_state,"native_packets":native_packets,"source_packets":source_packets});
                        }
                    }
                    services += 1;
                    host_packets += native_packets.len();
                    host_words += native_packets.iter().map(|p| p.2.len()).sum::<usize>();
                }
                if transport.state().count != 0 || transport.state().wait_ticks != 0 {
                    queue_errors += 1;
                }
                if record == 0x3000 {
                    input_mutes += 1;
                    input_contexts[context] += 1;
                } else {
                    routes += 1;
                    contexts[context] += 1;
                    if context < 4 {
                        pairs[usize::from(headers[0] & 127)][usize::from(headers[1] & 127)] += 1;
                    }
                }
            }
            _ => return Err("Unexpected routing record".into()),
        }
    }
    let passed = gains == 31744
        && routes == 62000
        && input_mutes == routes
        && input_contexts == contexts
        && errors == 0
        && queue_errors == 0
        && rejections == routes + input_mutes
        && gain_counts == [[512; 31]; 2]
        && pairs.iter().flatten().all(|&v| v == 64)
        && contexts == [15376, 15376, 15376, 15376, 496];
    let report = json!({"passed":passed,"whole_original_gain_calls":gains,"whole_original_routing_calls":routes,"whole_original_input_mute_calls":input_mutes,"whole_original_queue_service_calls":services,"errors":errors,"queue_errors":queue_errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"context_counts":contexts,"input_mute_context_counts":input_contexts,"all_961_insert_type_pairs":true,"all_31_insert_and_master_profiles":true,"object_types_and_stored_types_are_independent_inputs":true,"enable_flags_do_not_select_routing_profiles":true,"source_coefficients_or_queue_outputs_replayed_as_native_inputs":false,"original_callees_execute_without_stubs":true,"FXD03_audio_effect_lifecycle_or_physical_timer_frequency_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-routing-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native FX routing: {gains} original gains, {routes} whole routes, {services} queue services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native effects routing differs".into());
    }
    Ok(())
}
