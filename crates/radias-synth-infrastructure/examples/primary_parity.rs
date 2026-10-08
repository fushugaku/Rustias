use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    primary_oscillator::{PrimaryRampOscillator, PrimaryRampParameters},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/original-primary-ramp.bin"))?;
    let mut mismatches = 0;
    for (index, r) in raw.chunks_exact(68).enumerate() {
        let word = |i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().unwrap());
        let param = |i: usize| word(1 + i) as i16;
        let mut osc = PrimaryRampOscillator {
            phase: Phase(word(0)),
            offset: param(11),
        };
        let p = PrimaryRampParameters {
            increment: PhaseIncrement((word(1) << 16) | word(2)),
            shape: param(6),
            blend: param(8),
            offset_target: param(2),
            target_gain: param(9),
            memory_gain: param(10),
        };
        let sample = osc.next_sample(&table, p);
        if sample.0 != word(14) as i32 || osc.phase.0 != word(15) || osc.offset != word(16) as i16 {
            if mismatches < 3 {
                eprintln!(
                    "Primary {index}: out {} != {}, phase {:?}/{}, offset {}/{}, p={p:?}",
                    sample.0,
                    word(14) as i32,
                    osc.phase,
                    word(15),
                    osc.offset,
                    word(16) as i16
                );
            }
            mismatches += 1;
        }
    }
    println!(
        "{{\"primary_ramp_cases\":{},\"mismatches\":{mismatches}}}",
        raw.len() / 68
    );
    if mismatches != 0 {
        return Err("Primary oscillator parity failed".into());
    }
    Ok(())
}
