//! Original complete per-note initialization procedures and physical scale ROM.
use radias_synth_application::actor_note_initialization::{
    ActorNoteInitializationRequest, initialize_actor_note,
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_note_initialization::{
        ActorNoteInitializationPorts, ActorNoteInitializationTables, ActorPortamentoInitialization,
    },
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::ActorVirtualPatchPorts,
    raw_note_scale::RawNoteScaleContext,
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let pitch = firmware::note_pitch_tables(&sys)?;
    let scale_tables = firmware::raw_note_scale_tables(&sys)?;
    let rates = firmware::portamento_rates(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let modulation = firmware::modulation_tables(&sys)?;
    let tables = ActorNoteInitializationTables {
        pitch: &pitch,
        scale: &scale_tables,
        portamento: &rates,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let raw = fs::read(out.join("actor-note-initialization-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x414e4931 {
        return Err("Unsupported actor note initialization observation".into());
    }
    let (
        mut calls,
        mut state_errors,
        mut seed_errors,
        mut value_errors,
        mut clock_errors,
        mut accumulator_errors,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let mut coverage = [0u32; 7];
    let mut selections = [[false; 256]; 7];
    let mut enabled_transposes = [[false; 256]; 7];
    let mut first_state_error = None;
    let mut first_clock_error = None;
    let mut physical_outside_musical_table = 0u32;
    let mut portamento_branches = [0u32; 4];
    let mut sources_destinations = [[false; 40]; 16];
    let mut atomic_rejections = 0;
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor) as usize;
        let variant = take(&raw, &mut cursor);
        let mut seed = take(&raw, &mut cursor) as u16;
        let master_tune = take(&raw, &mut cursor) as i32;
        let common_receive_flags = take(&raw, &mut cursor) as u8;
        let owner_flags = take(&raw, &mut cursor) as u8;
        let wheel = take(&raw, &mut cursor) as u8;
        let bend = take(&raw, &mut cursor) as i16;
        let bend_q16 = take(&raw, &mut cursor) as i32;
        let timbre = take(&raw, &mut cursor) as u8;
        let midi_mode = take(&raw, &mut cursor) as u8;
        let time = take(&raw, &mut cursor) as u8;
        let switch = take(&raw, &mut cursor) != 0;
        let context_flags = take(&raw, &mut cursor) as u8;
        let gate_flags = take(&raw, &mut cursor) as u8;
        let context_pitch_q16 = take(&raw, &mut cursor) as i32;
        let selection = take(&raw, &mut cursor) as u8;
        let transpose_enabled = take(&raw, &mut cursor) != 0;
        let transpose = take(&raw, &mut cursor) as i8;
        if transpose_enabled {
            enabled_transposes[service][transpose as u8 as usize] = true;
        }
        let scale_note = take(&raw, &mut cursor) as u8;
        let auxiliary = take(&raw, &mut cursor) as i16;
        let mut assignments = [0; 5];
        let mut assignable_values = [0; 5];
        for i in 0..5 {
            assignments[i] = take(&raw, &mut cursor) as u8;
            assignable_values[i] = take(&raw, &mut cursor) as i8;
        }
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let mut controller = ActorControlState {
            bytes: raw[cursor..cursor + 496].try_into()?,
        };
        cursor += 496;
        let custom_cents: [i8; 256] = core::array::from_fn(|i| raw[cursor + i] as i8);
        cursor += 256;
        let expected_clocks = take(&raw, &mut cursor);
        let expected_value = take(&raw, &mut cursor) as i32;
        let expected_seed = take(&raw, &mut cursor) as u16;
        let expected = &raw[cursor..cursor + 496];
        cursor += 496;
        let expected_targets = if service == 6 {
            Some(core::array::from_fn::<i32, 40, _>(|_| {
                take(&raw, &mut cursor) as i32
            }))
        } else {
            None
        };
        let expected_linked = if service == 6 {
            take(&raw, &mut cursor) as i32
        } else {
            0
        };
        let scale = RawNoteScaleContext {
            selection,
            global_transpose: transpose_enabled.then_some(transpose),
            custom_cents,
        };
        let pitch_ports = ActorPitchPorts {
            timbre,
            midi_mode,
            bend_q16,
            wheel,
            common_receive_flags,
        };
        let portamento = ActorPortamentoInitialization {
            time,
            switch_required: common_receive_flags & 8 != 0,
            switch,
            context_flags,
            gate_flags,
            context_pitch_q16,
        };
        let ports = ActorNoteInitializationPorts {
            master_tune,
            pitch: pitch_ports,
            scale,
            portamento,
            sources: ActorVirtualPatchPorts {
                bend,
                wheel,
                auxiliary,
                midi_receive_flags: owner_flags,
                assignments,
                assignable_values,
            },
        };
        let work = match service {
            0 => controller.prepare_tuning(&body, &pitch, master_tune),
            1 => controller.prepare_random_offsets(&body, master_tune, pitch_ports, &pitch),
            2 => controller.prepare_transposed_note(&body, scale, &scale_tables, &mut seed),
            3 => {
                let clocks = controller.initialize_portamento(portamento, &rates);
                let rate = u32::from_be_bytes(controller.bytes[0x84..0x88].try_into()?);
                let branch = if rate == 0 {
                    0
                } else if context_flags & 128 != 0 {
                    1
                } else if gate_flags & 1 != 0 {
                    2
                } else {
                    3
                };
                portamento_branches[branch] += 1;
                clocks
            }
            4 => controller.prepare_secondary_pitch(&body, &pitch.vibrato),
            5 => {
                let index = i32::from(scale_note) + 12
                    - i32::from(scale.global_transpose.unwrap_or(0))
                    - i32::from(selection >> 4);
                if (1..=9).contains(&(selection & 15)) && !(0..144).contains(&index) {
                    physical_outside_musical_table += 1;
                }
                let (value, clocks) = scale_tables.offset(scale_note, scale, &mut seed);
                if value != expected_value {
                    value_errors += 1;
                    first_state_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"native_scale":value,"original_scale":expected_value,"index":index}));
                }
                clocks
            }
            6 => {
                if variant == 0 {
                    let mut bad_body = body;
                    bad_body[86..89].copy_from_slice(&[5, 63, 127]);
                    let mut bad = controller;
                    bad.bytes[0x37] = 100;
                    bad.bytes[0x1c4] = 0;
                    bad.bytes[0x158..0x15a].fill(0);
                    let saved = bad;
                    let mut bad_seed = seed;
                    let rejected = initialize_actor_note(
                        ActorNoteInitializationRequest {
                            body: &bad_body,
                            ports,
                        },
                        &mut bad,
                        &mut bad_seed,
                        &tables,
                    );
                    if !matches!(rejected, Err(radias_synth_domain::actor_virtual_patch::ActorVirtualPatchError::InvalidDestination(63)))
                        || bad != saved || bad_seed != seed {
                        return Err("Per-note failure partially committed controller or shared PRNG".into());
                    }
                    atomic_rejections += 1;
                }
                let initialized = initialize_actor_note(
                    ActorNoteInitializationRequest { body: &body, ports },
                    &mut controller,
                    &mut seed,
                    &tables,
                )
                .map_err(|e| format!("{e:?}"))?;
                if Some(initialized.modulations.values) != expected_targets
                    || initialized.modulations.linked_pitch != expected_linked
                {
                    accumulator_errors += 1;
                }
                for route in 0..6 {
                    sources_destinations[(body[86 + 3 * route] & 15) as usize]
                        [(body[87 + 3 * route] & 63) as usize] = true;
                }
                initialized.controller_clocks
            }
            _ => return Err("Invalid note procedure".into()),
        };
        if controller.bytes != expected {
            state_errors += 1;
            let offset = controller
                .bytes
                .iter()
                .zip(expected)
                .position(|(a, b)| a != b)
                .unwrap();
            first_state_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"offset":offset,"native":controller.bytes[offset],"original":expected[offset]}));
        }
        if seed != expected_seed {
            seed_errors += 1;
            first_state_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"native_seed":seed,"original_seed":expected_seed}));
        }
        if u32::from(work) != expected_clocks {
            clock_errors += 1;
            first_clock_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"native":work,"original":expected_clocks,"scale":selection&15}));
        }
        coverage[service] += 1;
        selections[service][selection as usize] = true;
        calls += 1;
    }
    let passed = state_errors + seed_errors + value_errors + clock_errors + accumulator_errors == 0
        && coverage == [4096; 7]
        && selections == [[true; 256]; 7]
        && physical_outside_musical_table > 0
        && portamento_branches.iter().all(|v| *v != 0)
        && sources_destinations == [[true; 40]; 16]
        && atomic_rejections == 1
        && enabled_transposes == [[true; 256]; 7];
    let report = serde_json::json!({"passed":passed,"whole_original_calls":calls,"service_coverage":coverage,"all256_scale_selections":selections==[[true;256];7],
        "physical_scale_ROM_reads_outside_musical_table":physical_outside_musical_table,"portamento_branches":portamento_branches,
        "all256_enabled_global_transpose_bytes_per_service":enabled_transposes == [[true;256];7],
        "whole_SYS01ef78_calls":coverage[6],"source_destination_pairs_covered":sources_destinations.iter().flatten().filter(|v|**v).count(),
        "state_errors":state_errors,"seed_errors":seed_errors,"value_errors":value_errors,"clock_errors":clock_errors,"accumulator_errors":accumulator_errors,
        "first_state_error":first_state_error,"first_clock_error":first_clock_error,"controller_bytes_compared":u64::from(calls)*496,
        "source_outputs_used_only_for_assertions":true,"atomic_controller_and_shared_PRNG_rejections":atomic_rejections,
        "native_application_used_for_whole_SYS01ef78":true,"complete_SYS01ee38_and_production_audio_qualified":false});
    fs::write(
        out.join("actor-note-initialization-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native whole per-note initialization differs or coverage incomplete".into());
    }
    Ok(())
}
