use radias_synth_domain::vocoder::{
    InterpolationTables, PARAMETER_WORDS, STATE_WORDS, Vocoder, VocoderFrame,
};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/vocoder-sample-original.bin"))?;
    let mut cursor = 0;
    if word(&raw, &mut cursor) != 0x56535031 {
        return Err("Whole vocoder corpus header differs".into());
    }
    let scenes = word(&raw, &mut cursor);
    let frames = word(&raw, &mut cursor);
    let tables = InterpolationTables {
        scalar_offsets: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
        wide_offset: word(&raw, &mut cursor) as u16,
    };
    let mut errors = [0u64; 3];
    let mut first_difference = None;
    let mut nonzero = 0;
    let mut modes = [0u32; 4];
    for scene in 0..scenes {
        let mut vocoder = Vocoder {
            parameters: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
            state: core::array::from_fn(|_| word(&raw, &mut cursor) as u16),
        };
        for frame in 0..frames {
            let busy = word(&raw, &mut cursor);
            let mut audio = VocoderFrame {
                samples: core::array::from_fn(|_| word(&raw, &mut cursor) as i32),
            };
            let output = vocoder
                .process(&mut audio, busy == 0, &tables)
                .map_err(|e| format!("Native vocoder scene{scene}/frame{frame}: {e:?}"))?;
            nonzero += u32::from(output.left.0 != 0 || output.right.0 != 0);
            modes[scene as usize % 4] += 1;
            for (kind, actual) in [
                vocoder
                    .parameters
                    .iter()
                    .map(|x| u32::from(*x))
                    .collect::<Vec<_>>(),
                vocoder.state.iter().map(|x| u32::from(*x)).collect(),
                audio.samples.iter().map(|x| *x as u32).collect(),
            ]
            .into_iter()
            .enumerate()
            {
                for (at, actual) in actual.into_iter().enumerate() {
                    let expected = word(&raw, &mut cursor);
                    if actual != expected {
                        errors[kind] += 1;
                        first_difference.get_or_insert(serde_json::json!({"kind":kind,"scene":scene,"frame":frame,"field":at,"expected":expected,"actual":actual}));
                    }
                }
            }
        }
    }
    if cursor != raw.len() || scenes != 256 || frames != 8 || nonzero == 0 {
        return Err("Incomplete or silent whole vocoder comparison".into());
    }
    let result = serde_json::json!({"passed":errors==[0,0,0],"errors_by_parameters_state_audio":errors,"first_difference":first_difference,
        "whole_original_vocoder_sample_calls":scenes*frames,"nonzero_output_pairs":nonzero,"mode_counts":modes,
        "parameter_words_compared":scenes*frames*PARAMETER_WORDS as u32,"state_words_compared":scenes*frames*STATE_WORDS as u32,"complete_frame_words_compared":scenes*frames*17,
        "original_output_words_used_as_native_input":false,"native_executes_firmware_instructions":false,"FXD03_audio_or_complete_native_instrument_verified":false});
    fs::write(
        root.join("runs/native-clone/vocoder-sample-parity.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    if errors != [0, 0, 0] {
        return Err("Native whole vocoder sample differs".into());
    }
    Ok(())
}
