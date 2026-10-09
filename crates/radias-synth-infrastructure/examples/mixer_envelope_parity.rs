use radias_synth_domain::{Sample, envelope::EnvelopeLevel, mixer::OscillatorMix};
use std::{fs, path::PathBuf};

fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or("..".into()));
    let raw = fs::read(root.join("runs/native-clone/original-envelopes.bin"))?;
    let mut eg_errors = 0;
    for (i, r) in raw.chunks_exact(16).enumerate() {
        let mut level = EnvelopeLevel(word(r, 0) as i16);
        let result = level.step(word(r, 1) as i16, word(r, 2) as i16);
        if result != word(r, 3) as i16 {
            if eg_errors < 3 {
                eprintln!("EG {i}: {result} != {}", word(r, 3) as i16);
            }
            eg_errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/original-mixers.bin"))?;
    let mut mix_errors = 0;
    for (i, r) in raw.chunks_exact(32).enumerate() {
        let m = OscillatorMix {
            primary_gain: word(r, 4) as i16,
            secondary_gain: word(r, 5) as i16,
            noise_gain: word(r, 6) as i16,
        };
        let result = m.sample(
            Sample(word(r, 0) as i32),
            Sample(word(r, 1) as i32),
            word(r, 2) as i16,
            word(r, 3) as i16,
        );
        if result.0 != word(r, 7) as i32 {
            if mix_errors < 3 {
                eprintln!("Mix {i}: {} != {}", result.0, word(r, 7) as i32);
            }
            mix_errors += 1;
        }
    }
    println!(
        "{{\"envelope_cases\":32768,\"envelope_errors\":{eg_errors},\"mixer_cases\":32768,\"mixer_errors\":{mix_errors}}}"
    );
    if eg_errors + mix_errors != 0 {
        return Err("Envelope/mixer parity failed".into());
    }
    Ok(())
}
