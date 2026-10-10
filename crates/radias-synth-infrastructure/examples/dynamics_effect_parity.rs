//! Compare all dynamics properties against whole original SYS07AC52 calls.
use radias_synth_application::{
    dynamics_effect::DynamicsParameterRequest,
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::{EffectProgramPort, EffectUpdateController},
};
use radias_synth_domain::{
    dynamics_effect::DynamicsEffectKind, effect_parameters::EffectParameterBatch,
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
struct Port {
    packets: Vec<(u16, u16, Vec<u32>)>,
}
impl EffectProgramPort for Port {
    type Error = std::convert::Infallible;
    fn upload_program(&mut self, _: u16, _: &[u64], _: u16) -> Result<(), Self::Error> {
        unreachable!()
    }
    fn write_coefficient(&mut self, a: u16, v: u32, c: u16) -> Result<(), Self::Error> {
        self.packets.push((a, c, vec![v]));
        Ok(())
    }
    fn write_coefficient_packet(&mut self, a: u16, v: &[u32], c: u16) -> Result<(), Self::Error> {
        self.packets.push((a, c, v.to_vec()));
        Ok(())
    }
}
fn state_words(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut r = s.order.map(u32::from).to_vec();
    for slot in s.slots {
        r.extend(slot.indices.map(u32::from));
        r.extend([slot.target, slot.last_value]);
    }
    r
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.dynamics_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let mut controller = EffectUpdateController {
        assignments: initial,
    };
    let raw = fs::read(root.join("runs/native-clone/dynamics-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated dynamics reference".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x44595031 {
        return Err("Invalid dynamics reference".into());
    }
    let (
        mut cursor,
        mut cases,
        mut errors,
        mut rejected,
        mut host_words,
        mut packets,
        mut sequences,
        mut max_batch,
    ) = (
        1usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut seen = BTreeSet::new();
    let mut first = Value::Null;
    let mut counts = [[0usize; 6]; 3];
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xffe4, 0xfffe, 0xffff,
    ];
    let switches = [0, 1, 0x10000, 0x80000000];
    while cursor < words.len() {
        if words[cursor] == 0x1000 {
            if words[cursor + 1] as usize != sequences {
                return Err("Sequence order changed".into());
            }
            cursor += 2;
            controller.assignments = initial;
            sequences += 1;
            continue;
        }
        let [
            tag,
            sequence,
            step,
            kind,
            parameter,
            slot,
            origin,
            value,
            direct,
            first_owner,
            second_owner,
        ]: [u32; 11] = words[cursor..cursor + 11].try_into().unwrap();
        cursor += 11;
        let effect_kind = DynamicsEffectKind::from_effect_type(kind as u8)
            .ok_or("Source dynamics kind changed")?;
        if tag != 0x2000
            || sequence >= 16
            || parameter as usize >= effect_kind.parameter_count()
            || slot != (sequence + step) % 8
            || origin != origins[step as usize % 16]
            || direct != switches[sequence as usize / 4]
        {
            return Err("Source dynamics input profile changed".into());
        }
        let route = sequence % 4;
        if first_owner
            != if matches!(route, 1 | 3) {
                parameter
            } else {
                0x1234
            }
            || second_owner
                != if matches!(route, 2 | 3) {
                    parameter
                } else {
                    0x5678
                }
        {
            return Err("Source owner profile changed".into());
        }
        // Stored object bytes are reference inputs but these handlers use the
        // requested raw byte. No coefficient output is an input to Rust.
        cursor += 20;
        let request = DynamicsParameterRequest {
            kind: effect_kind,
            origin: origin as u16,
            parameter: parameter as u8,
            value: value as u8,
            direct_switch: direct,
            owners: [first_owner, second_owner],
            master: false,
        };
        let before = controller.assignments;
        let mut no_room = Queue {
            reject: true,
            ..Default::default()
        };
        if controller
            .change_dynamics_parameter(&mut no_room, &tables, request)
            .is_err()
            && controller.assignments == before
            && no_room.batch.is_none()
        {
            rejected += 1;
        }
        let mut queue = Queue::default();
        controller.change_dynamics_parameter(&mut queue, &tables, request).map_err(|_| format!("Native rejected legal dynamics value: kind {kind}, parameter {parameter}, value {value}"))?;
        let batch = queue.batch.ok_or("Dynamics batch absent")?;
        let before_matches = state_words(&before) == words[cursor..cursor + 63];
        cursor += 63;
        let after_matches = state_words(&controller.assignments) == words[cursor..cursor + 63];
        cursor += 63;
        let n = words[cursor] as usize;
        cursor += 1;
        max_batch = max_batch.max(n);
        let original_queue: Vec<_> = words[cursor..cursor + 2 * n]
            .chunks_exact(2)
            .map(|b| [b[0], b[1]])
            .collect();
        cursor += 2 * n;
        let native_queue: Vec<_> = batch
            .words()
            .iter()
            .map(|b| [u32::from(b.address), b.tagged_value])
            .collect();
        let np = words[cursor] as usize;
        cursor += 1;
        let mut original_packets = Vec::new();
        for _ in 0..np {
            let address = words[cursor] as u16;
            let control = words[cursor + 1] as u16;
            let len = words[cursor + 2] as usize;
            cursor += 3;
            original_packets.push((address, control, words[cursor..cursor + len].to_vec()));
            cursor += len;
        }
        let mut port = Port::default();
        dispatch_parameter_batch(&mut port, &batch)?;
        if !before_matches
            || !after_matches
            || native_queue != original_queue
            || port.packets != original_packets
        {
            errors += 1;
            if first.is_null() {
                first = json!({"sequence":sequence,"step":step,"kind":kind,"parameter":parameter,"value":value,"before":before_matches,"after":after_matches,"native_queue":native_queue,"original_queue":original_queue,"native_state":state_words(&controller.assignments)});
            }
        }
        host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
        packets += port.packets.len();
        cases += 1;
        counts[kind as usize - 1][parameter as usize] += 1;
        seen.insert((sequence, kind, parameter, value));
    }
    let minimum = [[0, 0, 1, 0, 0, 0], [0, 0, 0, 0, 0, 23], [0, 0, 0, 0, 0, 23]];
    let maximum = [
        [100, 1, 127, 127, 127, 0],
        [100, 1, 69, 40, 127, 88],
        [100, 1, 127, 127, 127, 88],
    ];
    let mut expected = BTreeSet::new();
    let mut invalid_rejected = 0;
    for kind in 1..=3 {
        let effect_kind = DynamicsEffectKind::from_effect_type(kind).unwrap();
        for parameter in 0..effect_kind.parameter_count() {
            let lo = minimum[kind as usize - 1][parameter];
            let hi = maximum[kind as usize - 1][parameter];
            for sequence in 0..16 {
                for value in lo..=hi {
                    expected.insert((sequence, u32::from(kind), parameter as u32, value));
                }
            }
            for value in 0..=255 {
                if (lo..=hi).contains(&value) {
                    continue;
                }
                let before = controller.assignments;
                let mut queue = Queue::default();
                let request = DynamicsParameterRequest {
                    kind: effect_kind,
                    origin: 0xffff,
                    parameter: parameter as u8,
                    value: value as u8,
                    direct_switch: 0,
                    owners: [parameter as u32; 2],
                    master: false,
                };
                if controller
                    .change_dynamics_parameter(&mut queue, &tables, request)
                    .is_err()
                    && controller.assignments == before
                    && queue.batch.is_none()
                {
                    invalid_rejected += 1;
                }
            }
        }
        for parameter in effect_kind.parameter_count()..=255 {
            let before = controller.assignments;
            let mut queue = Queue::default();
            let request = DynamicsParameterRequest {
                kind: effect_kind,
                origin: 0,
                parameter: parameter as u8,
                value: 0,
                direct_switch: 0,
                owners: [0; 2],
                master: false,
            };
            if controller
                .change_dynamics_parameter(&mut queue, &tables, request)
                .is_err()
                && controller.assignments == before
                && queue.batch.is_none()
            {
                invalid_rejected += 1;
            }
        }
    }
    let passed = sequences == 16
        && cases == 23152
        && seen == expected
        && errors == 0
        && rejected == cases
        && invalid_rejected == 3656
        && max_batch == 11;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":cases,"parameter_counts":counts,"errors":errors,"first_difference":first,
        "continuous_sequences":sequences,"insert_instances":8,"full_queue_atomic_rejections":rejected,"invalid_parameter_rejections":invalid_rejected,
        "host_words_compared":host_words,"host_packets_compared":packets,"maximum_parameter_batch_words":max_batch,
        "all_17_dynamics_parameter_domains_complete":true,"all_original_dependency_handlers_execute_without_stubs":true,
        "previous_coefficient_outputs_replayed_as_inputs":false,"native_executes_firmware_instructions":false,
        "FXD03_audio_or_complete_master_effect_wrapper_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/dynamics-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Compressor/Limiter/Gate: {cases} original edits, {errors} differences, {host_words} host words"
    );
    if !passed {
        return Err("Native dynamics compiler differs or coverage incomplete".into());
    }
    Ok(())
}
