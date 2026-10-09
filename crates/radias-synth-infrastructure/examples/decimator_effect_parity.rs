//! Complete original St.Decimator parameter setter calls, native compiled data,
//! assignments, queue order and actual host packet comparison. No effects audio.
use radias_synth_application::{
    decimator_effect::{EffectParameterQueue, dispatch_parameter_batch},
    effects::{EffectProgramPort, EffectUpdateController},
};
use radias_synth_domain::{
    decimator_effect::{
        DecimatorEffectChange, DecimatorEffectState, EffectInterpolationControl,
        EffectParameterBatch,
    },
    effect_updates::EffectCoefficientAssignments,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::json;
use std::{collections::BTreeSet, fs, path::PathBuf};
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, batch: &EffectParameterBatch) -> Result<(), Self::Error> {
        if self.reject {
            return Err(());
        }
        self.batch = Some(*batch);
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
    let raw = fs::read(root.join("runs/native-clone/decimator-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated original decimator records".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x44454631 {
        return Err("Unsupported original decimator format".into());
    }
    let (
        mut cursor,
        mut cases,
        mut errors,
        mut queue_words,
        mut host_words,
        mut packets,
        mut rejections,
    ) = (1usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut seen = BTreeSet::new();
    let mut first = serde_json::Value::Null;
    while cursor < words.len() {
        let row = &words[cursor..cursor + 10];
        cursor += 10;
        let [
            sequence,
            step,
            kind,
            origin,
            offset,
            value,
            pre_lpf,
            stored,
            direct,
            enabled,
        ]: [u32; 10] = row.try_into().unwrap();
        if step == 0 {
            controller.assignments = initial;
        }
        let before = controller.assignments;
        let change = if kind == 0 {
            DecimatorEffectChange::BitDepth {
                value: value as u8,
                coefficient_offset: offset,
            }
        } else {
            DecimatorEffectChange::SampleRate {
                value: value as u8,
                state: DecimatorEffectState {
                    pre_lpf: pre_lpf as u8,
                    stored_sample_rate: stored as u8,
                },
            }
        };
        let interpolation = EffectInterpolationControl {
            direct_switch: direct as u16,
            enabled_argument: enabled,
        };
        if step == 0 {
            let mut rejected = Queue {
                reject: true,
                ..Default::default()
            };
            if controller
                .change_decimator(&mut rejected, &tables, origin as u16, change, interpolation)
                .is_err()
                && controller.assignments == before
                && rejected.batch.is_none()
            {
                rejections += 1;
            }
        }
        let mut queue = Queue::default();
        controller
            .change_decimator(&mut queue, &tables, origin as u16, change, interpolation)
            .map_err(|_| "Original decimator input rejected")?;
        let batch = queue.batch.ok_or("Decimator batch absent")?;
        let mut difference = state_words(&before) != words[cursor..cursor + 63];
        cursor += 63;
        difference |= state_words(&controller.assignments) != words[cursor..cursor + 63];
        cursor += 63;
        let count = words[cursor] as usize;
        cursor += 1;
        difference |= batch.words().len() != count;
        for entry in batch.words() {
            difference |=
                [u32::from(entry.address), entry.tagged_value] != words[cursor..cursor + 2];
            cursor += 2;
        }
        let mut port = Port::default();
        dispatch_parameter_batch(&mut port, &batch)?;
        let packet_count = words[cursor] as usize;
        cursor += 1;
        difference |= port.packets.len() != packet_count;
        for (address, control, values) in &port.packets {
            let original_count = words[cursor + 2] as usize;
            difference |= u32::from(*address) != words[cursor]
                || u32::from(*control) != words[cursor + 1]
                || values.len() != original_count;
            cursor += 3;
            difference |= values.as_slice() != &words[cursor..cursor + original_count];
            cursor += original_count;
            host_words += values.len();
        }
        if difference {
            errors += 1;
            if first.is_null() {
                first = json!({"sequence":sequence,"step":step,"kind":kind});
            }
        }
        queue_words += batch.words().len();
        packets += port.packets.len();
        cases += 1;
        seen.insert((sequence, step));
    }
    let mut expected = BTreeSet::new();
    for sequence in 0..54 {
        for step in 0..95 {
            expected.insert((sequence, step));
        }
    }
    for sequence in 54..58 {
        for step in 0..21 {
            expected.insert((sequence, step));
        }
    }
    let bad = [
        DecimatorEffectChange::BitDepth {
            value: 21,
            coefficient_offset: 7,
        },
        DecimatorEffectChange::SampleRate {
            value: 95,
            state: DecimatorEffectState {
                pre_lpf: 0,
                stored_sample_rate: 0,
            },
        },
        DecimatorEffectChange::SampleRate {
            value: 94,
            state: DecimatorEffectState {
                pre_lpf: 1,
                stored_sample_rate: 95,
            },
        },
    ];
    let mut invalid = 0;
    for change in bad {
        let before = controller.assignments;
        let mut queue = Queue::default();
        if controller
            .change_decimator(
                &mut queue,
                &tables,
                0,
                change,
                EffectInterpolationControl {
                    direct_switch: 0,
                    enabled_argument: 1,
                },
            )
            .is_err()
            && controller.assignments == before
            && queue.batch.is_none()
        {
            invalid += 1;
        }
    }
    let passed =
        errors == 0 && cases == 5214 && seen == expected && rejections == 58 && invalid == 3;
    let report = json!({"passed":passed,"whole_original_parameter_calls":cases,"errors":errors,"first_difference":first,
        "queue_words_compared":queue_words,"host_words_compared":host_words,"host_packets_compared":packets,
        "full_queue_atomic_rejections":rejections,"invalid_parameter_changes_rejected_atomically":invalid,
        "Fs_and_Bit_domains_complete":true,"original_table_bytes_are_declared_inputs":true,
        "recorded_coefficients_assignment_states_or_queues_used_as_native_inputs":false,
        "requested_Fs_and_stored_Fs_are_separate_inputs":true,"full_target_index_before_host_wrap_is_preserved":true,
        "native_controller_executes_firmware_instructions":false,"FXD03_audio_arithmetic_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/decimator-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native St.Decimator: {cases} original parameter calls, {errors} differences");
    if !passed {
        return Err("Native St.Decimator controller differs".into());
    }
    Ok(())
}
