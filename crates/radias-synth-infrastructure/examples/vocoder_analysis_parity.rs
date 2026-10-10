use radias_synth_domain::vocoder::{AnalysisCoefficients, AnalysisFilterBank};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/vocoder-analysis-original.bin"))?;
    let mut cursor = 0;
    if word(&raw, &mut cursor) != 0x56414231 {
        return Err("Original vocoder corpus header differs".into());
    }
    let scenes = word(&raw, &mut cursor);
    let frames = word(&raw, &mut cursor);
    let mut errors = 0;
    let mut first_difference = None;
    for scene in 0..scenes {
        let coefficients = AnalysisCoefficients {
            bands: core::array::from_fn(|_| {
                core::array::from_fn(|_| word(&raw, &mut cursor) as i32)
            }),
        };
        let mut bank = AnalysisFilterBank {
            words: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
        };
        for frame in 0..frames {
            bank.process(word(&raw, &mut cursor) as i32, &coefficients);
            for (at, actual) in bank.words.iter().enumerate() {
                let expected = word(&raw, &mut cursor);
                if u32::from(*actual) != expected {
                    errors += 1;
                    first_difference.get_or_insert(serde_json::json!({"scene":scene,"frame":frame,"word":at,"expected":expected,"actual":actual}));
                }
            }
        }
    }
    if cursor != raw.len() || scenes != 1024 || frames != 8 {
        return Err("Incomplete original vocoder comparison".into());
    }
    let result = serde_json::json!({
        "passed": errors == 0,
        "errors": errors,
        "first_difference": first_difference,
        "original_sample_calls": scenes * frames,
        "bands_per_frame": 16,
        "history_words_compared": scenes * frames * 132,
        "original_output_words_used_as_native_input": false,
        "complete_vocoder_or_FXD03_audio_verified": false
    });
    fs::write(
        root.join("runs/native-clone/vocoder-analysis-parity.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    if errors != 0 {
        return Err("Native vocoder analysis differs".into());
    }
    Ok(())
}
