//! Whole SYS0150d8 comparison, including modulation-before-EG ordering.
use radias_synth_application::note_modulators::{
    NoteModulatorRequest, NoteModulatorTables, initialize_note_modulators,
};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_lfo_initialization::ActorLfoState,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn actor(raw: &[u8], cursor: &mut usize) -> ActorControlState {
    let bytes = raw[*cursor..*cursor + 496].try_into().unwrap();
    *cursor += 496;
    ActorControlState { bytes }
}
fn pair(raw: &[u8], cursor: &mut usize) -> [ActorLfoState; 2] {
    core::array::from_fn(|_| {
        let bytes = raw[*cursor..*cursor + 32].try_into().unwrap();
        *cursor += 32;
        ActorLfoState { bytes }
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let curves = firmware::envelope_curves(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let modulation = firmware::modulation_tables(&sys)?;
    let tables = NoteModulatorTables {
        curves: &curves,
        timing: &timing,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let raw = fs::read(out.join("note-modulators-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4e4d4931 {
        return Err("Unsupported note modulator observation".into());
    }
    let (mut cases, mut state_errors, mut random_errors, mut clock_errors, mut accumulator_errors) =
        (0, 0, 0, 0, 0);
    let mut first_error = None;
    let mut rejection_cases = 0;
    let mut source_coverage = [false; 16];
    let mut destination_coverage = [false; 40];
    let mut envelope_destinations = [0u32; 12];
    while cursor < raw.len() {
        let variant = take(&raw, &mut cursor);
        let seed = take(&raw, &mut cursor) as u16;
        let bend = take(&raw, &mut cursor) as i16;
        let wheel = take(&raw, &mut cursor) as u8;
        let auxiliary = take(&raw, &mut cursor) as i16;
        let midi_receive_flags = take(&raw, &mut cursor) as u8;
        let mut assignments = [0; 5];
        let mut assignable_values = [0; 5];
        for index in 0..5 {
            assignments[index] = take(&raw, &mut cursor) as u8;
            assignable_values[index] = take(&raw, &mut cursor) as i8;
        }
        let ports = ActorVirtualPatchPorts {
            bend,
            wheel,
            auxiliary,
            midi_receive_flags,
            assignments,
            assignable_values,
        };
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let controller = actor(&raw, &mut cursor);
        let lfos = pair(&raw, &mut cursor);
        let shared_lfos = pair(&raw, &mut cursor);
        let clocks = take(&raw, &mut cursor);
        let expected_seed = take(&raw, &mut cursor) as u16;
        let expected_controller = actor(&raw, &mut cursor);
        let expected_lfos = pair(&raw, &mut cursor);
        let expected_targets: [i32; 40] = core::array::from_fn(|_| take(&raw, &mut cursor) as i32);
        let expected_linked = take(&raw, &mut cursor) as i32;
        let initialized = initialize_note_modulators(
            NoteModulatorRequest {
                controller,
                lfos,
                shared_lfos,
                random_seed: seed,
                body: &body,
                ports,
            },
            &tables,
        )
        .map_err(|error| format!("{error:?}"))?;
        if initialized.controller != expected_controller || initialized.lfos != expected_lfos {
            state_errors += 1;
            first_error.get_or_insert(format!(
                "case {variant}: full controller or LFO state differs"
            ));
        }
        if initialized.random_seed != expected_seed {
            random_errors += 1;
            first_error.get_or_insert(format!("case {variant}: PRNG differs"));
        }
        if u32::from(initialized.controller_clocks) != clocks {
            clock_errors += 1;
            first_error.get_or_insert(format!(
                "case {variant}: clocks {} != {clocks}",
                initialized.controller_clocks
            ));
        }
        if initialized.modulations.values != expected_targets
            || initialized.modulations.linked_pitch != expected_linked
        {
            accumulator_errors += 1;
            first_error.get_or_insert(format!("case {variant}: modulation accumulators differ"));
        }
        for route in 0..6 {
            source_coverage[(body[86 + 3 * route] & 15) as usize] = true;
            let destination = (body[87 + 3 * route] & 63) as usize;
            destination_coverage[destination] = true;
            if destination >= 28 {
                envelope_destinations[destination - 28] += 1;
            }
        }
        if variant == 0 {
            let mut bad_body = body;
            bad_body[86..89].copy_from_slice(&[5, 63, 65]);
            let mut bad_controller = controller;
            bad_controller.bytes[0x37] = 100;
            bad_controller.bytes[0x1c4] = 0;
            bad_controller.bytes[0x158..0x15a].fill(0);
            let bad = initialize_note_modulators(
                NoteModulatorRequest {
                    controller: bad_controller,
                    lfos,
                    shared_lfos,
                    random_seed: seed,
                    body: &bad_body,
                    ports,
                },
                &tables,
            );
            if !matches!(bad, Err(ActorVirtualPatchError::InvalidDestination(63))) {
                return Err("Invalid active route did not reject whole initialization".into());
            }
            rejection_cases += 1;
        }
        cases += 1;
    }
    let passed = cases == 4096
        && state_errors == 0
        && clock_errors == 0
        && random_errors == 0
        && accumulator_errors == 0
        && source_coverage == [true; 16]
        && destination_coverage == [true; 40]
        && envelope_destinations.iter().all(|v| *v != 0)
        && rejection_cases == 1;
    let report = serde_json::json!({
        "passed":passed,"whole_original_SYS0150d8_calls":cases,
        "state_errors":state_errors,"clock_errors":clock_errors,"random_errors":random_errors,
        "accumulator_errors":accumulator_errors,"first_error":first_error,
        "all16_sources":source_coverage == [true; 16],"all40_destinations":destination_coverage == [true; 40],
        "envelope_destination_coverage":envelope_destinations,"invalid_route_rejection_cases":rejection_cases,
        "voice_bytes_compared":cases * 560,"modulation_words_compared":cases * 41,
        "modulation_before_EG_initialization_and_shared_PRNG_order_checked":true,
        "source_output_used_only_for_assertions":true,
        "complete_constructor_and_production_audio_qualified":false
    });
    fs::write(
        out.join("note-modulators-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native whole note modulator service differs or coverage incomplete".into());
    }
    Ok(())
}
