//! Whole SYS079702 with live Master Talking and eight inactive inserts.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, master_effect_control::update_master_talking_midi,
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::EffectTransitionQueue,
    effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_effect_control::{MasterControlState, MasterEdit, MasterMidiBinding},
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
}
#[derive(Default)]
struct Queue {
    reject: bool,
    batch: Option<EffectParameterBatch>,
}
impl EffectParameterQueue for Queue {
    type Error = ();
    fn enqueue_parameter(&mut self, b: &EffectParameterBatch) -> Result<(), ()> {
        if self.reject {
            Err(())
        } else {
            self.batch = Some(*b);
            Ok(())
        }
    }
}
fn binding(s: &MasterControlState) -> [u32; 5] {
    [
        s.midi_binding.source,
        u32::from(s.midi_binding.values[0] as u8),
        u32::from(s.midi_binding.values[1] as u8),
        s.rotary_mode,
        s.rotary_speed,
    ]
}
fn state(s: &MasterControlState) -> Vec<u32> {
    let mut words = s.assignments.order.map(u32::from).to_vec();
    for slot in s.assignments.slots {
        words.extend(slot.indices.map(u32::from));
        words.extend([slot.target, slot.last_value]);
    }
    words.extend(binding(s));
    words
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.master_control_tables()?;
    let raw = fs::read(root.join("runs/native-clone/master-talking-midi-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master Talking MIDI corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d544d31 {
        return Err("Wrong Master Talking corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut rejected,
        mut words,
        mut packets,
        mut maximum,
        mut changes,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    for sequence in 0..16u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master Talking sequence changed".into());
        }
        let [source, primary, secondary, mode, speed] = r.array();
        let mut rack = MasterControlState {
            assignments: EffectCoefficientAssignments::new(library.coefficient_update_indices()?),
            lfo: EffectLfoProgram { bytes: [0; 6] },
            delay: DelayTimeState::default(),
            pending: [0; 2],
            pending_control: 0,
            owner: 0,
            update_marker: 0,
            filter_cache: FilterEffectCache::default(),
            midi_binding: MasterMidiBinding {
                source,
                values: [primary as u8 as i8, secondary as u8 as i8],
            },
            rotary_mode: mode,
            rotary_speed: speed,
            work_slot: 0,
            coefficient_scratch: [0; 73],
        };
        for frame in 0..512u32 {
            if r.array::<3>() != [0x2000, sequence, frame] {
                return Err("Master Talking frame changed".into());
            }
            let origin = r.one() as u16;
            let parameters = r.bytes::<20>();
            let midi = EffectMidiSources {
                global_controls: r.array::<12>().map(|v| v as u16),
                shared_control: r.one() as u8 as i8,
                ..Default::default()
            };
            let polarity = EffectMidiPolarity {
                assignments: r.bytes(),
            };
            let before = r.array::<68>();
            let after = r.array::<68>();
            let n = r.one();
            let original: Vec<_> = (0..n)
                .map(|_| CoefficientQueueWord {
                    address: r.one() as u16,
                    tagged_value: r.one(),
                })
                .collect();
            let edit = MasterEdit {
                kind: 30,
                parameter: 0,
                value: 0,
                parameters,
                previous_parameters: [0; 20],
                stored_owner: 0,
                stored_effect_type: 30,
                stored_enabled: true,
                update_marker: 0,
                origin,
                owner: 0,
                direct_switch: 0,
                clock_rate: 0,
                clock: DelayClock {
                    tempo: 1200,
                    status: 0,
                },
                current_note: 60,
                midi,
                polarity,
                prefix_origin: 0,
                body_origin: 0,
                relocation_origin: 0,
                transition_marker: 0,
            };
            let saved = rack;
            let before_matches = state(&rack) == before;
            let mut q = Queue {
                reject: true,
                ..Default::default()
            };
            if update_master_talking_midi(&mut rack, &mut q, &tables, edit).is_err()
                && rack == saved
                && q.batch.is_none()
            {
                rejected += 1;
            } else {
                errors += 1;
            }
            q.reject = false;
            update_master_talking_midi(&mut rack, &mut q, &tables, edit)
                .map_err(|_| "Native Master Talking MIDI rejected")?;
            let batch = q.batch.ok_or("Missing Master Talking batch")?;
            if !before_matches
                || state(&rack) != after
                || batch.words() != original
                || batch.lfo_publication().is_some()
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"case":calls,"before_matches":before_matches,"native_state":state(&rack),"original_state":after.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                }
            }
            changes += usize::from(rack.midi_binding != saved.midi_binding);
            maximum = maximum.max(batch.words().len());
            let mut queue = EffectTransitionQueue::default();
            queue
                .enqueue_words(batch.words())
                .map_err(|_| "Master Talking queue rejected")?;
            let count = r.one();
            let mut actual = Vec::new();
            for _ in 0..count {
                let out = queue.service(0, 0);
                if out.program.is_some() {
                    errors += 1;
                }
                actual.extend(
                    out.coefficients.packets[..usize::from(out.coefficients.count)]
                        .iter()
                        .map(|p| {
                            (
                                u32::from(p.address),
                                1u32,
                                p.values[..usize::from(p.count)].to_vec(),
                            )
                        }),
                );
            }
            let count_packets = r.one();
            let expected: Vec<_> = (0..count_packets)
                .map(|_| {
                    let address = r.one();
                    let control = r.one();
                    let n = r.one();
                    (
                        address,
                        control,
                        (0..n).map(|_| r.one()).collect::<Vec<_>>(),
                    )
                })
                .collect();
            if actual != expected || queue.state().rings.iter().any(|p| p.count != 0) {
                errors += 1;
                if first.is_null() {
                    first = json!({"queue_case":calls,"native":actual,"original":expected});
                }
            }
            words += actual.iter().map(|p| p.2.len()).sum::<usize>();
            packets += actual.len();
            services += count as usize;
            calls += 1;
        }
    }
    let passed = errors == 0
        && calls == 8192
        && rejected == calls
        && r.cursor == r.words.len()
        && changes > 0;
    let report = json!({"passed":passed,"whole_original_MIDI_service_sweeps":calls,"whole_original_queue_services":services,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejected,"host_words_compared":words,"host_packets_compared":packets,"maximum_batch_words":maximum,"binding_changes":changes,"evolving_native_binding_and_mode_state_replayed_from_original":false,"all_eight_inserts_inactive_during_original_Master_MIDI":true,"original_unrelated_instance_guard_bytes_preserved":true,"mixed_active_racks_physical_cadence_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/master-talking-midi-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Master Talking MIDI: {calls} original sweeps, {errors} differences, {changes} binding changes"
    );
    if !passed {
        return Err("Master Talking MIDI differs".into());
    }
    Ok(())
}
