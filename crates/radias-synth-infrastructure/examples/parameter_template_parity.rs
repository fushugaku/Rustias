//! Whole original static constructors compared to direct domain compilation.
use radias_synth_domain::parameter_template::{ParameterTemplate, TemplateCompilationError};
use radias_synth_infrastructure::firmware::{self, MasterTables};
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
    let dsp = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = firmware::parameter_template_tables(
        &sys,
        MasterTables::from_host_stream(&dsp)?.filter_mix()?,
    )?;
    let boot = fs::read(out.join("parameter-template-boot-original.bin"))?;
    let (mut cursor, mut words, mut errors, mut boot_banks) = (0, 0, 0, 0);
    let mut first_error = None;
    while cursor < boot.len() {
        let chip = take(&boot, &mut cursor);
        let before: [u16; 3200] = core::array::from_fn(|_| take(&boot, &mut cursor) as u16);
        let after: [u16; 3200] = core::array::from_fn(|_| take(&boot, &mut cursor) as u16);
        for index in 0..20 {
            let original: [u16; 160] = after[index * 160..(index + 1) * 160].try_into()?;
            let prior = before[index * 160..(index + 1) * 160].try_into()?;
            let native = ParameterTemplate::boot(index, prior).ok_or("Invalid boot bank")?;
            for (offset, original_word) in original.iter().enumerate() {
                words += 1;
                if native.words[offset] != *original_word {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"boot":true,"chip":chip,"index":index,"offset":offset,"source":original[offset],"native":native.words[offset]}));
                }
            }
            boot_banks += 1;
        }
    }
    let raw = fs::read(out.join("parameter-template-original.bin"))?;
    let (mut cursor, mut calls, mut receivers) = (0, 0, 0);
    let mut coverage = [0u64; 20];
    while cursor < raw.len() {
        let index = take(&raw, &mut cursor) as usize;
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [[u16; 160]; 2] =
            core::array::from_fn(|_| core::array::from_fn(|_| take(&raw, &mut cursor) as u16));
        receivers += take(&raw, &mut cursor);
        let after: [[u16; 160]; 2] =
            core::array::from_fn(|_| core::array::from_fn(|_| take(&raw, &mut cursor) as u16));
        for chip in 0..2 {
            let mut native = ParameterTemplate {
                words: before[chip],
            };
            native
                .compile(&body, &tables)
                .map_err(|e| format!("{e:?}"))?;
            for (offset, original_word) in after[chip].iter().enumerate() {
                words += 1;
                if native.words[offset] != *original_word {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"call":calls,"chip":chip,"index":index,"body":body.to_vec(),"offset":offset,"source":after[chip][offset],"native":native.words[offset]}));
                }
            }
        }
        coverage[index] += 1;
        calls += 1;
    }
    // Unsupported deferred generators and out-of-domain gain reject without
    // publishing a partially compiled image.
    let prior = [0x35a7; 160];
    for wave in [6, 7, 8, 15] {
        let mut body = [0; 104];
        body[22] = wave;
        let mut native = ParameterTemplate { words: prior };
        if native.compile(&body, &tables) != Err(TemplateCompilationError::PcmOrInputGenerator)
            || native.words != prior
        {
            return Err("Deferred generator mutated template".into());
        }
    }
    for gain in 128..=255 {
        let mut body = [0; 104];
        body[51] = gain;
        let mut native = ParameterTemplate { words: prior };
        if native.compile(&body, &tables) != Err(TemplateCompilationError::InvalidOutputGain)
            || native.words != prior
        {
            return Err("Invalid gain mutated template".into());
        }
    }
    let report = serde_json::json!({
        "passed":errors==0,"whole_original_SYS_static_constructor_calls":calls,
        "whole_original_E319_receives":receivers,"boot_banks_both_chips":boot_banks,
        "compared_parameter_words":words,"errors":errors,"first_error":first_error,
        "template_address_coverage":coverage,"original_other_template_banks_unchanged":true,
        "raw_program_and_prior_parameter_state_are_declared_inputs":true,
        "source_outputs_used_only_for_comparison":true,
        "firmware_tables_used_no_instruction_interpreter":true,
        "whole_note_constructor_or_DMA_job_timing_qualified":false,
        "complete_native_engine":false
    });
    fs::write(
        out.join("parameter-template-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original static template construction differs".into());
    }
    Ok(())
}
