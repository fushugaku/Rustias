//! Exact St.Filter coefficient, frequency-cache and atomic queue comparisons.
use radias_synth_application::{
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::{EffectProgramPort, EffectUpdateController},
    filter_effect::FilterEffectController,
};
use radias_synth_domain::{
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_updates::EffectCoefficientAssignments,
    filter_effect::{FilterEffectCache, FilterEffectFrequency},
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
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
    let tables = library.filter_effect_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let mut controller = FilterEffectController {
        updates: EffectUpdateController {
            assignments: initial,
        },
        caches: [FilterEffectCache::default(); 9],
    };
    let raw = fs::read(root.join("runs/native-clone/filter-effect-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated filter effect corpus".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x46455031 {
        return Err("Wrong filter effect corpus".into());
    }
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xffe2, 0xfffe, 0xffff,
    ];
    let switches = [0, 1, 0x10000, 0x80000000];
    let depths = [1, 64, 127, 70];
    let modulations: [i16; 16] = [
        -32768, -16384, -256, -1, 0, 1, 127, 128, 255, 256, 16384, 32767, -128, -255, 4096, -4096,
    ];
    let (mut cursor, mut errors, mut rejected, mut host_words, mut packets, mut skipped) =
        (1usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [0usize; 3];
    let mut sequences = [0usize; 3];
    let mut expected_step = 0;
    let mut current_phase = 0;
    let mut current_sequence = 0;
    let mut first = Value::Null;
    while cursor < words.len() {
        if words[cursor] == 0x1000 {
            if current_phase != 0
                && expected_step
                    != if current_phase == 1 {
                        32768
                    } else if current_phase == 2 {
                        16384
                    } else {
                        128
                    }
            {
                return Err("Source filter sequence incomplete".into());
            }
            let phase = words[cursor + 1] as usize;
            let sequence = words[cursor + 2] as usize;
            cursor += 3;
            if !(1..=3).contains(&phase) || sequence != sequences[phase - 1] {
                return Err("Source filter sequence order differs".into());
            }
            sequences[phase - 1] += 1;
            current_phase = phase;
            current_sequence = sequence;
            expected_step = 0;
            controller.updates.assignments = initial;
            controller.caches = [FilterEffectCache::default(); 9];
            continue;
        }
        let [
            tag,
            phase,
            sequence,
            step,
            slot,
            origin,
            direct,
            enabled,
            cutoff,
            resonance,
            value,
            modulation,
            dirty,
        ]: [u32; 13] = words[cursor..cursor + 13].try_into().unwrap();
        cursor += 13;
        if tag != 0x2000
            || phase as usize != current_phase
            || sequence as usize != current_sequence
            || step != expected_step
        {
            return Err("Source filter record order differs".into());
        }
        if phase == 1 {
            let declared_dirty = if step % 2 == 1 {
                0
            } else {
                [0, 1, 2, 0x80000000][step as usize / 2 % 4]
            };
            if cutoff != step / 256
                || resonance != (step / 2) % 128
                || slot != (sequence + step / 2) % 9
                || origin != origins[step as usize / 2 % 16]
                || value != depths[sequence as usize % 4]
                || modulation != i32::from(modulations[sequence as usize]) as u32
                || dirty != declared_dirty
                || direct != 0
                || enabled != 0
            {
                return Err("Source frequency profile differs".into());
            }
        } else if phase == 2 {
            if resonance != step / 128
                || value != step % 128
                || origin != origins[step as usize % 16]
                || cutoff != 0
                || slot != 0
                || direct != 0
                || enabled != 0
                || modulation != 0
                || dirty != 0
            {
                return Err("Source trim profile differs".into());
            }
        } else if value != step
            || origin != origins[step as usize % 16]
            || direct != switches[sequence as usize / 4]
            || enabled != sequence % 4
            || resonance != 0
            || cutoff != 0
            || slot != 0
            || modulation != 0
            || dirty != 0
        {
            return Err("Source response profile differs".into());
        }
        let before = controller.updates.assignments;
        let old_caches = controller.caches;
        let frequency = FilterEffectFrequency {
            origin: origin as u16,
            cutoff: cutoff as u8,
            resonance: resonance as u8,
            modulation_depth: value as u8,
            modulation: modulation as i16,
        };
        let interpolation = EffectInterpolationControl {
            direct_switch: direct,
            enabled_argument: enabled,
        };
        let mut no_room = Queue {
            reject: true,
            ..Default::default()
        };
        let rejection = match phase {
            1 => controller.update_frequency(&mut no_room, &tables, slot as u8, dirty, frequency),
            2 => controller.updates.change_filter_trim(
                &mut no_room,
                &tables,
                origin as u16,
                resonance as u8,
                value as u8,
            ),
            3 => controller.updates.change_filter_response(
                &mut no_room,
                &tables,
                origin as u16,
                value as u8,
                interpolation,
            ),
            _ => unreachable!(),
        };
        if rejection.is_err()
            && controller.updates.assignments == before
            && controller.caches == old_caches
            && no_room.batch.is_none()
        {
            rejected += 1;
        }
        let mut queue = Queue::default();
        match phase {
            1 => controller.update_frequency(&mut queue, &tables, slot as u8, dirty, frequency),
            2 => controller.updates.change_filter_trim(
                &mut queue,
                &tables,
                origin as u16,
                resonance as u8,
                value as u8,
            ),
            3 => controller.updates.change_filter_response(
                &mut queue,
                &tables,
                origin as u16,
                value as u8,
                interpolation,
            ),
            _ => unreachable!(),
        }
        .map_err(|_| "Native filter rejected original input")?;
        let batch = queue.batch.ok_or("Filter batch absent")?;
        let mut before_matches = true;
        let mut after_matches = true;
        if phase == 1 {
            before_matches =
                [old_caches[slot as usize].frequency, dirty] == words[cursor..cursor + 2];
            cursor += 2;
            let after = controller.caches[slot as usize];
            after_matches = [after.frequency, after.dirty] == words[cursor..cursor + 2];
            cursor += 2;
        } else if phase == 3 {
            before_matches = state_words(&before) == words[cursor..cursor + 63];
            cursor += 63;
            after_matches =
                state_words(&controller.updates.assignments) == words[cursor..cursor + 63];
            cursor += 63;
        }
        let n = words[cursor] as usize;
        cursor += 1;
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
                first = json!({"phase":phase,"sequence":sequence,"step":step,"cutoff":cutoff,"resonance":resonance,"value":value,"modulation":modulation,"before":before_matches,"after":after_matches,"native_queue":native_queue,"original_queue":original_queue});
            }
        }
        if phase == 1 && n == 0 {
            skipped += 1;
        }
        host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
        packets += port.packets.len();
        counts[phase as usize - 1] += 1;
        expected_step += 1;
    }
    let total = counts.iter().sum::<usize>();
    let passed = counts == [524288, 65536, 2048]
        && sequences == [16, 4, 16]
        && errors == 0
        && rejected == total
        && expected_step == 128;
    let report = json!({"passed":passed,"whole_original_frequency_cache_calls":counts[0],"whole_original_trim_calls":counts[1],"whole_original_response_calls":counts[2],"errors":errors,"first_difference":first,
        "continuous_sequences":sequences,"cached_no_write_calls":skipped,"all_nine_frequency_cache_slots":true,"full_queue_atomic_rejections":rejected,"host_words_compared":host_words,"host_packets_compared":packets,
        "source_cache_or_coefficients_replayed_as_inputs":false,"native_executes_firmware_instructions":false,"all_original_callees_execute_without_stubs":true,
        "complete_St_Filter_parameter_dispatch_or_FXD03_sound_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/filter-effect-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native St.Filter coefficient core: {total} original calls, {errors} differences, {skipped} cached skips"
    );
    if !passed {
        return Err("Native filter coefficient compiler differs".into());
    }
    Ok(())
}
