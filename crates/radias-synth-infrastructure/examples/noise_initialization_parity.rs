use radias_synth_domain::noise::NoiseFrameSeeds;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn numeric(out: &Path, chip: usize) -> Result<[usize; 36], Box<dyn std::error::Error>> {
    let raw = fs::read(out.join(if chip == 0 {
        "noise-initialization.bin"
    } else {
        "slave-noise-initialization.bin"
    }))?;
    if raw.len() != 65536 * 152 {
        return Err("Original frame initialization corpus incomplete".into());
    }
    let mut errors = [0; 36];
    for (index, row) in raw.chunks_exact(152).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let seeds = NoiseFrameSeeds::from_inputs(w(0) as i16, w(1) as i16);
        let actual: [u32; 36] = core::array::from_fn(|i| match i {
            0..12 => seeds.primary[i].0 >> 16,
            12..24 => seeds.secondary[i - 12].0 >> 16,
            _ => (seeds.mixer[i - 24].state as u32) >> 16,
        });
        for (field, error) in errors.iter_mut().enumerate() {
            if actual[field] != w(field + 2) {
                if *error < 2 {
                    eprintln!(
                        "DSP{chip} initialization case{index} field{field}:{} vs{}",
                        actual[field],
                        w(field + 2)
                    );
                }
                *error += 1;
            }
        }
    }
    Ok(errors)
}

fn observed_boot(out: &Path, chip: usize) -> Result<bool, Box<dyn std::error::Error>> {
    let prefix = if chip == 0 {
        "live-mixer-noise-lifecycle-reference"
    } else {
        "live-mixer-noise-lifecycle-reference-slave"
    };
    let boot: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join(format!("{prefix}-noise-boot-inputs.json")),
    )?)?;
    let seeds = NoiseFrameSeeds::from_inputs(
        boot["input602"]
            .as_u64()
            .ok_or("Original accepted first boot input absent")? as i16,
        boot["input603"]
            .as_u64()
            .ok_or("Original accepted second boot input absent")? as i16,
    );
    let first_phase_exact = boot["expected_first_primary_phase_high"].as_u64()
        == Some((seeds.primary[0].0 >> 16) as u64);
    let initializers: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{prefix}-mixer-noise-initializers.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    let rows: Vec<_> = initializers
        .iter()
        .filter(|row| row["pc"].as_u64() == Some(0xe13e))
        .collect();
    Ok(first_phase_exact
        && rows.len() == 12
        && rows.iter().enumerate().all(|(slot, row)| {
            row["address"].as_u64() == Some((0x3016 + slot * 64) as u64)
                && row["value"].as_u64() == Some((seeds.mixer[slot].state as u32 >> 16) as u64)
        }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let errors = [numeric(&out, 0)?, numeric(&out, 1)?];
    let boot_exact = [observed_boot(&out, 0)?, observed_boot(&out, 1)?];
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let seeds = radias_synth_infrastructure::firmware::formant_counter_seeds(&system)?;
    let notes = fs::read(out.join("noise-note-initialization.bin"))?;
    if notes.len() != 256 * 8 {
        return Err("Original Formant note initialization corpus incomplete".into());
    }
    let mut counter_errors = 0usize;
    for (index, row) in notes.chunks_exact(8).enumerate() {
        let slot = u32::from_le_bytes(row[..4].try_into().unwrap());
        let expected = u32::from_le_bytes(row[4..].try_into().unwrap());
        if slot != index as u32 {
            return Err("Original Formant counter slot order differs".into());
        }
        if seeds.for_slot(slot as u8) as i32 as u32 != expected {
            counter_errors += 1;
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0)
        && boot_exact.iter().all(|&exact| exact)
        && counter_errors == 0;
    let report = serde_json::json!({"passed":passed,"original_calls":131072,"fields_per_call":36,
        "errors":errors.iter().map(|row|row.to_vec()).collect::<Vec<_>>(),
        "source_entries":["MasterE0D4","SlaveE0D4"],"accepted_input_words_declared":true,
        "original_ready_wait_excluded":true,"primary_secondary_and_mixer_boot_seed_banks_exact":passed,
        "observed_original_boot_primary_phase_and_twelve_mixer_seeds_exact":boot_exact,
        "original_formant_note_initializers":256,"formant_counter_errors":counter_errors,
        "formant_counter_by_controller_slot_exact":counter_errors==0,
        "physical_note_reuse_lifecycle_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("noise-initialization-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Noise initialization mismatch".into());
    }
    Ok(())
}
