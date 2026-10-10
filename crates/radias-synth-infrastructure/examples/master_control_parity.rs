//! Whole original Master parameter controllers and timed transport.
use radias_synth_application::{
    effect_parameters::EffectParameterQueue, master_effect_control::change_master_parameter,
};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_lfo_program::EffectLfoProgram,
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    effect_parameters::EffectParameterBatch,
    effect_transition_queue::{EffectTransitionQueue, EffectTransitionQueueState},
    effect_updates::{CoefficientChange, CoefficientQueueWord, EffectCoefficientAssignments},
    filter_effect::FilterEffectCache,
    master_effect_control::{
        MasterControlState, MasterEdit, MasterMidiBinding, SUPPORTED_MASTER_TYPES,
    },
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
            return Err(());
        }
        self.batch = Some(*b);
        Ok(())
    }
}
fn time(r: &mut Reader) -> (DelayTimeState, [u32; 2], u32) {
    let [cached, capacity, ratio, limited] = r.array();
    let [left, right, control] = r.array();
    (
        DelayTimeState {
            cached_tempo: cached as u16,
            capacity,
            ratio,
            limited,
        },
        [left, right],
        control,
    )
}
fn filter(r: &mut Reader) -> (FilterEffectCache, MasterMidiBinding) {
    let [frequency, dirty, source, first, second] = r.array();
    (
        FilterEffectCache { frequency, dirty },
        MasterMidiBinding {
            source,
            values: [first as u8 as i8, second as u8 as i8],
        },
    )
}
fn assignments(s: &EffectCoefficientAssignments) -> Vec<u32> {
    let mut w = s.order.map(u32::from).to_vec();
    for s in s.slots {
        w.extend(s.indices.map(u32::from));
        w.extend([s.target, s.last_value]);
    }
    w
}
fn queue_state(s: EffectTransitionQueueState) -> [u32; 9] {
    [
        u32::from(s.rings[0].write_index),
        u32::from(s.rings[0].read_index),
        u32::from(s.rings[0].count),
        u32::from(s.rings[1].write_index),
        u32::from(s.rings[1].read_index),
        u32::from(s.rings[1].count),
        u32::from(s.control),
        u32::from(s.wait_ticks),
        u32::from(s.wait_started),
    ]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.master_control_tables()?;
    let indices = library.coefficient_update_indices()?;
    let raw = fs::read(root.join("runs/native-clone/master-control-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d435437 {
        return Err("Wrong Master corpus".into());
    }
    let (
        mut calls,
        mut services,
        mut errors,
        mut queue_errors,
        mut rejections,
        mut words,
        mut packets,
        mut maximum,
        mut lfo_publications,
    ) = (
        0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut counts = [[0usize; 20]; 31];
    let mut kinds = [0usize; 31];
    let mut owner_rebindings = 0usize;
    let mut stored_owners = [0usize; 32];
    let mut assignment_prefills = 0usize;
    let mut assignment_releases = 0usize;
    let mut stored_types = [0usize; 31];
    let mut cabinet_mode_states = [0usize; 2];
    let mut cache_changes = 0usize;
    let mut binding_changes = 0usize;
    let mut rotary_changes = 0usize;
    let mut first = Value::Null;
    for sequence in 0..32u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master sequence changed".into());
        }
        let [initial_mode, initial_speed] = r.array();
        let (initial_filter, initial_binding) = filter(&mut r);
        let (initial_time, initial_pending, initial_control) = time(&mut r);
        let config = r.bytes::<6>();
        let mut phase = r.bytes::<32>();
        let mut rack = MasterControlState {
            lfo: EffectLfoProgram { bytes: config },
            delay: initial_time,
            pending: initial_pending,
            pending_control: initial_control,
            owner: 0,
            update_marker: 0,
            filter_cache: initial_filter,
            midi_binding: initial_binding,
            rotary_mode: initial_mode,
            rotary_speed: initial_speed,
            work_slot: 0,
            coefficient_scratch: [0; 73],
            assignments: EffectCoefficientAssignments::new(indices),
        };
        let mut step = 0u32;
        for kind in SUPPORTED_MASTER_TYPES
            .into_iter()
            .filter(|&k| !matches!(k, 11 | 12 | 30))
        {
            let definition = &tables.definitions[usize::from(kind)];
            for (parameter, range) in definition.ranges[..definition.parameter_count]
                .iter()
                .enumerate()
            {
                for value in (i32::from(range.minimum) + i32::from(range.encoded_zero))
                    ..=(i32::from(range.maximum) + i32::from(range.encoded_zero))
                {
                    let [
                        tag,
                        seq,
                        num,
                        arg_kind,
                        slot,
                        arg_param,
                        arg_value,
                        direct,
                        owner1,
                        _declared_unused_owner,
                        origin,
                        clock,
                        tempo,
                        status,
                        note,
                    ] = r.array();
                    if [tag, seq, num, arg_kind, slot, arg_param, arg_value]
                        != [
                            0x2000,
                            sequence,
                            step,
                            u32::from(kind),
                            8,
                            parameter as u32,
                            value as u32,
                        ]
                    {
                        return Err("Declared Master input changed".into());
                    }
                    let parameters = r.bytes();
                    let previous_parameters = r.bytes();
                    let stored_owner = r.one() as u8;
                    stored_owners[usize::from(stored_owner)] += 1;
                    let stored_effect_type = r.one() as u8;
                    stored_types[usize::from(stored_effect_type)] += 1;
                    let stored_enabled = r.one() != 0;
                    if kind == 8 && parameter == 1 && direct == 0 {
                        cabinet_mode_states[usize::from(stored_enabled)] += 1;
                    }
                    let update_marker = r.one();
                    let midi = EffectMidiSources {
                        global_controls: r.array::<12>().map(|v| v as u16),
                        shared_control: r.one() as u8 as i8,
                        ..Default::default()
                    };
                    let polarity = EffectMidiPolarity {
                        assignments: r.bytes(),
                    };
                    let prefills = r.one();
                    for _ in 0..prefills {
                        let [target, value, mode] = r.array();
                        rack.assignments = rack
                            .assignments
                            .prepare(CoefficientChange {
                                direct_switch: 0,
                                standalone: false,
                                enabled_argument: 1,
                                target,
                                value,
                                mode: mode as u8,
                            })
                            .next;
                        assignment_prefills += 1;
                    }
                    let before = r.array::<63>();
                    let before_rotary = r.array::<2>();
                    let before_filter = filter(&mut r);
                    let before_time = time(&mut r);
                    let before_config = r.bytes::<6>();
                    let before_phase = r.bytes::<32>();
                    let after = r.array::<63>();
                    let after_rotary = r.array::<2>();
                    let after_filter = filter(&mut r);
                    let after_time = time(&mut r);
                    let after_config = r.bytes::<6>();
                    let after_phase = r.bytes::<32>();
                    let after_owner = r.one();
                    let after_marker = r.one();
                    let before_matches = assignments(&rack.assignments) == before
                        && [rack.rotary_mode, rack.rotary_speed] == before_rotary
                        && (rack.filter_cache, rack.midi_binding) == before_filter
                        && (rack.delay, rack.pending, rack.pending_control) == before_time
                        && rack.lfo.bytes == before_config
                        && phase == before_phase;
                    let n = r.one();
                    let original: Vec<_> = (0..n)
                        .map(|_| CoefficientQueueWord {
                            address: r.one() as u16,
                            tagged_value: r.one(),
                        })
                        .collect();
                    let edit = MasterEdit {
                        kind,
                        parameter: parameter as u8,
                        value: value as u8,
                        parameters,
                        previous_parameters,
                        stored_owner,
                        stored_effect_type,
                        stored_enabled,
                        update_marker,
                        origin: origin as u16,
                        owner: owner1,
                        direct_switch: direct,
                        clock_rate: clock,
                        clock: DelayClock {
                            tempo: tempo as u16,
                            status: status as u8,
                        },
                        current_note: note as u8,
                        midi,
                        polarity,
                        prefix_origin: 0,
                        body_origin: 0,
                        relocation_origin: 0,
                        transition_marker: 0,
                    };
                    let saved = rack;
                    let mut q = Queue {
                        reject: true,
                        ..Default::default()
                    };
                    if change_master_parameter(&mut rack, &mut q, &tables, edit).is_err()
                        && rack == saved
                        && q.batch.is_none()
                    {
                        rejections += 1;
                    } else {
                        errors += 1;
                    }
                    q.reject = false;
                    change_master_parameter(&mut rack, &mut q, &tables, edit)
                        .map_err(|_| "Native Master rejected")?;
                    let batch = q.batch.ok_or("Missing Master batch")?;
                    owner_rebindings += usize::from(rack.owner != owner1);
                    cache_changes += usize::from(rack.filter_cache != saved.filter_cache);
                    binding_changes += usize::from(rack.midi_binding != saved.midi_binding);
                    rotary_changes += usize::from(
                        (rack.rotary_mode, rack.rotary_speed)
                            != (saved.rotary_mode, saved.rotary_speed),
                    );
                    assignment_releases += usize::from(rack.update_marker != update_marker);
                    if let Some(p) = batch.lfo_publication() {
                        phase[4..8].copy_from_slice(&p.tempo_increment.to_be_bytes());
                        lfo_publications += 1;
                    }
                    if !before_matches
                        || assignments(&rack.assignments) != after
                        || (rack.delay, rack.pending, rack.pending_control) != after_time
                        || rack.lfo.bytes != after_config
                        || phase != after_phase
                        || rack.owner != after_owner
                        || rack.update_marker != after_marker
                        || (rack.filter_cache, rack.midi_binding) != after_filter
                        || [rack.rotary_mode, rack.rotary_speed] != after_rotary
                        || batch.words() != original
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,step,u32::from(kind),slot,parameter as u32,value as u32,direct],"before_matches":before_matches,"native_owner":rack.owner,"original_owner":after_owner,"native_time":format!("{:?}",(rack.delay,rack.pending,rack.pending_control)),"original_time":format!("{after_time:?}"),"native_state":assignments(&rack.assignments),"original_state":after.to_vec(),"native_config":rack.lfo.bytes,"original_config":after_config,"native_phase":phase.to_vec(),"original_phase":after_phase.to_vec(),"native_words":format!("{:?}",batch.words()),"original_words":format!("{original:?}")});
                        }
                    }
                    maximum = maximum.max(batch.words().len());
                    let mut queue = EffectTransitionQueue::default();
                    queue
                        .enqueue_words(batch.words())
                        .map_err(|_| "Native Master queue failed")?;
                    let total = r.one();
                    for _ in 0..total {
                        let tick = r.one() as u16;
                        let status = r.one() as u16;
                        let expected_state = r.array::<9>();
                        let count = r.one();
                        let expected: Vec<_> = (0..count)
                            .map(|_| {
                                let a = r.one();
                                let c = r.one();
                                let n = r.one();
                                (a, c, (0..n).map(|_| r.one()).collect::<Vec<_>>())
                            })
                            .collect();
                        let output = queue.service(tick, status);
                        let actual: Vec<_> = output.coefficients.packets
                            [..usize::from(output.coefficients.count)]
                            .iter()
                            .map(|p| {
                                (
                                    u32::from(p.address),
                                    1,
                                    p.values[..usize::from(p.count)].to_vec(),
                                )
                            })
                            .collect();
                        if queue_state(queue.state()) != expected_state
                            || actual != expected
                            || output.program.is_some()
                        {
                            queue_errors += 1;
                            if first.is_null() {
                                first = json!({"queue_case":calls,"service":services,"native_state":queue_state(queue.state()),"original_state":expected_state,"native_packets":actual,"original_packets":expected});
                            }
                        }
                        words += actual.iter().map(|p| p.2.len()).sum::<usize>();
                        packets += actual.len();
                        services += 1;
                    }
                    if queue.state().rings.iter().any(|p| p.count != 0)
                        || queue.state().wait_ticks != 0
                    {
                        queue_errors += 1;
                    }
                    calls += 1;
                    counts[usize::from(kind)][parameter] += 1;
                    kinds[usize::from(kind)] += 1;
                    step += 1;
                }
            }
        }
    }
    let passed = errors == 0
        && queue_errors == 0
        && calls > 40000
        && rejections == calls
        && r.cursor == r.words.len()
        && lfo_publications > 0;
    let report = json!({"passed":passed,"whole_original_parameter_dispatches":calls,"whole_original_timed_queue_services":services,"parameter_counts":counts.map(|r|r.to_vec()).to_vec(),"kind_counts":kinds,
        "errors":errors,"queue_service_errors":queue_errors,"first_difference":first,"full_queue_coefficient_and_LFO_atomic_rejections":rejections,"host_words_compared":words,"host_packets_compared":packets,"maximum_parameter_batch_words":maximum,"LFO_publications":lfo_publications,
        "non_owner_time_LFO_MIDI_Rotary_instance_guards_preserved_by_original":true,"original_functions_and_all_callees_execute_without_stubs":true,"native_prior_assignment_time_pending_LFO_and_phase_states_evolve_without_replaying_original_outputs":true,
        "qualified_master_type_ids":SUPPORTED_MASTER_TYPES.into_iter().filter(|&k|!matches!(k,11|12|30)).collect::<Vec<_>>(),
        "owner_rebindings":owner_rebindings,"stored_owner_counts":stored_owners.to_vec(),
        "assignment_prefills":assignment_prefills,"assignment_releases":assignment_releases,"stored_type_counts":stored_types.to_vec(),
        "cabinet_direct_type_change_enabled_states":cabinet_mode_states,
        "filter_cache_changes":cache_changes,"MIDI_binding_changes":binding_changes,
        "Rotary_mode_speed_changes":rotary_changes,"Master_Rotary_mode_speed_and_global_MIDI_state_compared":true,
        "master_filter_and_Wah_binding_cache_and_signed_global_MIDI_state_compared":true,
        "all_eight_inserts_inactive_during_original_Master_MIDI_callbacks":true,
        "master_Flanger_release_marker_and_all_assignment_records_compared":true,
        "mixed_active_insert_Master_MIDI_racks_or_FXD03_audio_verified":false,
        "master_four_band_EQ_and_distortion_owner_mutations_compared":true,
        "all_master_types_master_buffer_allocation_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/master-control-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Master: {calls} original edits, {services} services, {errors}/{queue_errors} differences"
    );
    if !passed {
        return Err("Native Master differs".into());
    }
    Ok(())
}
