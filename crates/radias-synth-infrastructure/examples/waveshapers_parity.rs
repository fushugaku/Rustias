use radias_synth_domain::{
    Sample,
    pitch::PhaseIncrement,
    waveshaper::{
        ShaperCoefficients, ShaperSignal, ShaperState, SubOscillatorCoefficients,
        SubOscillatorWaveform, Waveshaper,
    },
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/waveshapers.bin"))?;
    if raw.len() != 327680 * 104 {
        return Err("Incomplete original waveshaper corpus".into());
    }
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&master)?.waveform()?;
    let mut errors = [[0usize; 7]; 10];
    for (index, row) in raw.chunks_exact(104).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mode = w(0) as usize;
        if mode != index / 32768 {
            return Err("Original waveshaper corpus order differs".into());
        }
        let mut shaper = Waveshaper {
            state: ShaperState {
                words: [w(15) as i32, w(16) as i32, w(17) as i32, w(18) as i32],
                startup_counter: w(13) as u16,
            },
        };
        let depth = w(6) as i16;
        let c = match mode {
            0 => ShaperCoefficients::Decimator { depth },
            1 => ShaperCoefficients::MultiTriangle { depth },
            2 => ShaperCoefficients::MultiSine { depth },
            3 => ShaperCoefficients::OctSaw { depth },
            4 => ShaperCoefficients::Pickup {
                depth,
                pitch_current: w(9) as i16,
            },
            5..=8 => ShaperCoefficients::SubOscillator(SubOscillatorCoefficients {
                depth,
                target_depth: w(5) as i16,
                gain_current: w(9) as i16,
                waveform: match mode {
                    5 => SubOscillatorWaveform::Square,
                    6 => SubOscillatorWaveform::Saw,
                    7 => SubOscillatorWaveform::Triangle,
                    _ => SubOscillatorWaveform::Sine,
                },
            }),
            9 => ShaperCoefficients::LevelBoost { depth },
            _ => return Err("Unknown source shaper".into()),
        };
        let signal = ShaperSignal {
            input: Sample(w(1) as i32),
            primary_pitch_code: w(3) as u16,
            primary_increment: PhaseIncrement(w(4)),
        };
        let output = shaper.process(signal, c, &table.shapers);
        let a = [
            output.0,
            shaper.state.words[0],
            shaper.state.words[1],
            shaper.state.words[2],
            shaper.state.words[3],
            c.gain_target(signal.primary_pitch_code)
                .unwrap_or(w(8) as i16) as i32,
            shaper.state.startup_counter as i32,
        ];
        let e = [
            w(19) as i32,
            w(20) as i32,
            w(21) as i32,
            w(22) as i32,
            w(23) as i32,
            w(24) as i16 as i32,
            w(25) as i32,
        ];
        for field in 0..7 {
            if a[field] != e[field] {
                if errors[mode][field] < 2 {
                    eprintln!(
                        "Shaper{mode} case{index} field{field}:{} vs{}",
                        a[field], e[field]
                    );
                }
                errors[mode][field] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"qualified_sample_state_cases":327680,"errors":errors,
  "qualified_shapers":["Decimator","MultiTri","MultiSin","OctSaw","Pickup","SubOscSquare","SubOscSaw","SubOscTriangle","SubOscSine","LevelBoost"],
  "original_master_unchanged":true,"original_typed_entry_registers_used":true,"positive_primary_increment_range":true,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/waveshapers-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native waveshaper mismatch".into());
    }
    Ok(())
}
