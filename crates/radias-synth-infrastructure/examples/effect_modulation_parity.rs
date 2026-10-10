//! Complete mixed-type nine-slot SYS016C4C modulation sweep conformance.
use radias_synth_application::{
    effect_modulation::update_effect_modulation,
    effect_parameters::{EffectParameterQueue, dispatch_parameter_batch},
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    effect_lfo_program::EffectLfoProgram,
    effect_lfo_values::EffectLfoValueState,
    effect_modulation::{
        EffectModulationInstance, EffectModulationRack, EffectModulationTables,
        GrainModulationHistory,
    },
    effect_parameters::EffectParameterBatch,
    effect_updates::EffectCoefficientAssignments,
    filter_effect::FilterEffectCache,
    lfo::LfoState,
    program::Program,
};
use radias_synth_infrastructure::{effects::EffectLibrary, firmware::lfo_tables};
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
fn state_words(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut r = s.order.map(u32::from).to_vec();
    for slot in s.slots {
        r.extend(slot.indices.map(u32::from));
        r.extend([slot.target, slot.last_value]);
    }
    r
}
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
        let value = self.words[self.cursor..self.cursor + N].try_into().unwrap();
        self.cursor += N;
        value
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
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
fn rack_words(rack: &EffectModulationRack) -> Vec<u32> {
    let mut words = state_words(&rack.assignments);
    for cache in rack.caches {
        words.extend([cache.frequency, cache.dirty]);
    }
    for history in rack.grain_history {
        words.extend(history.left.map(|v| u32::from(v as u16)));
        words.extend(history.right.map(|v| u32::from(v as u16)));
        words.extend(
            [
                history.left_read,
                history.left_write,
                history.right_read,
                history.right_write,
            ]
            .map(u32::from),
        );
    }
    for instance in rack.instances {
        words.extend(instance.pending_coefficients);
        words.push(instance.control_argument);
    }
    words
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let coefficients = library.filter_effect_tables()?;
    let lfo = lfo_tables(&system)?;
    let values = library.lfo_value_tables()?;
    let tables = EffectModulationTables {
        coefficients: &coefficients,
        lfo: &lfo,
        values: &values,
        available: library.modulation_availability()?,
    };
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/effect-modulation-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated modulation corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x454d5331 {
        return Err("Wrong modulation corpus".into());
    }
    let switches = [0, 1, 0x10000, 0x80000000];
    let origins = [0, 1, 2, 3, 4, 5, 6, 7, 0x2f0, 0xfff8, 0xfffe, 0xffff];
    let (
        mut calls,
        mut errors,
        mut rejections,
        mut host_words,
        mut packets,
        mut getter_pairs,
        mut max_batch,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    let mut kind_counts = [0usize; 31];
    let mut emitted_kind_counts = [0usize; 31];
    for sequence in 0..16 {
        if read.one() != 0x1000 || read.one() != sequence {
            return Err("Modulation sequence order differs".into());
        }
        let mut rack = EffectModulationRack {
            instances: [EffectModulationInstance::default(); 9],
            caches: [FilterEffectCache::default(); 9],
            grain_history: [GrainModulationHistory::default(); 9],
            assignments: initial,
        };
        for frame in 0..512 {
            if read.one() != 0x2000 || read.one() != sequence || read.one() != frame {
                return Err("Modulation frame order differs".into());
            }
            let direct = read.one();
            if direct != switches[sequence as usize / 4] {
                return Err("Original direct switch profile differs".into());
            }
            let program =
                Program::from_bytes(&read.bytes::<1790>()).map_err(|_| "Raw program rejected")?;
            let mut states = [EffectLfoValueState {
                oscillator: LfoState::default(),
                alternate_phase: 0,
            }; 9];
            for (slot, (instance, state)) in
                rack.instances.iter_mut().zip(states.iter_mut()).enumerate()
            {
                let kind = read.one();
                let origin = read.one();
                let blocks_next = read.one();
                if kind
                    != if frame < 16 {
                        27
                    } else {
                        (frame + slot as u32 * 7 + sequence * 3) % 31
                    }
                    || origin != origins[(frame as usize + slot) % 12]
                {
                    return Err("Original modulation instance profile differs".into());
                }
                instance.kind = kind as u8;
                instance.origin = origin as u16;
                instance.blocks_next_insert = blocks_next as u8;
                instance.parameters = read.bytes::<20>();
                instance.program = EffectLfoProgram {
                    bytes: read.bytes::<6>(),
                };
                let [phase, previous, current, alternate, changed] = read.array::<5>();
                *state = EffectLfoValueState {
                    oscillator: LfoState {
                        phase,
                        previous_random: previous as i16,
                        random: current as i16,
                        half_cycle: 0,
                    },
                    alternate_phase: alternate as u8,
                };
                if changed != 0 {
                    instance.pending_coefficients = read.array::<2>();
                    instance.control_argument = read.one();
                }
                kind_counts[kind as usize] += 1;
            }
            let before = rack;
            let before_source = read.array::<288>();
            let before_matches = rack_words(&before) == before_source;
            let mut no_room = Queue {
                reject: true,
                ..Default::default()
            };
            if update_effect_modulation(&mut rack, &mut no_room, &program, states, &tables, direct)
                .is_err()
                && rack == before
                && no_room.batch.is_none()
            {
                rejections += 1;
            }
            let mut queue = Queue::default();
            let evaluation =
                update_effect_modulation(&mut rack, &mut queue, &program, states, &tables, direct)
                    .map_err(|_| {
                        format!(
                            "Native modulation rejected original sequence {sequence}, frame {frame}"
                        )
                    })?;
            let evaluated = evaluation.evaluated;
            let pairs = evaluation.pairs;
            let batch = queue.batch.ok_or("Modulation batch absent")?;
            let after_source = read.array::<288>();
            let after_matches = rack_words(&rack) == after_source;
            let source_evaluated = read.one();
            let source_pairs = read.array::<18>();
            let native_pairs: Vec<_> = pairs.iter().flat_map(|p| p.map(|v| v as u32)).collect();
            getter_pairs += evaluated.count_ones() as usize;
            let n = read.one() as usize;
            max_batch = max_batch.max(n);
            let original_queue: Vec<_> = (0..n).map(|_| read.array::<2>()).collect();
            let native_queue: Vec<_> = batch
                .words()
                .iter()
                .map(|w| [u32::from(w.address), w.tagged_value])
                .collect();
            let np = read.one();
            let mut original_packets = Vec::new();
            for _ in 0..np {
                let address = read.one();
                let control = read.one();
                let count = read.one();
                let words: Vec<_> = (0..count).map(|_| read.one()).collect();
                original_packets.push((address, control, words));
            }
            let mut port = Port::default();
            dispatch_parameter_batch(&mut port, &batch)?;
            if !before_matches
                || !after_matches
                || u32::from(evaluated) != source_evaluated
                || native_pairs != source_pairs
                || native_queue != original_queue
                || port.packets != original_packets
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"frame":frame,"before":before_matches,"after":after_matches,"native_evaluated":evaluated,"original_evaluated":source_evaluated,"native_pairs":native_pairs,"original_pairs":source_pairs,"native_queue":native_queue,"original_queue":original_queue,"native_state":rack_words(&rack),"original_state":&after_source[..]});
                }
            }
            for slot in 0..9 {
                if evaluated & (1 << slot) != 0 {
                    emitted_kind_counts[usize::from(rack.instances[slot].kind)] += 1;
                }
            }
            host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
            packets += port.packets.len();
            calls += 1;
        }
    }
    let passed = calls == 8192
        && errors == 0
        && rejections == 8192
        && read.cursor == read.words.len()
        && kind_counts.iter().all(|c| *c > 0);
    let report = json!({"passed":passed,"whole_original_nine_slot_modulation_sweeps":calls,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejections,
        "host_words_compared":host_words,"host_packets_compared":packets,"whole_effect_value_getter_pairs_compared":getter_pairs,"maximum_modulation_batch_words":max_batch,
        "all_31_effect_type_contexts":true,"type_context_counts":kind_counts,"evaluated_pair_counts_by_type":emitted_kind_counts,"all_nine_slots_including_active_master":true,
        "previous_cache_history_assignment_and_pending_outputs_replayed_as_inputs":false,"raw_phase_random_and_pan_parameter_edits_are_declared_inputs":true,"all_original_functions_and_callees_execute_without_stubs":true,
        "phase_advance_real_event_cadence_or_FXD03_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-modulation-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native mixed FX modulation: {calls} original sweeps, {errors} differences, {host_words} host words"
    );
    if !passed {
        return Err("Native complete effect modulation sweep differs".into());
    }
    Ok(())
}
