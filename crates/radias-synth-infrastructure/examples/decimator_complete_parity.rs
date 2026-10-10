//! Continuous full SYS07AC52 effect edits, including LFO construction and rate
//! publication, compared to native domain/application and the live native pool.
use radias_synth_application::{
    decimator_effect::{
        DecimatorLfoEdit, DecimatorParameterRequest, EffectControlPort, EffectParameterQueue,
        dispatch_complete_parameter_batch,
    },
    effects::{EffectProgramPort, EffectUpdateController},
    polyphony::PolyphonicRenderer,
};
use radias_synth_domain::{
    decimator_effect::{DecimatorEffectState, EffectParameterBatch},
    effect_control::{EffectBank, EffectKind},
    effect_lfo_program::{EffectLfoProgram, EffectLfoPublication, EffectLfoSlot},
    effect_updates::EffectCoefficientAssignments,
};
use radias_synth_infrastructure::{effects::EffectLibrary, firmware::lfo_tempo_tables};
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
    publication: Option<EffectLfoPublication>,
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
impl EffectControlPort for Port {
    fn publish_effect_lfo(&mut self, p: EffectLfoPublication) -> Result<(), Self::Error> {
        assert!(self.publication.is_none());
        self.publication = Some(p);
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
    let tables = library.decimator_tables()?;
    let tempo = lfo_tempo_tables(&source)?;
    let mapping = library.lfo_mapping(EffectBank::Insert, EffectKind::new(10).unwrap())?;
    let initial = EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
    let mut controller = EffectUpdateController {
        assignments: initial,
    };
    let mut programs = [EffectLfoProgram::default(); 8];
    let mut phases = [[0u32; 32]; 8];
    let mut pool = PolyphonicRenderer::default();
    pool.enable_tempo_clock(tempo, 1200);
    let raw = fs::read(root.join("runs/native-clone/decimator-complete-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated complete effect reference".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if words[0] != 0x44415031 {
        return Err("Invalid complete effect reference".into());
    }
    let (
        mut cursor,
        mut cases,
        mut errors,
        mut rejected,
        mut lfo_publications,
        mut live_pool_checks,
        mut host_words,
        mut packets,
        mut sequences,
    ) = (
        1usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize,
    );
    let mut seen = BTreeSet::new();
    let mut first = Value::Null;
    let mut by_parameter = [0usize; 14];
    let origins = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0x2ea, 0xfff4, 0xfffe, 0xffff,
    ];
    let switches = [0, 1, 0x10000, 0x80000000];
    let clocks = [
        0,
        1,
        1200 * 7158,
        3000 * 7158,
        0x7fffffff,
        0x80000000,
        0xffffffff,
    ];
    while cursor < words.len() {
        if words[cursor] == 0x1000 {
            let sequence = words[cursor + 1];
            cursor += 2;
            if sequence as usize != sequences {
                return Err("Source sequence order changed".into());
            }
            controller.assignments = initial;
            for slot in 0..8 {
                programs[slot] = EffectLfoProgram {
                    bytes: words[cursor..cursor + 6]
                        .iter()
                        .map(|v| *v as u8)
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap(),
                };
                cursor += 6;
                phases[slot].copy_from_slice(&words[cursor..cursor + 32]);
                cursor += 32;
            }
            sequences += 1;
            continue;
        }
        let [
            tag,
            sequence,
            step,
            parameter,
            slot,
            origin,
            value,
            direct,
            first_owner,
            second_owner,
            clock,
        ]: [u32; 11] = words[cursor..cursor + 11].try_into().unwrap();
        cursor += 11;
        if tag != 0x2000
            || sequence >= 16
            || parameter >= 14
            || slot != (sequence + step) % 8
            || origin != origins[step as usize % 16]
            || direct != switches[sequence as usize / 4]
            || clock != clocks[step as usize % 7]
        {
            return Err("Source complete parameter input profile changed".into());
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
            return Err("Source control assignments changed".into());
        }
        let parameters: [u8; 20] = words[cursor..cursor + 20]
            .iter()
            .map(|v| *v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        cursor += 20;
        let index = slot as usize;
        let request = DecimatorParameterRequest {
            origin: origin as u16,
            parameter: parameter as u8,
            value: value as u8,
            state: DecimatorEffectState {
                pre_lpf: parameters[1],
                stored_sample_rate: parameters[3],
            },
            direct_switch: direct,
            owners: [first_owner, second_owner],
            master: false,
        };
        let edit = DecimatorLfoEdit {
            parameters,
            mapping,
            slot: EffectLfoSlot::new(slot as u8).unwrap(),
            offset_control: 0,
            clock_rate: clock,
        };
        let before = controller.assignments;
        let old_program = programs[index];
        let old_phase = phases[index];
        let mut no_room = Queue {
            reject: true,
            ..Default::default()
        };
        if controller
            .change_decimator_complete(
                &mut no_room,
                &tables,
                request,
                &mut programs[index],
                edit,
                &tempo,
            )
            .is_err()
            && controller.assignments == before
            && programs[index] == old_program
            && no_room.batch.is_none()
        {
            rejected += 1;
        }
        let mut queue = Queue::default();
        controller
            .change_decimator_complete(
                &mut queue,
                &tables,
                request,
                &mut programs[index],
                edit,
                &tempo,
            )
            .map_err(|_| "Complete original effect input rejected")?;
        let batch = queue.batch.ok_or("Complete parameter batch absent")?;
        let before_matches = state_words(&before) == words[cursor..cursor + 63];
        cursor += 63;
        let before_program_matches = old_program.bytes.map(u32::from) == words[cursor..cursor + 6];
        cursor += 6;
        let before_phase_matches = old_phase == words[cursor..cursor + 32];
        cursor += 32;
        let after_matches = state_words(&controller.assignments) == words[cursor..cursor + 63];
        cursor += 63;
        let after_program_matches =
            programs[index].bytes.map(u32::from) == words[cursor..cursor + 6];
        cursor += 6;
        let mut port = Port::default();
        dispatch_complete_parameter_batch(&mut port, &batch)?;
        let publication_matches = port.publication.is_some() == (parameter >= 7);
        if let Some(p) = port.publication {
            for (i, b) in p.tempo_increment.to_be_bytes().into_iter().enumerate() {
                phases[index][4 + i] = u32::from(b);
            }
            let pool_before = if slot < 8 {
                pool.shared_lfo_states(index / 2).unwrap()[index % 2 + 2]
            } else {
                pool.global_lfo_state()
            };
            let old_alt = pool
                .effect_lfo_parameters(slot as u8)
                .unwrap()
                .alternate_phase;
            pool.apply_effect_lfo_publication(p)
                .map_err(|_| "Native pool rejected published LFO")?;
            let pool_after = pool.shared_lfo_states(index / 2).unwrap()[index % 2 + 2];
            let projected = pool.effect_lfo_parameters(slot as u8).unwrap();
            if pool_before != pool_after
                || projected.mode != p.program.bytes[0]
                || projected.frequency != p.program.bytes[2]
                || projected.phase_sync != p.program.bytes[3]
                || projected.beat != p.program.bytes[4]
                || projected.alternate_phase != old_alt
                || pool.effect_lfo_rate(slot as u8) != Some(p.tempo_increment)
            {
                return Err("Native live effect phase publication differs".into());
            }
            live_pool_checks += 1;
            lfo_publications += 1;
        }
        let after_phase_matches = phases[index] == words[cursor..cursor + 32];
        cursor += 32;
        let n = words[cursor] as usize;
        cursor += 1;
        let original_queue: Vec<_> = words[cursor..cursor + 2 * n]
            .chunks_exact(2)
            .map(|b| [b[0], b[1]])
            .collect();
        cursor += 2 * n;
        let candidate_queue: Vec<_> = batch
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
        if !before_matches
            || !after_matches
            || !before_program_matches
            || !after_program_matches
            || !before_phase_matches
            || !after_phase_matches
            || !publication_matches
            || candidate_queue != original_queue
            || port.packets != original_packets
        {
            errors += 1;
            if first.is_null() {
                first = json!({"sequence":sequence,"step":step,"parameter":parameter,"slot":slot,
                "coefficient_before":before_matches,"coefficient_after":after_matches,"LFO_before":before_program_matches,"LFO_after":after_program_matches,
                "phase_before":before_phase_matches,"phase_after":after_phase_matches,"publication":publication_matches,
                "native_LFO":programs[index].bytes,"native_phase":phases[index],"native_queue":candidate_queue,"original_queue":original_queue});
            }
        }
        host_words += port.packets.iter().map(|p| p.2.len()).sum::<usize>();
        packets += port.packets.len();
        cases += 1;
        by_parameter[parameter as usize] += 1;
        seen.insert((sequence, parameter, value));
    }
    let limits = [100, 1, 100, 94, 20, 127, 127, 1, 127, 16, 4, 127, 1, 18];
    let mut expected = BTreeSet::new();
    for sequence in 0..16 {
        for (parameter, limit) in limits.into_iter().enumerate() {
            for value in if matches!(parameter, 6 | 11) { 1 } else { 0 }..=limit {
                expected.insert((sequence, parameter as u32, value));
            }
        }
    }
    let passed = sequences == 16
        && cases == 14000
        && seen == expected
        && errors == 0
        && rejected == 14000
        && lfo_publications == 4800
        && live_pool_checks == 4800;
    let report = json!({"passed":passed,"original_whole_parameter_dispatches":cases,"parameter_counts":by_parameter,"errors":errors,"first_difference":first,
        "continuous_sequences":sequences,"insert_instances":8,"LFO_configuration_and_rate_publications":lfo_publications,
        "live_native_pool_parameter_rate_and_phase_preservation_checks":live_pool_checks,"full_queue_rejections_preserve_coefficients_and_LFO":rejected,
        "host_words_compared":host_words,"host_packets_compared":packets,"all_14_parameter_domains_complete":true,
        "previous_LFO_configuration_and_rate_outputs_replayed_as_inputs":false,"first_LFO_configurations_and_phase_states_are_declared_inputs":true,
        "all_original_dependency_handlers_and_LFO_publication_calls_execute_without_stubs":true,"native_controller_executes_firmware_instructions":false,
        "FXD03_audio_or_complete_master_effect_wrapper_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/decimator-complete-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native complete St.Decimator: {cases} whole original edits, {errors} differences, {live_pool_checks} live LFO publications"
    );
    if !passed {
        return Err("Complete native effect parameter compiler differs".into());
    }
    Ok(())
}
