use radias_synth_domain::{
    drum::{DRUM_KIT_BYTES, DrumKit, DrumProgram},
    program_binding::{ActorProgramBinding, TimbreProgramBinding, bind_selected},
};
use radias_synth_infrastructure::rdl;
use std::{fs, path::PathBuf};
fn w(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn actor(r: &[u8], i: usize) -> ActorProgramBinding {
    ActorProgramBinding {
        selection_bit: w(r, i),
        owner: w(r, i + 1),
        common: w(r, i + 2),
        synthesis: w(r, i + 3),
        voice_cost: w(r, i + 4) as u16,
        uses_program_common: w(r, i + 5) as u8,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let kits = rdl::drum_kits(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let original = fs::read(out.join("drum-original-kits.bin"))?;
    if original.len() != 32 * DRUM_KIT_BYTES
        || kits.iter().enumerate().any(|(i, k)| {
            k.bytes().as_slice() != &original[i * DRUM_KIT_BYTES..(i + 1) * DRUM_KIT_BYTES]
        })
    {
        return Err("Native drum extraction differs".into());
    }
    let programs = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let cost_table = radias_synth_infrastructure::firmware::voice_cost_tables(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let costs = fs::read(out.join("drum-original-costs.bin"))?;
    if costs.len() != 512 * 12 {
        return Err("Original drum costs incomplete".into());
    }
    let mut instrument_bindings = 0;
    let master_image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master =
        radias_synth_infrastructure::firmware::MasterTables::from_host_stream(&master_image)?;
    let map = radias_synth_infrastructure::prepared::ControlMap::from_system(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let mix = master.filter_mix()?;
    let base = radias_synth_infrastructure::prepared::PreparedVoice::from_program_json(&fs::read(
        root.join("assets/native-va/saw.json"),
    )?)?
    .parameters
    .filter;
    let mut owner_program = *programs[0].bytes();
    owner_program[24] = 32;
    let owner_program = radias_synth_domain::program::Program::from_bytes(&owner_program).unwrap();
    let mut compiled_graphs = 0;
    for (k, kit) in kits.iter().enumerate() {
        let compiled = radias_synth_infrastructure::stored_program::compile_drum_kit(
            &owner_program,
            kit.clone(),
            &map,
            &mix,
            base,
        )?;
        for index in 0..16 {
            let owner = programs[0].timbre(0).unwrap();
            let instrument = kit.instrument(index).ok_or("Native instrument missing")?;
            let actual = radias_synth_application::program::TimbreControls::from_drum_instrument(
                owner, instrument,
            )
            .map_err(|_| "Invalid drum patch route")?;
            let cost = cost_table
                .cost(radias_synth_domain::voice_allocation::VoiceCostParameters {
                    primary: actual.oscillator_selection,
                    secondary: actual.secondary.selection,
                    filter_route: actual.filter_route,
                    drive_mode: actual.shaper.allocation_mode(),
                    shaper_type: actual.shaper.allocation_type(),
                })
                .ok_or("Drum cost selection invalid")?;
            let r = &costs[12 * (k * 16 + index)..12 * (k * 16 + index + 1)];
            if w(r, 0) != k as u32 || w(r, 1) != index as u32 || w(r, 2) != cost {
                return Err(
                    "Native drum synthesis cost differs from complete original call".into(),
                );
            }
            if actual.primary().selection != instrument[0x16]
                || actual.oscillator_controls != instrument[0x17..0x19]
                || actual.amplifier_level != instrument[0x2d]
                || actual.pan != instrument[0x31]
                || actual.pitch.transpose != instrument[0x13]
                || actual.pitch.fine_tune != instrument[0x14]
            {
                return Err("Drum synthesis field binding differs".into());
            }
            for eg in 0..3 {
                let b = 0x34 + 8 * eg;
                let bound = actual.envelope[eg];
                if bound.adsr != instrument[b..b + 4]
                    || bound.curve != instrument[b + 4]
                    || bound.velocity_level_sensitivity != instrument[b + 5]
                    || bound.velocity_time_sensitivity != instrument[b + 6]
                    || bound.key_tracking != instrument[b + 7]
                {
                    return Err("Drum envelope binding differs".into());
                }
            }
            instrument_bindings += 1;
            let bound = &compiled.instruments[index];
            if bound.controls.oscillator_selection != instrument[0x16]
                || bound.controls.amplifier_level != instrument[0x2d]
                || bound.graph.filter2.output
                    != [
                        radias_synth_domain::filter_routing::Filter2Output::LowPass,
                        radias_synth_domain::filter_routing::Filter2Output::HighPass,
                        radias_synth_domain::filter_routing::Filter2Output::BandPass,
                        radias_synth_domain::filter_routing::Filter2Output::Comb,
                    ][((instrument[0x21] >> 4) & 3) as usize]
                || bound.graph.dynamic_filter.input.cutoff != instrument[0x23]
            {
                return Err("Independent drum graph/controller ownership differs".into());
            }
            compiled_graphs += 1;
        }
    }
    let routes = fs::read(out.join("drum-original-routing.bin"))?;
    let bindings = fs::read(out.join("drum-original-binding.bin"))?;
    if routes.len() != 65536 * 92 || bindings.len() != 8192 * 1184 {
        return Err("Incomplete original drum corpus".into());
    }
    let mut routing_errors = 0;
    let mut binding_errors = 0;
    for (n, r) in routes.chunks_exact(92).enumerate() {
        let mut raw = [0; DRUM_KIT_BYTES];
        for i in 0..16 {
            raw[36 + i] = w(r, 4 + i) as u8;
        }
        let kit = DrumKit::from_bytes(&raw).unwrap();
        let selection = DrumProgram::from_raw(w(r, 0) as u8, 100, 64, w(r, 1) as u8);
        let mask = if selection.timbre.is_some() {
            kit.trigger_mask(w(r, 2) as u8, selection.transpose)
        } else {
            0
        };
        let ordinary = 15 ^ selection.timbre.map_or(0, |t| 1 << t);
        if mask as u32 != w(r, 20) || ordinary != w(r, 21) || mask.count_ones() != w(r, 22) {
            if routing_errors < 3 {
                eprintln!(
                    "Drum route {n}: {mask:x}/{ordinary:x} != {:x}/{:x}",
                    w(r, 20),
                    w(r, 21)
                );
            }
            routing_errors += 1;
        }
    }
    for (n, r) in bindings.chunks_exact(1184).enumerate() {
        let mut timbre = TimbreProgramBinding {
            owner: w(r, 2),
            common: w(r, 3),
            synthesis: w(r, 4),
            voice_cost: w(r, 5) as u16,
        };
        let mut actors = core::array::from_fn(|i| actor(r, 7 + 6 * i));
        bind_selected(
            &mut actors,
            w(r, 0),
            &mut timbre,
            (w(r, 1) != 0).then_some(w(r, 6)),
        );
        let different = timbre.synthesis != w(r, 151)
            || actors
                .iter()
                .enumerate()
                .any(|(i, a)| *a != actor(r, 152 + 6 * i));
        if different {
            if binding_errors < 3 {
                eprintln!("Actor program binding {n} differs");
            }
            binding_errors += 1;
        }
    }
    let passed = routing_errors == 0 && binding_errors == 0;
    let report = serde_json::json!({"passed":passed,"original_drum_MIDI_selection_cases":65536,"original_complete_24_actor_binding_calls":8192,
        "native_kit_records":32,"native_instrument_records":512,"native_kit_bytes_match_original":true,
        "compiled_instrument_control_bindings":instrument_bindings,"compiled_independent_drum_graphs":compiled_graphs,"original_instrument_voice_cost_calls":512,
        "routing_errors":routing_errors,"binding_errors":binding_errors,"original_instructions_modified":false,
        "note_action_subcalls_excluded_from_routing":true,"drum_audio_and_note_lifecycle_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("drum-program-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native drum selection or actor binding differs".into());
    }
    Ok(())
}
