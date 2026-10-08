//! Four-timbre compilation and routing from the unchanged user's RDL bank.
use radias_synth_application::program::StoredProgram;
use radias_synth_domain::voice_allocation::VoiceCostParameters;
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::{ControlMap, PreparedVoice},
    rdl,
    stored_program::compile_program,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let bank = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let cost_tables = firmware::voice_cost_tables(&sys)?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&master)?;
    let mix = tables.filter_mix()?;
    let map = ControlMap::from_json(&fs::read(
        root.join("assets/native-va/filter-controls.json"),
    )?)?;
    let template =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let mut compiled = Vec::new();
    let mut generator_available = 0;
    for p in &bank {
        let c = compile_program(p, 0, &map, &mix, template.parameters.filter)?;
        generator_available += usize::from(c.validate_native_generators().is_ok());
        compiled.push(c);
    }
    let raw = fs::read(out.join("stored-program-costs.bin"))?;
    if raw.len() != 1024 * 12 {
        return Err("Original stored cost corpus incomplete".into());
    }
    for row in raw.chunks_exact(12) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let c = compiled[w(0) as usize].stored.timbres[w(1) as usize].controls;
        let cost = cost_tables
            .cost(VoiceCostParameters {
                primary: c.oscillator_selection,
                secondary: c.secondary.selection,
                filter_route: c.filter_route,
                drive_mode: c.shaper.allocation_mode(),
                shaper_type: c.shaper.allocation_type(),
            })
            .ok_or("Native cost descriptor invalid")?;
        if cost != w(2) {
            return Err(format!(
                "Stored source cost differs in program{} timbre{}: {cost} != {}",
                w(0),
                w(1),
                w(2)
            )
            .into());
        }
    }
    let raw = fs::read(out.join("stored-program-routing.bin"))?;
    if raw.len() != 16384 * 44 {
        return Err("Original routing corpus incomplete".into());
    }
    let mut routed = 0;
    for row in raw.chunks_exact(44) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let c = StoredProgram::from_program(&bank[w(0) as usize], w(4) as u8)
            .map_err(|_| "Invalid stored routing")?;
        let mut mask = 0;
        let mut order = [255u32; 4];
        let mut count = 0;
        for t in (0..4).rev() {
            if c.timbres[t].accepts(w(1) as u8, w(2) as u8) {
                mask |= 1 << t;
                order[count] = t as u32;
                count += 1;
            }
        }
        if mask != w(5)
            || order != core::array::from_fn(|i| w(6 + i))
            || 65533u16.wrapping_add(count as u16) as u32 != w(10)
        {
            return Err(format!(
                "Stored original routing differs: p{} note{} ch{} mask{mask:x}/{}",
                w(0),
                w(2),
                w(1),
                w(5)
            )
            .into());
        }
        routed += count;
    }
    let report = serde_json::json!({"passed":true,"lossless_RDL_programs":256,"compiled_stored_timbres":1024,
        "original_voice_cost_cases":1024,"original_ordinary_MIDI_routing_cases":16384,"routed_timbre_events":routed,
        "source_dispatch_order_and_per_routed_timbre_age_increment_match":true,"programs_with_current_native_oscillator_selections":generator_available,
        "PCM_input_drum_sequence_vocoder_effects_completion_qualified":false,"joint_full_bank_audio_qualified":false,
        "application_compiler_no_std_and_no_CPU_emulation":true});
    fs::write(
        out.join("stored-program-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
