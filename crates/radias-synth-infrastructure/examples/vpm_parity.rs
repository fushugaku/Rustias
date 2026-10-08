use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    primary_oscillator::{PrimaryOscillator, PrimaryParameters, PrimaryVpmParameters},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/original-vpm.bin"))?;
    if raw.len() != 32768 * 88 {
        return Err("Incomplete VPM corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(88).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
        let param = |n: usize| word(n + 2) as i16;
        let pair = |n: usize| word(n + 2) << 16 | word(n + 3);
        let mut osc = PrimaryOscillator {
            phase: Phase(word(0)),
            modulated_phase: Phase(word(1)),
            ..Default::default()
        };
        let output = osc.next_sample(
            &table,
            PrimaryParameters::Vpm(PrimaryVpmParameters {
                increment: PhaseIncrement(pair(0)),
                modulation_gain: param(3),
                ratio: param(4),
                limit: param(9),
                center: pair(10) as i32,
                shape: param(6),
                offset: param(7),
                blend: param(8),
            }),
        );
        if output.0 != word(19) as i32
            || osc.phase.0 != word(20)
            || osc.modulated_phase.0 != word(21)
        {
            if errors < 3 {
                eprintln!(
                    "VPM {i}: {output:?}/{:?}/{:?} != {}/{}/{}",
                    osc.phase,
                    osc.modulated_phase,
                    word(19) as i32,
                    word(20),
                    word(21)
                );
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"scope":"Original C97C..CA4D VPM ramp generator","cases":32768,"errors":errors,"passed":errors==0});
    println!("{report}");
    fs::write(
        root.join("runs/native-clone/vpm-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Native VPM parity failed".into());
    }
    Ok(())
}
