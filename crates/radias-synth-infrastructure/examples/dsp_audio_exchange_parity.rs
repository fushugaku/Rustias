//! Direct native receive/frame-edit/transmit parity against original C55 calls.
use radias_synth_application::dsp_audio_exchange::DspAudioExchange;
use radias_synth_domain::{
    Sample,
    dsp_audio_exchange::{ADC_WORDS, BLOCK_WORDS, DOUBLE_BUFFER_WORDS},
};
use std::{env, fs, path::Path};
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn word(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn array<const N: usize>(&mut self) -> [u16; N] {
        core::array::from_fn(|_| self.word() as u16)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    let root = args.get(1).map_or(Path::new("."), Path::new);
    let variant = args.get(2).map_or("correct", String::as_str);
    if !matches!(variant, "correct" | "wrong-half" | "wrong-word-order") {
        return Err("unknown exchange negative control".into());
    }
    let bytes = fs::read(root.join("runs/native-clone/dsp-audio-exchange-original.bin"))?;
    let mut input = Reader {
        bytes: &bytes,
        at: 0,
    };
    if input.word() != 0x46584231 {
        return Err("wrong exchange corpus magic".into());
    }
    let count = input.word() as usize;
    let mut errors = 0usize;
    let mut cases = [[0usize; 3]; 2];
    let mut nonzero = 0usize;
    for _ in 0..count {
        let chip = input.word() as usize;
        let _scene = input.word();
        let flag = input.word() as u16;
        let adc = input.array::<ADC_WORDS>();
        let received = input.array::<DOUBLE_BUFFER_WORDS>();
        let _initial = input.array::<BLOCK_WORDS>();
        let mut transmitted = input.array::<DOUBLE_BUFFER_WORDS>();
        let expected_received = input.array::<BLOCK_WORDS>();
        let mut exchange = DspAudioExchange::default();
        let supplied_flag = if variant == "wrong-half" {
            u16::from(flag == 0)
        } else {
            flag
        };
        exchange.receive(supplied_flag, &adc, &received);
        if variant == "wrong-word-order" {
            for frame in 0..4 {
                let incorrect = exchange
                    .frame(frame)
                    .map_err(|_| "legal frame rejected")?
                    .map(|sample| Sample((sample.0 as u32).rotate_left(16) as i32));
                exchange
                    .replace_frame(frame, incorrect)
                    .map_err(|_| "legal replacement rejected")?;
            }
        }
        for (actual, expected) in exchange.working_words().iter().zip(expected_received) {
            errors += usize::from(*actual != expected);
        }
        for frame in 0..4 {
            let logical = exchange.frame(frame).map_err(|_| "legal frame rejected")?;
            for (sample, value) in logical.iter().enumerate() {
                let at = frame * 32 + 2 * sample;
                let expected =
                    (u32::from(expected_received[at + 1]) << 16) | u32::from(expected_received[at]);
                errors += usize::from(value.0 as u32 != expected);
            }
        }
        let before = *exchange.working_words();
        if exchange.frame(4).is_ok()
            || exchange.replace_frame(4, [Sample(0); 16]).is_ok()
            || *exchange.working_words() != before
        {
            return Err("invalid frame rejection mutated the block".into());
        }
        let edited = input.word() as usize;
        let data = core::array::from_fn(|_| Sample(input.word() as i32));
        exchange
            .replace_frame(edited, data)
            .map_err(|_| "legal replacement rejected")?;
        let expected_working = input.array::<BLOCK_WORDS>();
        for (actual, expected) in exchange.working_words().iter().zip(expected_working) {
            errors += usize::from(*actual != expected);
        }
        exchange.transmit(supplied_flag, &mut transmitted);
        let expected_transmitted = input.array::<DOUBLE_BUFFER_WORDS>();
        for (actual, expected) in transmitted.iter().zip(expected_transmitted) {
            errors += usize::from(*actual != expected);
            nonzero += usize::from(*actual != 0);
        }
        cases[chip][match flag {
            0 => 0,
            1 => 1,
            0xffff => 2,
            _ => return Err("unexpected flag profile".into()),
        }] += 1;
    }
    let passed = errors == 0
        && input.at == bytes.len()
        && count == 1536
        && cases == [[256; 3]; 2]
        && nonzero > 0;
    let report = serde_json::json!({"passed":passed,"errors":errors,"negative_control":variant,"complete_original_calls":count*2,"four_frame_blocks":count,"processed_sample_frames":count*4,"physical_memory_words_compared":count*512,"logical_samples_compared":count*64,"cases":cases,"corpus_bytes":bytes.len(),"physical_working_halfword_order_qualified":passed,"frame_edit_and_invalid_index_rejection_qualified":passed,"native_interprets_firmware":false,"ASIC_bit_clock_alignment_or_timing_qualified":false,"FXD03_arithmetic_or_wet_audio_qualified":false,"installed_application_replaced":false});
    let report_name = if variant == "correct" {
        "dsp-audio-exchange-parity.json".to_owned()
    } else {
        format!("dsp-audio-exchange-{variant}.json")
    };
    fs::write(
        root.join("runs/native-clone").join(report_name),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("native exchange differs from original C55 execution".into());
    }
    Ok(())
}
