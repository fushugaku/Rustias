//! Execute native St.Decimator parameter compilation against complete original
//! SYS07AC52 calls. Every original dependency handler runs without substitution.
use radias_synth_application::{
    decimator_effect::{DecimatorParameterRequest, EffectParameterQueue, dispatch_parameter_batch},
    effects::{EffectProgramPort, EffectUpdateController},
};
use radias_synth_domain::{
    decimator_effect::{DecimatorEffectState, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::EffectCoefficientAssignments,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, b: &EffectParameterBatch) -> Result<(), Self::Error> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*b);
        Ok(())
    }
}
#[derive(Default)]
struct Port(Vec<(u16, u16, Vec<u32>)>);
impl EffectProgramPort for Port {
    type Error = std::convert::Infallible;
    fn upload_program(&mut self, _: u16, _: &[u64], _: u16) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn write_coefficient(&mut self, a: u16, v: u32, c: u16) -> Result<(), Self::Error> {
        self.0.push((a, c, vec![v]));
        Ok(())
    }
    fn write_coefficient_packet(&mut self, a: u16, v: &[u32], c: u16) -> Result<(), Self::Error> {
        self.0.push((a, c, v.to_vec()));
        Ok(())
    }
}
fn state_words(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut result = s.order.map(u32::from).to_vec();
    for slot in s.slots {
        result.extend(slot.indices.map(u32::from));
        result.extend([slot.target, slot.last_value]);
    }
    result
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.decimator_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let mut controller = EffectUpdateController {
        assignments: initial,
    };
    let raw = fs::read(root.join("runs/native-clone/decimator-parameters-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated dependency reference".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x44504d31 || words[1] != 1029 {
        return Err("Invalid dependency reference".into());
    }
    let mut cursor = 2;
    let mut owner_errors = 0;
    let mut owners = BTreeSet::new();
    for _ in 0..words[1] {
        let [parameter, first, second, master, original]: [u32; 5] =
            words[cursor..cursor + 5].try_into().unwrap();
        cursor += 5;
        let candidate =
            EffectInterpolationControl::from_owners(0, parameter as u8, first, second, master != 0);
        owner_errors += usize::from(candidate.enabled_argument != original);
        owners.insert((parameter, first, second, master));
    }
    let (
        mut cases,
        mut errors,
        mut queue_words,
        mut host_words,
        mut packets,
        mut rejections,
        mut max_words,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut seen = BTreeSet::new();
    let mut first_difference = Value::Null;
    let mut by_parameter = [0usize; 7];
    while cursor < words.len() {
        let [
            sequence,
            step,
            parameter,
            origin,
            value,
            pre_lpf,
            stored,
            direct,
            first,
            second,
        ]: [u32; 10] = words[cursor..cursor + 10].try_into().unwrap();
        cursor += 10;
        let origins = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xfff4, 0xfffe, 0xffff,
        ];
        let switches = [0, 1, 0x10000, 0x80000000];
        let route = sequence % 4;
        let expected_first = if matches!(route, 1 | 3) {
            parameter
        } else {
            0x1234
        };
        let expected_second = if matches!(route, 2 | 3) {
            parameter
        } else {
            0x5678
        };
        if sequence >= 16
            || origin != origins[step as usize % 16]
            || direct != switches[sequence as usize / 4]
            || first != expected_first
            || second != expected_second
            || pre_lpf != if sequence & 1 != 0 { 127 } else { 0 }
            || stored != (3 * value + 29 * sequence) % 95
        {
            return Err("Original parameter input profiles are incomplete or altered".into());
        }
        if step == 0 {
            controller.assignments = initial;
        }
        let before = controller.assignments;
        let request = DecimatorParameterRequest {
            origin: origin as u16,
            parameter: parameter as u8,
            value: value as u8,
            state: DecimatorEffectState {
                pre_lpf: pre_lpf as u8,
                stored_sample_rate: stored as u8,
            },
            direct_switch: direct,
            owners: [first, second],
            master: false,
        };
        let mut rejected = Queue {
            reject: true,
            ..Default::default()
        };
        if controller
            .change_decimator_parameter(&mut rejected, &tables, request)
            .is_err()
            && controller.assignments == before
            && rejected.batch.is_none()
        {
            rejections += 1;
        }
        let mut queue = Queue::default();
        controller
            .change_decimator_parameter(&mut queue, &tables, request)
            .map_err(|_| "Original dependency input rejected")?;
        let batch = queue.batch.ok_or("Native dependency batch absent")?;
        let before_matches = state_words(&before) == words[cursor..cursor + 63];
        cursor += 63;
        let after_matches = state_words(&controller.assignments) == words[cursor..cursor + 63];
        cursor += 63;
        let count = words[cursor] as usize;
        cursor += 1;
        let original_queue: Vec<_> = words[cursor..cursor + 2 * count]
            .chunks_exact(2)
            .map(|b| [b[0], b[1]])
            .collect();
        cursor += 2 * count;
        let candidate_queue: Vec<_> = batch
            .words()
            .iter()
            .map(|b| [u32::from(b.address), b.tagged_value])
            .collect();
        let packet_count = words[cursor] as usize;
        cursor += 1;
        let mut original_packets = Vec::new();
        for _ in 0..packet_count {
            let address = words[cursor];
            let control = words[cursor + 1];
            let n = words[cursor + 2] as usize;
            cursor += 3;
            original_packets.push((
                address as u16,
                control as u16,
                words[cursor..cursor + n].to_vec(),
            ));
            cursor += n;
        }
        let mut port = Port::default();
        dispatch_parameter_batch(&mut port, &batch)?;
        let queue_matches = candidate_queue == original_queue;
        let packets_match = port.0 == original_packets;
        if !before_matches || !after_matches || !queue_matches || !packets_match {
            errors += 1;
            if first_difference.is_null() {
                first_difference = json!({"sequence":sequence,"step":step,"parameter":parameter,"value":value,
                "before_matches":before_matches,"after_matches":after_matches,"queue_matches":queue_matches,"packets_match":packets_match,
                "native_queue":candidate_queue,"original_queue":original_queue,"native_packets":port.0,"original_packets":original_packets});
            }
        }
        queue_words += batch.words().len();
        max_words = max_words.max(batch.words().len());
        host_words += port.0.iter().map(|p| p.2.len()).sum::<usize>();
        packets += port.0.len();
        cases += 1;
        by_parameter[parameter as usize] += 1;
        seen.insert((sequence, parameter, value));
    }
    let limits = [100, 1, 100, 94, 20, 127, 127];
    let mut expected = BTreeSet::new();
    for sequence in 0..16 {
        for (parameter, limit) in limits.into_iter().enumerate() {
            for value in if parameter == 6 { 1 } else { 0 }..=limit {
                expected.insert((sequence, parameter as u32, value));
            }
        }
    }
    let mut expected_owners = BTreeSet::new();
    for master in [0, 1, 127] {
        for parameter in 0..7 {
            for first in 0..7 {
                for second in 0..7 {
                    expected_owners.insert((parameter, first, second, master));
                }
            }
        }
    }
    let mut unsupported = 0;
    for parameter in (7..14).chain(std::iter::once(255)) {
        let before = controller.assignments;
        let mut queue = Queue::default();
        let request = DecimatorParameterRequest {
            origin: 0,
            parameter,
            value: 0,
            state: DecimatorEffectState {
                pre_lpf: 0,
                stored_sample_rate: 0,
            },
            direct_switch: 0,
            owners: [0, 0],
            master: false,
        };
        if controller
            .change_decimator_parameter(&mut queue, &tables, request)
            .is_err()
            && queue.batch.is_none()
            && controller.assignments == before
        {
            unsupported += 1;
        }
    }
    let passed = owner_errors == 0
        && owners == expected_owners
        && cases == 9200
        && seen == expected
        && errors == 0
        && rejections == 9200
        && unsupported == 8;
    let report = json!({"passed":passed,"original_complete_parameter_dispatches":cases,"parameter_call_counts":by_parameter,
        "original_owner_selections":owners.len(),"owner_errors":owner_errors,"errors":errors,"first_difference":first_difference,
        "queue_words_compared":queue_words,"host_words_compared":host_words,"host_packets_compared":packets,"largest_parameter_batch_words":max_words,
        "full_queue_atomic_rejections":rejections,"unsupported_parameters_rejected_atomically":unsupported,
        "original_dependency_dispatcher_and_all_called_handlers_execute_without_stubs":true,
        "original_coefficients_assignment_states_or_queue_outputs_used_as_native_inputs":false,
        "native_controller_executes_firmware_instructions":false,"unsupported_LFO_parameters_7_to_13_not_qualified":true,
        "FXD03_audio_arithmetic_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/decimator-parameters-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native St.Decimator parameter dispatch: {cases} complete original calls, {errors} differences; {owner_errors} owner errors"
    );
    if !passed {
        return Err("Native parameter dispatch differs from original SYS".into());
    }
    Ok(())
}
