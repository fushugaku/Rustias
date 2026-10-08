use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    primary_oscillator::{
        PrimaryRampOscillator, PrimaryRampParameters, PrimarySineOscillator, PrimarySineParameters,
        PrimaryTriangleParameters, triangle_sample,
    },
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let mut reports = Vec::new();
    for mode in 0..3 {
        let raw = fs::read(root.join(format!("runs/native-clone/original-primary-va-{mode}.bin")))?;
        if raw.len() != 32768 * 96 {
            return Err("Incomplete primary VA corpus".into());
        }
        let mut errors = 0;
        for (index, r) in raw.chunks_exact(96).enumerate() {
            let word = |i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().unwrap());
            let param = |i: usize| word(i + 2) as i16;
            let pair = |i: usize| ((word(i + 2) << 16) | word(i + 3)) as i32;
            let increment = PhaseIncrement(pair(0) as u32);
            let (output, phase, phase2, control, offset) = match mode {
                0 => {
                    let mut osc = PrimaryRampOscillator {
                        phase: Phase(word(0)),
                        offset: param(11),
                    };
                    let result = osc.next_pulse(
                        &table,
                        PrimaryRampParameters {
                            increment,
                            shape: param(6),
                            blend: param(8),
                            offset_target: param(2),
                            target_gain: param(9),
                            memory_gain: param(10),
                        },
                    );
                    (result.0, osc.phase.0, word(1), param(2), osc.offset)
                }
                1 => {
                    let mut phase = Phase(word(0));
                    let result = triangle_sample(
                        &mut phase,
                        PrimaryTriangleParameters {
                            increment,
                            center: param(6),
                            gain: param(7),
                            edge_gain: param(3),
                            upper: param(8),
                            upper_reflection: param(9),
                            lower: param(10),
                            lower_reflection: param(11),
                        },
                    );
                    (result.0, phase.0, word(1), param(2), param(11))
                }
                _ => {
                    let mut osc = PrimarySineOscillator {
                        phase: Phase(word(0)),
                        modulated_phase: Phase(word(1)),
                        control_feedback: param(2),
                    };
                    let result = osc.next_sample(PrimarySineParameters {
                        increment,
                        modulation_gain: param(3),
                        control: [param(6), param(7)],
                        center: param(8),
                        polynomial: [((word(13) << 16) | word(12)) as i32, pair(12), pair(14)],
                    });
                    (
                        result.0,
                        osc.phase.0,
                        osc.modulated_phase.0,
                        osc.control_feedback,
                        param(11),
                    )
                }
            };
            let actual = [
                output as u32,
                phase,
                phase2,
                control as u16 as u32,
                offset as u16 as u32,
            ];
            let expected = [word(19), word(20), word(21), word(22), word(23)];
            if actual != expected {
                if errors < 3 {
                    eprintln!(
                        "Primary VA {mode}/{index}: {actual:?} != {expected:?}, parameters={:?}",
                        (0..17).map(param).collect::<Vec<_>>()
                    );
                }
                errors += 1;
            }
        }
        reports.push(serde_json::json!({"mode":mode,"cases":32768,"errors":errors}));
    }
    let passed = reports.iter().all(|r| r["errors"] == 0);
    let report = serde_json::json!({"scope":"Original OSC1 pulse, folded triangle and phase-modulated polynomial sine","sets":reports,"passed":passed});
    fs::write(
        root.join("runs/native-clone/primary-va-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Primary VA parity failed".into());
    }
    Ok(())
}
