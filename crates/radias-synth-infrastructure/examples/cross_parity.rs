use radias_synth_domain::{
    Phase, Sample,
    pitch::PhaseIncrement,
    primary_oscillator::{PrimaryCrossParameters, PrimaryOscillator, PrimaryParameters},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/original-primary-cross.bin"))?;
    if raw.len() != 32768 * 84 {
        return Err("Incomplete Cross corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(84).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
        let param = |n: usize| word(n + 2) as i16;
        let mut osc = PrimaryOscillator {
            phase: Phase(word(0)),
            ..Default::default()
        };
        let p = PrimaryCrossParameters {
            increment: PhaseIncrement(word(2) << 16 | word(3)),
            modulation_gain: param(3),
            shape: param(6),
            offset: param(7),
            blend: param(8),
        };
        let result =
            osc.next_with_modulator(&table, PrimaryParameters::Cross(p), Sample(word(1) as i32));
        if result.0 != word(19) as i32 || osc.phase.0 != word(20) {
            if errors < 3 {
                eprintln!(
                    "Cross {i}: {result:?}/{:?} != {}/{}",
                    osc.phase,
                    word(19) as i32,
                    word(20)
                );
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"scope":"Original C430..C4BF primary Cross generator","cases":32768,"errors":errors,"passed":errors==0});
    println!("{report}");
    fs::write(
        root.join("runs/native-clone/cross-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Cross kernel parity failed".into());
    }
    Ok(())
}
