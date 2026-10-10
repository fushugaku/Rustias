//! Whole SYS07AC52 TubePreAmpSim dispatches and original host publications.
use radias_synth_application::{
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::EffectProgramPort,
    tube_effect::change_tube_parameter,
};
use radias_synth_domain::{
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    tube_effect::TubeParameterEdit,
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.tube_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/tube-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Tube corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x54555031 {
        return Err("Wrong Tube corpus".into());
    }
    let (mut cases, mut errors, mut rejections, mut host_words, mut host_packets, mut maximum) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [0usize; 13];
    let mut first = Value::Null;
    let mut seen = BTreeSet::new();
    for sequence in 0..16 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Tube sequence order differs".into());
        }
        let mut assignments = initial;
        for step in 0..1279 {
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
            ] = read.array::<11>();
            if tag != 0x2000
                || scene != sequence
                || number != step
                || kind != 9
                || slot != (sequence + step) % 8
            {
                return Err("Tube input profile changed".into());
            }
            let parameters = read.bytes();
            let edit = TubeParameterEdit {
                parameter: parameter as u8,
                origin: origin as u16,
                value: value as u8,
                parameters,
                interpolation: EffectInterpolationControl::from_owners(
                    direct,
                    parameter as u8,
                    owner1,
                    owner2,
                    false,
                ),
            };
            let before = read.array::<63>();
            let after = read.array::<63>();
            let matches_before = assignment_words(&assignments) == before;
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
            let saved = assignments;
            let mut rejected = Queue {
                reject: true,
                ..Default::default()
            };
            if change_tube_parameter(&mut assignments, &mut rejected, edit, &tables).is_ok()
                || assignments != saved
                || rejected.batch.is_some()
            {
                return Err("Tube rejection modified assignments".into());
            }
            rejections += 1;
            let mut queue = Queue::default();
            change_tube_parameter(&mut assignments, &mut queue, edit, &tables)
                .map_err(|_| "Tube rejected declared edit")?;
            let native = queue.batch.ok_or("Tube publication missing")?;
            let mut port = Port::default();
            dispatch_parameter_batch(&mut port, &native)?;
            if !matches_before
                || native.words() != original
                || assignment_words(&assignments) != after
                || port.packets != original_packets
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"step":step,"parameter":parameter,"value":value,"parameters":parameters,"before_matches":matches_before,"native_queue":native.words().iter().map(|w| [u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"original_queue":original.iter().map(|w| [u32::from(w.address),w.tagged_value]).collect::<Vec<_>>(),"native_assignments":assignment_words(&assignments),"original_assignments":after.as_slice(),"native_packets":port.packets,"original_packets":original_packets});
                }
            }
            maximum = maximum.max(native.words().len());
            cases += 1;
            counts[parameter as usize] += 1;
            host_packets += port.packets.len();
            host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
            seen.insert((sequence, parameter, value));
        }
    }
    let mut expected = BTreeSet::new();
    for sequence in 0..16 {
        for (parameter, range) in tables.parameter_ranges.iter().enumerate() {
            for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
            {
                expected.insert((sequence, parameter as u32, value as u32));
            }
        }
    }
    let passed = cases == 20464
        && read.cursor == read.words.len()
        && seen == expected
        && errors == 0
        && rejections == cases;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":cases,"parameter_counts":counts,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":host_packets,"maximum_parameter_batch_words":maximum,"all_thirteen_parameter_domains_complete":true,"continuous_sequences":16,"insert_instances":8,"bias_and_saturation_use_stored_snapshot":true,"requested_parameters_and_stored_snapshot_are_separate_inputs":true,"prior_coefficient_or_assignment_outputs_replayed_as_native_inputs":false,"all_original_callees_execute_without_stubs":true,"master_parameter_wrapper_or_FXD03_sound_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/tube-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native TubePreAmpSim: {cases} original edits, {errors} differences");
    if !passed {
        return Err("Native Tube controller differs".into());
    }
    Ok(())
}
