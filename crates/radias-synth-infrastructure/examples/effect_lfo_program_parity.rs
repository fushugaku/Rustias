use radias_synth_application::{
    modulation::VoiceModulationTables, polyphony::PolyphonicRenderer,
    shared_lfo::EffectLfoParameters,
};
use radias_synth_domain::effect_control::{EffectBank, EffectKind};
use radias_synth_domain::effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot};
use radias_synth_infrastructure::{
    effects::EffectLibrary,
    firmware::{MasterTables, lfo_tables, lfo_tempo_tables, modulation_tables},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&source)?;
    let tempo = lfo_tempo_tables(&source)?;
    let master_image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_image)?;
    let modulation = VoiceModulationTables {
        lfo: lfo_tables(&source)?,
        matrix: modulation_tables(&source)?,
        pitch: master.pitch()?,
        bandwidth: master.bandwidth()?,
    };
    let mut pool = PolyphonicRenderer::default();
    pool.enable_tempo_clock(tempo, 1200);
    for t in 0..4 {
        pool.set_timbre_modulation_active(t, true);
    }
    for slot in 0..9 {
        let parameters = EffectLfoParameters {
            alternate_phase: slot + 17,
            frequency: 64,
            ..Default::default()
        };
        pool.edit_effect_lfo(
            if slot < 8 { Some(slot / 2) } else { None },
            usize::from(slot % 2),
            parameters,
        )
        .map_err(|_| "Initial effect configuration rejected")?;
    }
    let raw = fs::read(root.join("runs/native-clone/effect-lfo-program-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated LFO source words".into());
    }
    let words: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    const ROW: usize = 7 + 8 + 32 + 6 + 32 + 6 + 32;
    if words[0] != 0x454c5031 || !(words.len() - 1).is_multiple_of(ROW) {
        return Err("Invalid original LFO program records".into());
    }
    let (mut cases, mut errors, mut rate_stores) = (0usize, 0usize, 0usize);
    let mut seen = BTreeSet::new();
    let mut first = Value::Null;
    let mut pool_checks = [0usize; 9];
    for row in words[1..].chunks_exact(ROW) {
        let [bank, kind, variant, slot, mode, offset, clock]: [u32; 7] =
            row[..7].try_into().unwrap();
        let mapping = if bank < 2 {
            library.lfo_mapping(
                if bank == 0 {
                    EffectBank::Insert
                } else {
                    EffectBank::Master
                },
                EffectKind::new(kind as u8).ok_or("Invalid source effect kind")?,
            )?
        } else {
            EffectLfoMapping {
                fields: [10, 11, 8, 13, 12, 7, 9, 0],
                definition_mode: kind as u8,
            }
        };
        let mapping_matches =
            mapping.definition_mode as u32 == mode && mapping.fields.map(u32::from) == row[7..15];
        let parameters: Vec<_> = row[15..47].iter().map(|v| *v as u8).collect();
        let prior = EffectLfoProgram {
            bytes: row[47..53]
                .iter()
                .map(|v| *v as u8)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        };
        let before = &row[53..85];
        let original_program = &row[85..91];
        let original_state = &row[91..123];
        let publication = prior
            .prepare(
                &parameters,
                mapping,
                EffectLfoSlot::new(slot as u8).ok_or("Invalid original LFO slot")?,
                offset,
                clock,
                &tempo,
            )
            .ok_or("Original LFO input rejected")?;
        let mut expected_state = before.to_vec();
        let mut candidate_program = prior;
        if let Some(p) = publication {
            candidate_program = p.program;
            for (i, b) in p.tempo_increment.to_be_bytes().into_iter().enumerate() {
                expected_state[4 + i] = u32::from(b);
            }
            rate_stores += 1;
            pool.service_lfos(0, &modulation);
            pool.service_lfos(1, &modulation);
            let pool_before = if slot < 8 {
                pool.shared_lfo_states(slot as usize / 2).unwrap()[slot as usize % 2 + 2]
            } else {
                pool.global_lfo_state()
            };
            let old_alt = pool
                .effect_lfo_parameters(slot as u8)
                .unwrap()
                .alternate_phase;
            pool.apply_effect_lfo_publication(p)
                .map_err(|_| "Live pool rejected original LFO publication")?;
            let after = if slot < 8 {
                pool.shared_lfo_states(slot as usize / 2).unwrap()[slot as usize % 2 + 2]
            } else {
                pool.global_lfo_state()
            };
            let projected = pool.effect_lfo_parameters(slot as u8).unwrap();
            if after != pool_before
                || projected.mode != p.program.bytes[0]
                || projected.frequency != p.program.bytes[2]
                || projected.phase_sync != p.program.bytes[3]
                || projected.beat != p.program.bytes[4]
                || projected.alternate_phase != old_alt
                || pool.effect_lfo_rate(slot as u8) != Some(p.tempo_increment)
            {
                return Err(
                    "Live native effect publication changed phase or rate incorrectly".into(),
                );
            }
            pool_checks[slot as usize] += 1;
        }
        if !mapping_matches
            || candidate_program.bytes.map(u32::from) != original_program
            || expected_state != original_state
        {
            errors += 1;
            if first.is_null() {
                first = json!({"bank":bank,"kind":kind,"variant":variant,"slot":slot,"mode":mode,
                "mapping_matches":mapping_matches,"native_program":candidate_program.bytes,"original_program":original_program,
                "native_state":expected_state,"original_state":original_state});
            }
        }
        seen.insert((bank, kind, variant));
        cases += 1;
    }
    let mut expected = BTreeSet::new();
    for bank in 0..2 {
        for kind in 0..31 {
            for variant in 0..256 {
                expected.insert((bank, kind, variant));
            }
        }
    }
    for mode in 0..256 {
        for variant in 0..17 {
            expected.insert((2, mode, variant));
        }
    }
    let invalid = EffectLfoProgram::default()
        .prepare(
            &[],
            EffectLfoMapping {
                fields: [255; 8],
                definition_mode: 1,
            },
            EffectLfoSlot::new(0).unwrap(),
            0,
            0,
            &tempo,
        )
        .is_none();
    let dormant = EffectLfoProgram::default().prepare(
        &[],
        EffectLfoMapping {
            fields: [255; 8],
            definition_mode: 0,
        },
        EffectLfoSlot::new(0).unwrap(),
        0,
        0,
        &tempo,
    ) == Some(None);
    let passed = cases == 20224
        && seen == expected
        && errors == 0
        && invalid
        && dormant
        && pool_checks.iter().all(|n| *n > 0);
    let report = json!({"passed":passed,"whole_original_LFO_program_calls":cases,"errors":errors,"first_difference":first,"rate_stores":rate_stores,
        "original_descriptor_banks":2,"effect_types_per_bank":31,"definition_mode_byte_domain_complete":true,
        "live_pool_publications_by_slot":pool_checks,"live_publications_preserve_running_phase_and_alternate_phase":true,
        "all_6_configuration_bytes_and_32_phase_state_bytes_compared":true,
        "only_original_tempo_increment_store_replaces_phase_state_bytes":true,"invalid_mapping_rejected":invalid,"mode_zero_does_not_read_mapping":dormant,
        "source_tempo_table_and_descriptor_mapping_are_declared_inputs":true,"original_configurations_or_rates_used_as_native_inputs":false,
        "native_controller_executes_firmware_instructions":false,"FXD03_audio_execution_or_full_effect_lifecycle_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-lfo-program-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native effect LFO programs: {cases} complete original calls, {errors} differences");
    if !passed {
        return Err("Native effect LFO program differs from original SYS".into());
    }
    Ok(())
}
