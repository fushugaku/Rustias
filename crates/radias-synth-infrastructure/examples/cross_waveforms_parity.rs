use radias_synth_domain::{
    Phase, Sample,
    pitch::PhaseIncrement,
    primary_oscillator::{
        PrimaryCrossSineParameters, PrimaryCrossTriangleParameters, PrimaryOscillator,
        PrimaryParameters, PrimaryTriangleParameters,
    },
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&image)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/cross-waveforms.bin"))?;
    if raw.len() != 65536 * 116 {
        return Err("Original Cross waveform corpus truncated".into());
    }
    let mut errors = [0usize; 2];
    for (i, row) in raw.chunks_exact(116).enumerate() {
        let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let p = |n: usize| word(n + 3) as i16;
        let q = |n: usize| (word(n + 3) << 16 | word(n + 4)) as i32;
        let mode = word(0) as usize;
        let mut osc = PrimaryOscillator {
            phase: Phase(word(1)),
            ..Default::default()
        };
        let parameters = if mode == 0 {
            PrimaryParameters::CrossTriangle(PrimaryCrossTriangleParameters {
                carrier: PrimaryTriangleParameters {
                    increment: PhaseIncrement(q(0) as u32),
                    center: p(6),
                    gain: p(7),
                    edge_gain: 0,
                    upper: p(8),
                    upper_reflection: p(9),
                    lower: p(10),
                    lower_reflection: p(11),
                },
                modulation_gain: p(3),
            })
        } else {
            PrimaryParameters::CrossSine(PrimaryCrossSineParameters {
                increment: PhaseIncrement(q(0) as u32),
                modulation_gain: p(3),
                center: p(8),
                polynomial: [
                    ((p(11) as u16 as u32) << 16 | p(10) as u16 as u32) as i32,
                    q(12),
                    q(14),
                ],
            })
        };
        let actual = osc.next_with_modulator(&table, parameters, Sample(word(2) as i32));
        if actual.0 != word(27) as i32 || osc.phase.0 != word(28) {
            if errors[mode] < 3 {
                eprintln!(
                    "Cross {mode}/{i}: {actual:?}/{:?} vs {}/{}, p={parameters:?}",
                    osc.phase,
                    word(27) as i32,
                    word(28)
                );
            }
            errors[mode] += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==[0,0],"original_samples":65536,"triangle_errors":errors[0],"sine_errors":errors[1],"original_instructions_modified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/cross-waveforms-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != [0, 0] {
        return Err("Cross waveform generator differs".into());
    }
    Ok(())
}
