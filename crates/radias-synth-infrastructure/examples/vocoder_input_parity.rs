use radias_synth_domain::vocoder::{AnalysisInput, AnalysisInputParameters};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/vocoder-input-original.bin"))?;
    let mut cursor = 0;
    if word(&raw, &mut cursor) != 0x56494631 {
        return Err("Original input corpus header differs".into());
    }
    let scenes = word(&raw, &mut cursor);
    let frames = word(&raw, &mut cursor);
    let mut errors = 0;
    let mut first_difference = None;
    for scene in 0..scenes {
        let words: [u16; 16] = core::array::from_fn(|_| word(&raw, &mut cursor) as u16);
        let pair = |at: usize| ((u32::from(words[at]) << 16) | u32::from(words[at + 1])) as i32;
        let parameters = AnalysisInputParameters {
            input_gain: words[3] as i16,
            attack: pair(4),
            release: pair(6),
            gate: pair(8),
            high_pass_gain: words[10] as i16,
            high_pass_feedback: words[11] as i16,
            high_pass_frequency: words[12] as i16,
            high_pass_attack: words[13] as i16,
            high_pass_release: words[14] as i16,
        };
        let mut state = AnalysisInput {
            words: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
        };
        for frame in 0..frames {
            state.process(
                [
                    word(&raw, &mut cursor) as i32,
                    word(&raw, &mut cursor) as i32,
                ],
                parameters,
            );
            for (at, actual) in state.words.iter().enumerate() {
                let expected = word(&raw, &mut cursor);
                if u32::from(*actual) != expected {
                    errors += 1;
                    first_difference.get_or_insert(serde_json::json!({"scene":scene,"frame":frame,"word":at,"expected":expected,"actual":actual}));
                }
            }
        }
    }
    if cursor != raw.len() || scenes != 1024 || frames != 16 {
        return Err("Incomplete original input comparison".into());
    }
    let result = serde_json::json!({"passed":errors==0,"errors":errors,"first_difference":first_difference,
        "original_sample_calls":scenes*frames,"history_words_compared":scenes*frames*20,
        "original_output_words_used_as_native_input":false,"complete_vocoder_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/vocoder-input-parity.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    if errors != 0 {
        return Err("Native vocoder front end differs".into());
    }
    Ok(())
}
