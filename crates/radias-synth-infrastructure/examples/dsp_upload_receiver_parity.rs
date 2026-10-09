use radias_synth_application::dsp_receiver::{
    ParameterMemory, ReceiveOutcome, receive_parameter_command,
};
use radias_synth_domain::{filter_control::FilterMixTable, parameter_upload::transfer_count};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Clone, Default)]
struct Memory {
    words: BTreeMap<u32, u16>,
    writes: Vec<(u32, u16)>,
    wide: bool,
}
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.words.get(&u32::from(address)).copied().unwrap_or(0)
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.write_upload_word(u32::from(address), value);
    }
    fn upload_word_mapped(&self, address: u32) -> bool {
        address < if self.wide { 0x200000 } else { 0x10000 }
    }
    fn write_upload_word(&mut self, address: u32, value: u16) {
        assert!(self.upload_word_mapped(address));
        self.words.insert(address, value);
        self.writes.push((address, value));
    }
}
fn word(raw: &[u8], cursor: &mut usize) -> Result<u16, Box<dyn std::error::Error>> {
    let value = u16::from_le_bytes(
        raw.get(*cursor..*cursor + 2)
            .ok_or("Truncated original upload row")?
            .try_into()?,
    );
    *cursor += 2;
    Ok(value)
}
fn long(raw: &[u8], cursor: &mut usize) -> Result<u32, Box<dyn std::error::Error>> {
    let lo = word(raw, cursor)?;
    let hi = word(raw, cursor)?;
    Ok(u32::from(lo) | (u32::from(hi) << 16))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-upload-receiver.bin"))?;
    let table = FilterMixTable {
        weights: [[0; 128]; 5],
    }; // Loader commands do not read filter ROM.
    let (
        mut cursor,
        mut cases,
        mut errors,
        mut write_errors,
        mut compared_words,
        mut compared_writes,
        mut wide_targets,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = word(&raw, &mut cursor)?;
        let opcode = word(&raw, &mut cursor)?;
        let count = long(&raw, &mut cursor)?;
        let mut memory = Memory {
            wide: true,
            ..Default::default()
        };
        let mut addresses = Vec::new();
        for _ in 0..count {
            let address = long(&raw, &mut cursor)?;
            let value = word(&raw, &mut cursor)?;
            memory.words.insert(address, value);
            addresses.push(address);
        }
        let outcome = receive_parameter_command(&mut memory, 0x100, &table);
        for address in addresses {
            let expected = word(&raw, &mut cursor)?;
            let actual = memory.words[&address];
            compared_words += 1;
            if expected != actual {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"address":address,"original":expected,"native":actual}));
            }
        }
        let hpic = word(&raw, &mut cursor)?;
        let hint = word(&raw, &mut cursor)?;
        if outcome != ReceiveOutcome::Ready || hpic != 12 || hint != 1 {
            errors += 1;
        }
        let count = long(&raw, &mut cursor)?;
        let mut expected = Vec::new();
        for _ in 0..count {
            let address = long(&raw, &mut cursor)?;
            let value = word(&raw, &mut cursor)?;
            expected.push((address, value));
        }
        if memory.writes != expected {
            write_errors += 1;
            first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"original_writes":expected,"native_writes":memory.writes}));
        }
        if expected.iter().any(|&(address, _)| address >= 0x10000) {
            wide_targets += 1;
        }
        compared_writes += expected.len();
        cases += 1;
    }
    let zero: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join("dsp-original-upload-zero-count-prefix.json"),
    )?)?;
    let zero_count_decoding = zero["packet_count"] == 0
        && zero["original_processors"] == 2
        && u64::from(transfer_count(0)) == zero["original_repeat_count"].as_u64().unwrap() + 1;
    // A16-bit memory adapter must reject a wider address before changing data,
    // rather than truncate its bank or partially overwrite a packet.
    let mut narrow = Memory::default();
    for (address, value) in [
        (0x100, 6),
        (0x101, 10),
        (0x102, 0),
        (0x103, 4),
        (0x104, 1),
        (0x105, 0x1000),
        (0x106, 4),
    ] {
        narrow.words.insert(address, value);
    }
    let before = narrow.words.clone();
    let unmapped = receive_parameter_command(&mut narrow, 0x100, &table)
        == ReceiveOutcome::UnmappedUploadWord(0x11000)
        && narrow.words == before
        && narrow.writes.is_empty();
    let report = serde_json::json!({"passed":cases==8192&&errors==0&&write_errors==0&&wide_targets>1000&&zero_count_decoding&&unmapped,
        "complete_original_receiver_calls":cases,"opcodes":[10,11],"both_original_processors":true,
        "parameter_and_upload_words_compared":compared_words,"ordered_upload_and_mailbox_writes_compared":compared_writes,
        "calls_with_extended_destinations":wide_targets,"memory_errors":errors,"write_order_errors":write_errors,"first_error":first_error,
        "counts1_through64_signed_raw_counters_and_23bit_addresses":true,"payload_mailbox_aliases_and_16bit_bank_crossing":true,
        "zero_count_source_decoding_prefixes":2,"zero_count_repeat_decoding_matches":zero_count_decoding,"complete_zero_count_wrap_CPU_alias_upload_qualified":false,
        "unmapped_upload_keeps_packet_unchanged":unmapped,"native_domain_upload_cursor_used":true,"original_instructions_modified":false,
        "source_subcalls_skipped":false,"uploaded_code_interpreted_by_native":false,"complete_production_HPI_or_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("dsp-upload-receiver-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Native upload receiver differs".into());
    }
    Ok(())
}
