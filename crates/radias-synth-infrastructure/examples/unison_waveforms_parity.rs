use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    unison::{UnisonOscillator, UnisonParameters, UnisonWaveform},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&image)?;
    let normalization =
        (u32::from(master.word(0x4024)?) << 16 | u32::from(master.word(0x4025)?)) as i32;
    let raw = fs::read(root.join("runs/native-clone/unison-waveforms.bin"))?;
    if raw.len() != 98304 * 184 {
        return Err("Original Unison waveform corpus truncated".into());
    }
    let mut errors = [0usize; 3];
    for (i, row) in raw.chunks_exact(184).enumerate() {
        let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let q = |n: usize| word(n + 1) << 16 | word(n + 2);
        let mode = word(0) as usize;
        let mut oscillator = UnisonOscillator {
            phases: [18, 20, 22, 26, 28].map(|n| Phase(q(n))),
        };
        let parameters = UnisonParameters {
            increments: [0, 6, 8, 12, 14].map(|n| PhaseIncrement(q(n))),
            detune: 0,
            correction_gain: word(17) as i16,
            level: word(18) as i16,
        };
        let waveform = match mode {
            0 => UnisonWaveform::Pulse {
                bandwidth: word(31) as i16,
            },
            1 => UnisonWaveform::Triangle,
            2 => UnisonWaveform::Sine { normalization },
            _ => return Err("Invalid Unison waveform".into()),
        };
        let output = oscillator.next_sample_waveform(parameters, waveform);
        let phases = oscillator.phases.map(|p| p.0);
        let expected = core::array::from_fn::<_, 5, _>(|j| word(j + 40));
        if output.0 != word(39) as i32 || phases != expected || phases[0] != word(45) {
            if errors[mode] < 3 {
                eprintln!(
                    "Unison {mode}/{i}: {output:?}, {phases:?} vs {},{expected:?}",
                    word(39) as i32
                );
            }
            errors[mode] += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==[0;3],"original_samples":98304,"pulse_errors":errors[0],"triangle_errors":errors[1],"sine_errors":errors[2],"original_instructions_modified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/unison-waveforms-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != [0; 3] {
        return Err("Unison carrier differs".into());
    }
    Ok(())
}
