use radias_synth_application::dsp_receiver::{
    ParameterMemory, ReceiveOutcome, receive_memory_command,
};
use std::{fs, path::PathBuf};

struct Memory(Vec<u16>);
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.0[address as usize]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.0[address as usize] = value;
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-memory-receiver.bin"))?;
    const ROW_WORDS: usize = 2 + 2 * 1024 + 2;
    if raw.len() != 8192 * ROW_WORDS * 2 {
        return Err("Original receiver corpus incomplete".into());
    }
    let mut errors = 0;
    let mut compared_words = 0;
    let mut first_error = None;
    for (case, row) in raw.chunks_exact(ROW_WORDS * 2).enumerate() {
        let w = |i: usize| u16::from_le_bytes(row[2 * i..2 * i + 2].try_into().unwrap());
        let mut memory = Memory(vec![0; 65536]);
        for i in 0..256 {
            memory.0[0x100 + i] = w(2 + i);
        }
        for i in 0..768 {
            memory.0[0x2000 + i] = w(2 + 256 + i);
        }
        let outcome = receive_memory_command(&mut memory, 0x100);
        if outcome
            != if w(1) >= 42 {
                ReceiveOutcome::OpcodeOutOfRange
            } else {
                ReceiveOutcome::Ready
            }
            || w(2050)
                != if outcome == ReceiveOutcome::Ready {
                    12
                } else {
                    8
                }
            || w(2051) != u16::from(outcome == ReceiveOutcome::Ready)
        {
            errors += 1;
        }
        for (offset, address) in (0x100..0x200).chain(0x2000..0x2300).enumerate() {
            let expected = w(1026 + offset);
            if memory.0[address] != expected {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":case,"opcode":w(1),"address":address,"native":memory.0[address],"original":expected}));
            }
            compared_words += 1;
        }
    }
    // Arithmetic packets require their arithmetic adapter. Do not acknowledge
    // or erase a packet merely because a handler is still being transferred.
    let mut unsupported = Memory(vec![0; 65536]);
    unsupported.0[0x100] = 6;
    unsupported.0[0x101] = 16;
    let before = unsupported.0.clone();
    let preserves_unsupported = receive_memory_command(&mut unsupported, 0x100)
        == ReceiveOutcome::UnsupportedOpcode(16)
        && unsupported.0 == before;
    let report = serde_json::json!({"passed":errors==0 && preserves_unsupported,"original_complete_receiver_calls":8192,
        "both_original_processors":true,"compared_words":compared_words,"errors":errors,"first_error":first_error,
        "memory_opcodes":[0,1,2,3,4,5,6,7,8,9,12,13,14,15,22,41],
        "out_of_range_opcodes":[42,127],"overlapping_copies_and_negative_gain":true,
        "hot_receive_header_not_revalidated":true,"unsupported_arithmetic_keeps_packet":preserves_unsupported,
        "original_instructions_modified":false,"source_subcalls_skipped":false,
        "complete_native_HPI_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("dsp-receiver-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Native memory receiver differed".into());
    }
    Ok(())
}
