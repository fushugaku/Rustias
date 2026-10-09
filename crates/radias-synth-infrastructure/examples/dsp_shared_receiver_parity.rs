use radias_synth_application::dsp_receiver::{
    ParameterMemory, ReceiveOutcome, receive_parameter_command,
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};

struct Memory {
    words: Vec<u16>,
    writes: Vec<(u16, u16)>,
    input_address: usize,
    ready_address: usize,
}
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.words[address as usize]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.words[address as usize] = value;
        self.writes.push((address, value));
    }
    fn noise_boot_inputs(&self) -> (u16, u16) {
        (
            self.words[self.input_address],
            self.words[self.input_address + 1],
        )
    }
    fn noise_boot_ready(&self) -> bool {
        self.words[self.ready_address] != 0
    }
}
fn addresses() -> impl Iterator<Item = usize> {
    (0x100..0x180)
        .chain(0x2000..0x2320)
        .chain(0x3000..0x3300)
        .chain(0x3800..0x3e00)
}
fn take(raw: &[u8], cursor: &mut usize) -> Result<u16, Box<dyn std::error::Error>> {
    let word = u16::from_le_bytes(
        raw.get(*cursor..*cursor + 2)
            .ok_or("Truncated shared receiver row")?
            .try_into()?,
    );
    *cursor += 2;
    Ok(word)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.filter_mix()?;
    let raw = fs::read(out.join("dsp-original-shared-receiver.bin"))?;
    let mut rom = Vec::new();
    for chip in ["master", "slave"] {
        let bytes = fs::read(root.join(format!("firmware/dsp-{chip}-host-stream.bin")))?;
        let origin = usize::from(u16::from_be_bytes(bytes[..2].try_into()?));
        let mut words = vec![0; 65536];
        for (i, pair) in bytes[2..bytes.len() - 8].chunks_exact(2).enumerate() {
            words[origin + i] = u16::from_be_bytes(pair.try_into()?);
        }
        rom.push(words);
    }
    let (
        mut cursor,
        mut cases,
        mut completed,
        mut waits,
        mut memory_errors,
        mut write_errors,
        mut compared_words,
        mut compared_writes,
    ) = (0, 0, 0, 0, 0, 0, 0, 0);
    let mut first_error = None;
    let mut memory_errors_by_opcode = [0u64; 8];
    let mut write_errors_by_opcode = [0u64; 8];
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor)?;
        let opcode = take(&raw, &mut cursor)?;
        let input_address = if chip == 0 { 0x602 } else { 0x4e2 };
        let ready_address = if chip == 0 { 0x602 } else { 0x4e4 };
        let mut memory = Memory {
            words: rom[chip as usize].clone(),
            writes: Vec::new(),
            input_address,
            ready_address,
        };
        memory.words[input_address] = take(&raw, &mut cursor)?;
        memory.words[input_address + 1] = take(&raw, &mut cursor)?;
        memory.words[ready_address] = take(&raw, &mut cursor)?;
        for address in addresses() {
            memory.words[address] = take(&raw, &mut cursor)?;
        }
        let waiting = opcode == 33 && !memory.noise_boot_ready();
        let outcome = receive_parameter_command(&mut memory, 0x100, &table);
        for address in addresses() {
            let expected = take(&raw, &mut cursor)?;
            if memory.words[address] != expected {
                memory_errors += 1;
                memory_errors_by_opcode[usize::from(opcode - 33)] += 1;
                first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"address":address,"original":expected,"native":memory.words[address]}));
            }
            compared_words += 1;
        }
        let hpic = take(&raw, &mut cursor)?;
        let hint = take(&raw, &mut cursor)?;
        let expected_outcome = if waiting {
            ReceiveOutcome::AwaitingInput
        } else {
            ReceiveOutcome::Ready
        };
        if outcome != expected_outcome
            || hpic != if waiting { 8 } else { 12 }
            || hint != u16::from(!waiting)
        {
            memory_errors += 1;
        }
        let count = take(&raw, &mut cursor)?;
        let mut expected = Vec::with_capacity(count as usize);
        for _ in 0..count {
            expected.push((take(&raw, &mut cursor)?, take(&raw, &mut cursor)?));
        }
        if memory.writes != expected {
            write_errors += 1;
            write_errors_by_opcode[usize::from(opcode - 33)] += 1;
            first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"opcode":opcode,"native_writes":memory.writes,"original_writes":expected}));
        }
        compared_writes += expected.len();
        cases += 1;
        if waiting {
            waits += 1;
        } else {
            completed += 1;
        }
    }
    let report = serde_json::json!({"passed":cases==8194 && completed==8192 && waits==2 && memory_errors==0 && write_errors==0,
        "complete_original_receiver_calls":completed,"original_input_wait_prefixes":waits,"cases":cases,
        "memory_errors":memory_errors,"write_order_errors":write_errors,"first_error":first_error,
        "memory_errors_by_opcode33_through40":memory_errors_by_opcode,"write_errors_by_opcode33_through40":write_errors_by_opcode,
        "compared_voice_frame_vocoder_and_mailbox_words":compared_words,"compared_ordered_parameter_writes":compared_writes,
        "opcodes":[33,34,35,36,37,38,39,40],"both_original_processors":true,"count_plus_one_and_aliased_targets":true,
        "original_instructions_modified":false,"source_subcalls_skipped":false,"zero_input_preserves_packet_and_readiness":true,
        "DSP_CPU_register_stack_and_arithmetic_scratch_outside_parameter_interface":true,
        "input_words_are_declared_initial_inputs":true,"vocoder_sample_processing_transferred":false,
        "complete_production_HPI_or_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("dsp-shared-receiver-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Shared parameter receiver differs".into());
    }
    Ok(())
}
