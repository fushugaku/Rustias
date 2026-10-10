//! Every original insert pair through SYS07CCB2 and unchanged queue service.
use radias_synth_application::{
    effect_pair_transition::transition_effect_pair, effect_parameters::EffectParameterQueue,
};
use radias_synth_domain::{
    effect_parameters::EffectParameterBatch, effect_routing::EffectRoutingInstance,
    effect_transition_queue::EffectTransitionQueue, effect_updates::CoefficientQueueWord,
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
struct Reject {
    calls: usize,
}
impl EffectParameterQueue for Reject {
    type Error = ();
    fn enqueue_parameter(&mut self, _: &EffectParameterBatch) -> Result<(), Self::Error> {
        self.calls += 1;
        Err(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&system)?.pair_transition_tables()?;
    let raw = fs::read(root.join("runs/native-clone/effect-pair-transition-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated pair transition corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x45505431 {
        return Err("Wrong pair transition corpus".into());
    }
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xffe4, 0xfffe, 0xffff,
    ];
    let (mut cases, mut errors, mut host_words, mut host_packets) =
        (0usize, 0usize, 0usize, 0usize);
    let mut counts = [[0usize; 31]; 31];
    let mut suppressed = 0usize;
    let mut first = Value::Null;
    let mut reject = Reject::default();
    for first_kind in 0..31 {
        for second_kind in 0..31 {
            for profile in 0..16 {
                for mode in [0, 1, 0x10000, 0x80000000] {
                    if r.array::<4>() != [first_kind, second_kind, profile, mode] {
                        return Err("Original transition input profile changed".into());
                    }
                    let instances = [
                        EffectRoutingInstance {
                            kind: first_kind as u8,
                            origin: origins[profile as usize],
                            parameters: [0; 20],
                        },
                        EffectRoutingInstance {
                            kind: second_kind as u8,
                            origin: origins[(profile as usize + 7) % 16],
                            parameters: [0; 20],
                        },
                    ];
                    let batch = tables
                        .prepare(&instances, mode)
                        .ok_or("Original pair rejected")?;
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let mut queue = EffectTransitionQueue::default();
                    transition_effect_pair(&mut queue, &tables, &instances, mode)
                        .map_err(|_| "Native queue rejected pair")?;
                    let mut blocked = queue.service(0xfffe, 3);
                    if blocked.coefficients.count != 0 || queue.state().rings[0].count != n as u16 {
                        errors += 1;
                    }
                    blocked = queue.service(0xffff, 0);
                    let actual: Vec<_> = blocked.coefficients.packets
                        [..usize::from(blocked.coefficients.count)]
                        .iter()
                        .map(|p| {
                            (
                                u32::from(p.address),
                                1,
                                p.values[..usize::from(p.count)].to_vec(),
                            )
                        })
                        .collect();
                    let packets = r.one();
                    let expected: Vec<_> = (0..packets)
                        .map(|_| {
                            let address = r.one();
                            let control = r.one();
                            let size = r.one();
                            (
                                address,
                                control,
                                (0..size).map(|_| r.one()).collect::<Vec<_>>(),
                            )
                        })
                        .collect();
                    if batch.words() != original
                        || actual != expected
                        || queue.state().rings[0].count != 0
                        || blocked.program.is_some()
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":cases,"input":[first_kind,second_kind,profile,mode],"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}"),"native_host":actual,"original_host":expected});
                        }
                    }
                    if transition_effect_pair(&mut reject, &tables, &instances, mode).is_ok() {
                        errors += 1;
                    }
                    if !tables.second_active[first_kind as usize] {
                        suppressed += 1;
                    }
                    counts[first_kind as usize][second_kind as usize] += 1;
                    cases += 1;
                    host_packets += actual.len();
                    host_words += actual.iter().map(|p| p.2.len()).sum::<usize>();
                }
            }
        }
    }
    let mut full = EffectTransitionQueue::default();
    full.enqueue_words(&vec![
        CoefficientQueueWord {
            address: 3,
            tagged_value: 1
        };
        2046
    ])
    .map_err(|_| "Full declared queue failed")?;
    let before = full.state();
    let pair = [EffectRoutingInstance::default(); 2];
    let atomic =
        transition_effect_pair(&mut full, &tables, &pair, 1).is_err() && full.state() == before;
    let passed = errors == 0
        && cases == 61504
        && r.cursor == r.words.len()
        && atomic
        && reject.calls == cases;
    let report = json!({"passed":passed,"whole_original_pair_transition_calls":cases,"whole_original_queue_service_calls":cases,
        "errors":errors,"first_difference":first,"type_pair_counts":counts.map(|row| row.to_vec()).to_vec(),"host_words_compared":host_words,"host_packets_compared":host_packets,
        "second_role_suppression_cases":suppressed,"application_transaction_rejections":reject.calls,"full_native_queue_rejection_preserves_state":atomic,
        "all_31_insert_types_both_roles_and_full_32_bit_mute_arguments_verified":true,"original_functions_and_all_callees_execute_without_stubs":true,
        "FXD03_audio_full_Early_Reflect_controller_master_wrapper_or_physical_clock_verified":false});
    fs::write(
        root.join("runs/native-clone/effect-pair-transition-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native pair transition: {cases} original calls, {host_words} host words, {errors} differences"
    );
    if !passed {
        return Err("Native pair transition differs".into());
    }
    Ok(())
}
