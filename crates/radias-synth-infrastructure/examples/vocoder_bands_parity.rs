use radias_synth_domain::vocoder::{BandEnvelopes, SynthesisCoefficients, SynthesisFilterBank};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/vocoder-bands-original.bin"))?;
    let mut cursor = 0;
    if word(&raw, &mut cursor) != 0x56425331 {
        return Err("Original band corpus header differs".into());
    }
    let scenes = word(&raw, &mut cursor);
    let frames = word(&raw, &mut cursor);
    let mut errors = [0u64; 2];
    let mut first_difference = None;
    for scene in 0..scenes {
        let attack = word(&raw, &mut cursor) as i32;
        let release = word(&raw, &mut cursor) as i32;
        let mut envelope = BandEnvelopes {
            levels: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
        };
        for frame in 0..frames {
            envelope.update(
                core::array::from_fn(|_| word(&raw, &mut cursor) as i32),
                attack,
                release,
            );
            for (at, actual) in envelope.levels.iter().enumerate() {
                let expected = word(&raw, &mut cursor);
                if u32::from(*actual) != expected {
                    errors[0] += 1;
                    first_difference.get_or_insert(serde_json::json!({"kind":"envelope","scene":scene,"frame":frame,"field":at,"expected":expected,"actual":actual}));
                }
            }
        }
        let coefficients = SynthesisCoefficients {
            damping: word(&raw, &mut cursor) as i32,
            frequencies: core::array::from_fn(|_| word(&raw, &mut cursor) as i32),
        };
        let mut bank = SynthesisFilterBank {
            states: core::array::from_fn(|_| {
                core::array::from_fn(|_| word(&raw, &mut cursor) as i32)
            }),
        };
        for frame in 0..frames {
            bank.process(word(&raw, &mut cursor) as i32, &coefficients);
            for (at, actual) in bank.states.iter().flatten().enumerate() {
                let expected = word(&raw, &mut cursor) as i32;
                if *actual != expected {
                    errors[1] += 1;
                    first_difference.get_or_insert(serde_json::json!({"kind":"carrier","scene":scene,"frame":frame,"field":at,"expected":expected,"actual":actual}));
                }
            }
        }
    }
    if cursor != raw.len() || scenes != 512 || frames != 16 {
        return Err("Incomplete original band comparison".into());
    }
    let result = serde_json::json!({"passed":errors==[0,0],"errors_by_kernel":errors,"first_difference":first_difference,
        "original_envelope_calls":scenes*frames,"original_carrier_calls":scenes*frames,
        "state_fields_compared":scenes*frames*80,"original_output_words_used_as_native_input":false,"complete_vocoder_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/vocoder-bands-parity.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    if errors != [0, 0] {
        return Err("Native vocoder bands differ".into());
    }
    Ok(())
}
