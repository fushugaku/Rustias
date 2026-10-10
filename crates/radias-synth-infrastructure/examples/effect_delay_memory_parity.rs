//! Compares only connected delay-memory sample bits, never full effects audio.
use radias_synth_domain::{
    Sample,
    effect_delay_memory::{EffectDelayMemory, WORD_COUNT},
};
use radias_synth_infrastructure::effect_delay_memory::EffectDelayStorage;
use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        return Err("expected original snapshot, output prefix, report".into());
    }
    let initial = fs::read(&args[1])?;
    if initial.len() != WORD_COUNT * 2 {
        return Err("invalid original snapshot extent".into());
    }
    let words: Box<[u16]> = initial
        .chunks_exact(2)
        .map(|v| u16::from_be_bytes([v[0], v[1]]))
        .collect();
    let mut storage =
        EffectDelayStorage::from_cells(words).map_err(|_| "invalid storage extent")?;
    if EffectDelayMemory::new(&mut [0u16; 1]).is_ok() {
        return Err("short storage accepted".into());
    }
    let mut errors = 0usize;
    // Construction/reborrowing must preserve the original SH diagnostic image.
    {
        let memory = storage.samples();
        for address in 0..WORD_COUNT {
            let value = ((address + (address >> 16)) & 0xffff) as u32;
            errors += usize::from(memory.read(address as u32) != (value << 8));
        }
    }
    let events = fs::read(format!("{}-events.bin", args[2]))?;
    if events.len() % 20 != 0 {
        return Err("partial oracle event".into());
    }
    let mut signed = 0usize;
    let mut aligned = 0usize;
    {
        let mut memory = storage.samples();
        for record in events.chunks_exact(20) {
            let field = |index: usize| {
                u32::from_le_bytes(record[4 * index..4 * index + 4].try_into().unwrap())
            };
            let address = field(0);
            let input = field(1);
            if field(2) == 1 {
                memory.write_left_aligned(address, Sample(input as i32));
                aligned += 1;
            } else {
                memory.write(address, input);
            }
            let alias = address ^ 0xfffc0000;
            errors += usize::from(memory.read(address) != field(3));
            errors += usize::from(memory.read(alias) != field(3));
            errors += usize::from(memory.read_left_aligned(alias).0 as u32 != field(4));
            signed += usize::from(memory.read_left_aligned(alias).0 < 0);
        }
    }
    let expected = fs::read(format!("{}-cells.bin", args[2]))?;
    if expected.len() != WORD_COUNT * 2 {
        return Err("invalid final oracle snapshot".into());
    }
    for (actual, expected) in storage.cells().iter().zip(expected.chunks_exact(2)) {
        errors += usize::from(*actual != u16::from_le_bytes([expected[0], expected[1]]));
    }
    let passed = errors == 0 && events.len() / 20 == WORD_COUNT + 192 && signed > 0 && aligned > 0;
    let report = serde_json::json!({"passed":passed,"scope":"FXD03 populated delay-memory sample storage; existing independently validated C++ wiring model and original isolated SH SRAM image","sample_writes":events.len()/20,"left_aligned_writes":aligned,"negative_samples":signed,"initial_cells_compared":WORD_COUNT,"final_cells_compared":WORD_COUNT,"errors":errors,"physical_storage_bytes":WORD_COUNT*2,"low_sample_bus_bits_grounded":8,"upper_address_bits_mirrored":true,"borrowed_storage_retained":true,"native_interprets_firmware":false,"FXD03_instruction_arithmetic_qualified":false,"ASIC_memory_access_timing_qualified":false,"complete_effects_audio_qualified":false});
    fs::write(
        Path::new(&args[3]),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("delay-memory storage differs from independent oracle".into());
    }
    Ok(())
}
