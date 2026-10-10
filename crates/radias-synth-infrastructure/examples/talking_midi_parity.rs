//! Whole original SYS079702 live MIDI sweeps in an eight-Talking rack.
use radias_synth_application::talking_effect::{TalkingEffectPort, update_talking_midi};
use radias_synth_domain::{
    effect_midi::{EffectMidiPolarity, EffectMidiSources, EffectMidiTimbre},
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectProgramStaging,
    effect_transition_queue::EffectTransitionQueue,
    effect_updates::EffectCoefficientAssignments,
    talking_effect::{PreparedTalkingEdit, TalkingInstance, TalkingRack},
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
#[derive(Default)]
struct Queue {
    batch: Option<EffectParameterBatch>,
    reject: bool,
}
impl TalkingEffectPort for Queue {
    type Error = ();
    fn accept_talking_edit(&mut self, p: &PreparedTalkingEdit) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        assert!(p.program_writes.iter().all(Option::is_none) && p.body_program.is_none());
        self.batch = Some(p.batch);
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
fn bindings(rack: &TalkingRack) -> Vec<u32> {
    rack.instances
        .iter()
        .flat_map(|i| {
            [
                i.controller_source,
                i32::from(i.controller_value) as u32,
                i32::from(i.secondary_value) as u32,
            ]
        })
        .collect()
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
fn rack_words(r: &TalkingRack) -> Vec<u32> {
    let mut w = state_words(&r.assignments);
    w.extend(bindings(r));
    w
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tables = library.talking_tables()?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let raw = fs::read(root.join("runs/native-clone/talking-midi-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Talking MIDI corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x544d4931 {
        return Err("Wrong Talking MIDI corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut rejections,
        mut host_words,
        mut packets,
        mut largest,
        mut switches,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut first = Value::Null;
    for sequence in 0..16u32 {
        if read.array::<2>() != [0x1000, sequence] {
            return Err("Talking MIDI sequence changed".into());
        }
        let mut rack = TalkingRack {
            instances: [TalkingInstance::default(); 8],
            assignments: initial,
            staging: EffectProgramStaging::default(),
        };
        for instance in &mut rack.instances {
            let [source, primary, secondary] = read.array();
            instance.kind = 30;
            instance.controller_source = source;
            instance.controller_value = primary as i8;
            instance.secondary_value = secondary as i8;
        }
        for frame in 0..512u32 {
            if read.array::<3>() != [0x2000, sequence, frame] {
                return Err("Talking MIDI frame changed".into());
            }
            let polarity = EffectMidiPolarity {
                assignments: read.bytes::<5>(),
            };
            let mut midi = EffectMidiSources::default();
            for t in &mut midi.timbres {
                let f = read.array::<10>();
                *t = EffectMidiTimbre {
                    control_49: f[0] as i8,
                    bend: f[1] as i16,
                    control_4b: f[2] as i8,
                    channel: f[3] as u8,
                    switch_45: f[4] as u8,
                    controls_4c_50: core::array::from_fn(|i| f[i + 5] as i8),
                };
            }
            for g in &mut midi.channel_controls {
                *g = read.bytes::<16>();
            }
            midi.shared_control = read.one() as i8;
            for i in &mut rack.instances {
                i.parameters = read.bytes::<20>();
                i.origin = read.one() as u16;
            }
            let before = read.array::<87>();
            let before_matches = rack_words(&rack) == before;
            let saved = rack;
            let mut q = Queue {
                reject: true,
                ..Default::default()
            };
            if update_talking_midi(&mut q, &mut rack, &tables, &midi, polarity).is_err()
                && rack == saved
                && q.batch.is_none()
            {
                rejections += 1;
            } else {
                errors += 1;
            }
            q.reject = false;
            update_talking_midi(&mut q, &mut rack, &tables, &midi, polarity)
                .map_err(|_| "Talking MIDI rejected")?;
            let batch = q.batch.ok_or("Talking MIDI batch missing")?;
            let after = read.array::<87>();
            let after_matches = rack_words(&rack) == after;
            let count = read.one();
            let expected: Vec<_> = (0..count).map(|_| read.array::<2>()).collect();
            let actual: Vec<_> = batch
                .words()
                .iter()
                .map(|w| [u32::from(w.address), w.tagged_value])
                .collect();
            largest = largest.max(actual.len());
            let ns = read.one();
            services += ns as usize;
            let np = read.one();
            let expected_packets: Vec<_> = (0..np)
                .map(|_| {
                    let a = read.one();
                    let c = read.one();
                    let n = read.one();
                    (a, c, (0..n).map(|_| read.one()).collect::<Vec<_>>())
                })
                .collect();
            let mut queue = EffectTransitionQueue::default();
            queue
                .enqueue_words(batch.words())
                .map_err(|_| "Talking MIDI transport rejected")?;
            let mut actual_packets = Vec::new();
            for _ in 0..ns {
                let result = queue.service(0, 0);
                if result.program.is_some() {
                    errors += 1;
                }
                for p in &result.coefficients.packets[..usize::from(result.coefficients.count)] {
                    actual_packets.push((
                        u32::from(p.address),
                        1,
                        p.values[..usize::from(p.count)].to_vec(),
                    ));
                }
            }
            if !before_matches
                || !after_matches
                || actual != expected
                || actual_packets != expected_packets
                || queue.state().rings.iter().any(|q| q.count != 0)
            {
                errors += 1;
                if first.is_null() {
                    first = json!({"sequence":sequence,"frame":frame,"before_matches":before_matches,"after_matches":after_matches,"native_state":rack_words(&rack),"original_state":after.to_vec(),"native_words":actual,"original_words":expected,"native_packets":actual_packets,"original_packets":expected_packets});
                }
            }
            for (a, b) in saved.instances.iter().zip(rack.instances) {
                switches += usize::from(a.controller_value != b.controller_value);
            }
            host_words += actual_packets.iter().map(|p| p.2.len()).sum::<usize>();
            packets += actual_packets.len();
            calls += 1;
        }
    }
    let passed = calls == 8192
        && errors == 0
        && rejections == calls
        && switches > 0
        && read.cursor == read.words.len();
    let report = json!({"passed":passed,"whole_original_eight_insert_and_master_MIDI_service_sweeps":calls,"whole_original_queue_services":services,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"host_words_compared":host_words,"host_packets_compared":packets,"maximum_batch_words":largest,"controller_value_changes":switches,"raw_MIDI_controls_and_parameter_edits_are_declared_inputs":true,"previous_binding_or_coefficients_replayed_from_original":false,"non_controller_instance_guards_preserved_by_original":true,"all_original_callees_execute_without_stubs":true,"all_eight_instances_are_Talking_and_master_is_empty":true,"mixed_types_master_or_physical_cadence_and_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/talking-midi-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native live Talking MIDI: {calls} original whole sweeps, {errors} differences, {switches} switches"
    );
    if !passed {
        return Err("Talking MIDI differs".into());
    }
    Ok(())
}
