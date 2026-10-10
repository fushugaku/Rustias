//! Grain time edits followed by whole original nine-slot modulation sweeps.
use radias_synth_application::{
    effect_modulation::update_effect_modulation,
    effect_parameters::{
        EffectControlPort, EffectParameterQueue, dispatch_complete_parameter_batch,
    },
    effects::EffectProgramPort,
    pitch_grain_shifter::change_pitch_grain_parameter,
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_lfo_program::EffectLfoProgram,
    effect_lfo_program::EffectLfoPublication,
    effect_lfo_values::EffectLfoValueState,
    effect_modulation::{
        EffectModulationInstance, EffectModulationRack, EffectModulationTables,
        GrainModulationHistory,
    },
    effect_parameters::EffectParameterBatch,
    effect_updates::EffectCoefficientAssignments,
    filter_effect::FilterEffectCache,
    lfo::LfoState,
    pitch_grain_shifter::{GrainPendingTime, PitchGrainEdit, PitchGrainKind, PitchGrainRack},
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
impl EffectControlPort for Port {
    fn publish_effect_lfo(&mut self, _: EffectLfoPublication) -> Result<(), Self::Error> {
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
fn time(r: &mut Reader) -> (DelayTimeState, GrainPendingTime) {
    let [cached, capacity, ratio, limited, left, right, control] = r.array();
    (
        DelayTimeState {
            cached_tempo: cached as u16,
            capacity,
            ratio,
            limited,
        },
        GrainPendingTime {
            coefficients: [left, right],
            control_argument: control,
        },
    )
}
#[derive(Default)]
struct Transport {
    services: usize,
    words: usize,
    packets: usize,
    errors: usize,
}
impl Transport {
    fn compare(
        &mut self,
        r: &mut Reader,
        b: &EffectParameterBatch,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let n = r.one();
        let expected: Vec<_> = (0..n).map(|_| r.array::<2>()).collect();
        let actual: Vec<_> = b
            .words()
            .iter()
            .map(|w| [u32::from(w.address), w.tagged_value])
            .collect();
        self.services += r.one() as usize;
        let count = r.one();
        let mut expected_packets = Vec::new();
        for _ in 0..count {
            let a = r.one();
            let c = r.one();
            let n = r.one();
            expected_packets.push((a, c, (0..n).map(|_| r.one()).collect::<Vec<_>>()));
        }
        let mut port = Port::default();
        dispatch_complete_parameter_batch(&mut port, b)?;
        let matched = actual == expected && port.packets == expected_packets;
        self.errors += usize::from(!matched);
        self.words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
        self.packets += port.packets.len();
        Ok(matched)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let parameters = library.pitch_grain_tables()?;
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
    let raw = fs::read(root.join("runs/native-clone/grain-pending-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Grain integration corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x47504931 {
        return Err("Wrong Grain integration corpus".into());
    }
    let (mut edits, mut sweeps, mut errors, mut rejections, mut consumed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut transport = Transport::default();
    let mut first = Value::Null;
    let mut slots = [0usize; 8];
    let mut retained = Vec::new();
    for profile in 0..32u32 {
        for parameter in 1..=7u32 {
            let [
                tag,
                pf,
                arg_parameter,
                slot,
                origin,
                direct,
                clock,
                tempo,
                status,
                value,
            ] = read.array();
            if [tag, pf, arg_parameter, slot] != [0x1000, profile, parameter, profile % 8] {
                return Err("Grain integration profile changed".into());
            }
            let program =
                Program::from_bytes(&read.bytes::<1790>()).map_err(|_| "Program rejected")?;
            let snapshot = read.bytes::<20>();
            let (initial_time, initial_pending) = time(&mut read);
            let config = read.bytes::<6>();
            let mut phase = read.bytes::<32>();
            let mut controller = PitchGrainRack {
                times: [DelayTimeState::default(); 8],
                pending: [GrainPendingTime::default(); 8],
                lfos: [EffectLfoProgram::default(); 8],
                assignments: initial,
            };
            controller.times[slot as usize] = initial_time;
            controller.pending[slot as usize] = initial_pending;
            controller.lfos[slot as usize].bytes = config;
            let edit = PitchGrainEdit {
                kind: PitchGrainKind::Grain,
                slot: slot as u8,
                parameter: parameter as u8,
                value: value as u8,
                parameters: snapshot,
                origin: origin as u16,
                owners: [parameter, parameter],
                direct_switch: direct,
                clock: DelayClock {
                    tempo: tempo as u16,
                    status: status as u8,
                },
                clock_rate: clock,
            };
            let saved = controller;
            let mut q = Queue {
                reject: true,
                ..Default::default()
            };
            if change_pitch_grain_parameter(&mut controller, &mut q, &parameters, edit).is_err()
                && controller == saved
                && q.batch.is_none()
            {
                rejections += 1;
            } else {
                errors += 1;
            }
            q.reject = false;
            change_pitch_grain_parameter(&mut controller, &mut q, &parameters, edit)
                .map_err(|_| "Grain edit rejected")?;
            let batch = q.batch.ok_or("Grain edit batch missing")?;
            if let Some(p) = batch.lfo_publication() {
                phase[4..8].copy_from_slice(&p.tempo_increment.to_be_bytes());
            }
            let expected_time = time(&mut read);
            let expected_config = read.bytes::<6>();
            let expected_phase = read.bytes::<32>();
            let mut rack = EffectModulationRack {
                instances: [EffectModulationInstance::default(); 9],
                caches: [FilterEffectCache::default(); 9],
                grain_history: [GrainModulationHistory::default(); 9],
                assignments: controller.assignments,
            };
            rack.instances[slot as usize] = EffectModulationInstance {
                kind: 27,
                parameters: snapshot,
                origin: origin as u16,
                program: controller.lfos[slot as usize],
                blocks_next_insert: 0,
                pending_coefficients: controller.pending[slot as usize].coefficients,
                control_argument: controller.pending[slot as usize].control_argument,
            };
            let expected_state = read.array::<288>();
            let matched = (
                controller.times[slot as usize],
                controller.pending[slot as usize],
            ) == expected_time
                && controller.lfos[slot as usize].bytes == expected_config
                && phase == expected_phase
                && rack_words(&rack) == expected_state;
            let host_matches = transport.compare(&mut read, &batch)?;
            if !matched || !host_matches {
                errors += 1;
                if first.is_null() {
                    first = json!({"profile":profile,"parameter":parameter,"edit_state_matches":matched,"edit_transport_matches":host_matches});
                }
            }
            edits += 1;
            slots[slot as usize] += 1;
            for frame in 0..16u32 {
                if read.array::<2>() != [0x2000, frame] {
                    return Err("Grain integration frame changed".into());
                }
                let states = core::array::from_fn(|_| {
                    let [phase, previous, current, alternate] = read.array();
                    EffectLfoValueState {
                        oscillator: LfoState {
                            phase,
                            previous_random: previous as i16,
                            random: current as i16,
                            half_cycle: 0,
                        },
                        alternate_phase: alternate as u8,
                    }
                });
                let before = read.array::<288>();
                let before_matches = rack_words(&rack) == before;
                let saved = rack;
                let mut q = Queue {
                    reject: true,
                    ..Default::default()
                };
                if update_effect_modulation(&mut rack, &mut q, &program, states, &tables, direct)
                    .is_err()
                    && rack == saved
                    && q.batch.is_none()
                {
                    rejections += 1;
                } else {
                    errors += 1;
                }
                q.reject = false;
                update_effect_modulation(&mut rack, &mut q, &program, states, &tables, direct)
                    .map_err(|_| "Grain modulation rejected")?;
                let batch = q.batch.ok_or("Grain modulation batch missing")?;
                let after = read.array::<288>();
                let after_matches = rack_words(&rack) == after;
                let host_matches = transport.compare(&mut read, &batch)?;
                for (a, b) in saved.instances[slot as usize]
                    .pending_coefficients
                    .into_iter()
                    .zip(rack.instances[slot as usize].pending_coefficients)
                {
                    if a & 0x80000000 != 0 && b & 0x80000000 == 0 {
                        consumed += 1;
                    }
                }
                if !before_matches || !after_matches || !host_matches {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"profile":profile,"parameter":parameter,"frame":frame,"before_matches":before_matches,"after_matches":after_matches,"transport_matches":host_matches,"native_state":rack_words(&rack),"original_state":after.to_vec()});
                    }
                }
                sweeps += 1;
            }
            for (channel, value) in rack.instances[slot as usize]
                .pending_coefficients
                .iter()
                .enumerate()
            {
                if value & 0x80000000 != 0 {
                    retained.push([profile, parameter, channel as u32]);
                }
            }
        }
    }
    let passed = errors == 0
        && transport.errors == 0
        && edits == 224
        && sweeps == 3584
        && consumed == 444
        && retained.len() == 4
        && rejections == edits + sweeps
        && read.cursor == read.words.len();
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":edits,"whole_original_nine_slot_modulation_sweeps":sweeps,"whole_original_queue_service_calls":transport.services,"errors":errors,"transport_errors":transport.errors,"first_difference":first,"full_queue_atomic_rejections":rejections,"consumed_pending_time_words":consumed,"retained_pending_time_words":retained.len(),"retained_time_contexts":retained,"slot_counts":slots,"host_words_compared":transport.words,"host_packets_compared":transport.packets,"prior_controller_history_assignment_and_pending_outputs_replayed_as_inputs":false,"phase_random_values_are_declared_inputs_not_observed_callback_values":true,"original_functions_and_all_callees_execute_without_stubs":true,"physical_modulation_cadence_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/grain-pending-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Grain time integration: {edits} original edits, {sweeps} sweeps, {consumed} consumed pending words, {errors} differences"
    );
    if !passed {
        return Err("Grain time integration differs".into());
    }
    Ok(())
}
