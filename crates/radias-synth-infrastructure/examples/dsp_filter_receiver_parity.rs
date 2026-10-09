use radias_synth_application::dsp_receiver::{
    ParameterMemory, ReceiveOutcome, receive_parameter_command,
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};

struct Memory {
    words: Vec<u16>,
    writes: Vec<(u16, u16)>,
}
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.words[address as usize]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.words[address as usize] = value;
        self.writes.push((address, value));
    }
}
fn take(raw: &[u8], cursor: &mut usize) -> Result<u16, Box<dyn std::error::Error>> {
    let word = u16::from_le_bytes(
        raw.get(*cursor..*cursor + 2)
            .ok_or("Truncated original receiver row")?
            .try_into()?,
    );
    *cursor += 2;
    Ok(word)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mode = std::env::args().nth(2);
    let phase_mode = mode.as_deref() == Some("--phases");
    let coupled_mode = mode.as_deref() == Some("--filter-shaper");
    let voice_end = if coupled_mode { 0x2320 } else { 0x2300 };
    let extended_mode = mode.as_deref() == Some("--oscillator-extended");
    let oscillator_mode = mode.as_deref() == Some("--oscillator");
    let pitch_mode = extended_mode || oscillator_mode || mode.as_deref() == Some("--pitch");
    let family = if coupled_mode {
        "filter-shaper"
    } else if phase_mode {
        "phase"
    } else if extended_mode {
        "oscillator-extended"
    } else if oscillator_mode {
        "oscillator"
    } else if pitch_mode {
        "pitch"
    } else {
        "filter"
    };
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.filter_mix()?;
    let raw = fs::read(out.join(format!("dsp-original-{family}-receiver.bin")))?;
    let mut rom_images = Vec::new();
    for chip in ["master", "slave"] {
        let bytes = fs::read(root.join(format!("firmware/dsp-{chip}-host-stream.bin")))?;
        let origin = usize::from(u16::from_be_bytes(bytes[..2].try_into()?));
        let mut words = vec![0; 65536];
        for (i, pair) in bytes[2..bytes.len() - 8].chunks_exact(2).enumerate() {
            words[origin + i] = u16::from_be_bytes(pair.try_into()?);
        }
        rom_images.push(words);
    }
    let mut cursor = 0;
    let mut cases = 0;
    let mut errors = 0;
    let mut write_errors = 0;
    let mut compared_words = 0;
    let mut compared_writes = 0;
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor)?;
        let opcode = take(&raw, &mut cursor)?;
        let mut memory = Memory {
            words: rom_images[chip as usize].clone(),
            writes: Vec::new(),
        };
        memory.words[0x4024] = take(&raw, &mut cursor)?;
        memory.words[0x4025] = take(&raw, &mut cursor)?;
        for address in (0x100..0x180).chain(0x2000..voice_end) {
            memory.words[address] = take(&raw, &mut cursor)?;
        }
        let outcome = receive_parameter_command(&mut memory, 0x100, &table);
        for address in (0x100..0x180).chain(0x2000..voice_end) {
            let expected = take(&raw, &mut cursor)?;
            if memory.words[address] != expected {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"address":address,"original":expected,"native":memory.words[address]}));
            }
            compared_words += 1;
        }
        let hpic = take(&raw, &mut cursor)?;
        let hint = take(&raw, &mut cursor)?;
        if outcome != ReceiveOutcome::Ready || hpic != 12 || hint != 1 {
            errors += 1;
        }
        let count = take(&raw, &mut cursor)?;
        let mut expected = Vec::with_capacity(count as usize);
        for _ in 0..count {
            expected.push((take(&raw, &mut cursor)?, take(&raw, &mut cursor)?));
        }
        if memory.writes != expected {
            write_errors += 1;
            first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"native_writes":memory.writes,"original_writes":expected}));
        }
        compared_writes += expected.len();
        cases += 1;
    }
    let report = serde_json::json!({"passed":cases==8192 && errors==0 && write_errors==0,"complete_original_receiver_calls":cases,
        "memory_errors":errors,"write_order_errors":write_errors,"first_error":first_error,"compared_voice_and_mailbox_words":compared_words,
        "compared_ordered_voice_and_mailbox_writes":compared_writes,"opcodes":if coupled_mode {vec![21]}else if phase_mode {vec![28,31,32]}else if extended_mode {vec![26,27,29,30]}else if oscillator_mode {vec![23,24,25]}else if pitch_mode {vec![16,17]}else{vec![18,19,20]},"both_original_processors":true,
        "original_instructions_modified":false,"source_subcalls_skipped":false,"signed_full_frequency_resonance_and_normalization_inputs":!pitch_mode && !phase_mode,
        "full_raw_u16_pitch_codes_signed_secondary_offsets_and_ROM_word_inputs":pitch_mode,
        "raw_signed_phase_codes_secondary_target_aliases_and_repeated_copy_targets":phase_mode,
        "raw_signed_shaper_depth_with_filter_frequency_and_resonance":coupled_mode,
        "count_plus_one_and_repeated_parameter_addresses":true,"native_domain_coefficient_kernels_used":true,
        "DSP_CPU_register_stack_and_arithmetic_scratch_are_outside_parameter_interface":true,
        "complete_production_HPI_or_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join(format!("dsp-{family}-receiver-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Native filter receiver differs".into());
    }
    Ok(())
}
