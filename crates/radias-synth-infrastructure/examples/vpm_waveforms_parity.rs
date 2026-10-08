use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    primary_oscillator::{
        PrimaryOscillator, PrimaryParameters, PrimaryTriangleParameters, PrimaryVpmCarrier,
        PrimaryVpmCarrierParameters, PrimaryVpmModulatorParameters, PrimaryVpmParameters,
    },
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(root.join("runs/native-clone/vpm-waveforms.bin"))?;
    if raw.len() != 98304 * 176 {
        return Err("Incomplete VPM carrier corpus".into());
    }
    let mut errors = [0usize; 3];
    for (i, r) in raw.chunks_exact(176).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[4 * n..4 * n + 4].try_into().unwrap());
        let p = |n: usize| word(n + 3) as i16;
        let q = |n: usize| ((word(n + 3) << 16) | word(n + 4)) as i32;
        let mode = word(0) as usize;
        if mode != i / 32768 {
            return Err("VPM corpus order differs".into());
        }
        let mut oscillator = PrimaryOscillator {
            phase: Phase(word(1)),
            modulated_phase: Phase(word(2)),
            ..Default::default()
        };
        let increment = PhaseIncrement(q(0) as u32);
        let params = if mode == 0 {
            PrimaryParameters::Vpm(PrimaryVpmParameters {
                increment,
                modulation_gain: p(3),
                ratio: p(4),
                limit: p(9),
                center: q(10),
                shape: p(6),
                offset: p(7),
                blend: p(8),
            })
        } else {
            let carrier = if mode == 1 {
                PrimaryVpmCarrier::Triangle(PrimaryTriangleParameters {
                    increment,
                    center: p(10),
                    gain: p(11),
                    edge_gain: 0,
                    upper: p(12),
                    upper_reflection: p(13),
                    lower: p(14),
                    lower_reflection: p(15),
                })
            } else {
                PrimaryVpmCarrier::Sine {
                    center: p(10),
                    polynomial: [
                        (((p(13) as u16 as u32) << 16) | (p(12) as u16 as u32)) as i32,
                        q(14),
                        q(16),
                    ],
                }
            };
            PrimaryParameters::VpmCarrier(PrimaryVpmCarrierParameters {
                modulator: PrimaryVpmModulatorParameters {
                    increment,
                    modulation_gain: p(3),
                    ratio: p(4),
                    limit: p(6),
                    center: q(8),
                },
                carrier,
            })
        };
        let output = oscillator.next_sample(&table, params);
        if output.0 != word(41) as i32
            || oscillator.phase.0 != word(42)
            || oscillator.modulated_phase.0 != word(43)
        {
            if errors[mode] < 3 {
                eprintln!(
                    "VPM mode{mode} row{i}: {output:?}/{:?}/{:?} vs {}/{}/{}",
                    oscillator.phase,
                    oscillator.modulated_phase,
                    word(41) as i32,
                    word(42),
                    word(43)
                );
            }
            errors[mode] += 1;
        }
    }
    let report = serde_json::json!({"passed":errors.iter().all(|&n|n==0),"original_samples":98304,"saw_pulse_errors":errors[0],"triangle_errors":errors[1],"sine_errors":errors[2],"original_instructions_modified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/vpm-waveforms-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors.iter().any(|&n| n != 0) {
        return Err("Native VPM carrier mismatch".into());
    }
    Ok(())
}
