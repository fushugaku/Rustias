//! Complete original C55 receive/vocoder/transmit composition, including ADC tail.
use radias_synth_application::{dsp_audio_exchange::DspAudioExchange, vocoder::VocoderRenderer};
use radias_synth_domain::{
    dsp_audio_exchange::{ADC_WORDS, BLOCK_WORDS, DOUBLE_BUFFER_WORDS},
    vocoder::{InterpolationTables, Vocoder, VocoderFrame},
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
    fn words<const N: usize>(&mut self) -> [u16; N] {
        core::array::from_fn(|_| self.word() as u16)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    let root = args.get(1).map_or(Path::new("."), Path::new);
    let wrong = args.get(2).is_some_and(|v| v == "copy-view");
    let bytes = fs::read(root.join("runs/native-clone/vocoder-exchange-original.bin"))?;
    let mut r = Reader {
        bytes: &bytes,
        at: 0,
    };
    if r.word() != 0x45565831 || r.word() != 2 || r.word() != 64 || r.word() != 4 {
        return Err("wrong composed corpus header".into());
    }
    let mut errors = [0usize; 5];
    let mut first = None;
    let mut nonzero = 0usize;
    let mut cross_frame = 0usize;
    for chip in 0..2 {
        if r.word() != chip {
            return Err("wrong DSP order".into());
        }
        let offsets = r.words::<39>();
        let tables = InterpolationTables {
            scalar_offsets: offsets[..38].try_into().unwrap(),
            wide_offset: offsets[38],
        };
        for scene in 0..64 {
            if r.word() != scene {
                return Err("wrong scene order".into());
            }
            let mut renderer = VocoderRenderer {
                processor: Vocoder {
                    parameters: r.words::<352>(),
                    state: r.words::<300>(),
                },
                tables: tables.clone(),
            };
            for block in 0..4 {
                let flag = r.word() as u16;
                let adc = r.words::<ADC_WORDS>();
                let incoming = r.words::<DOUBLE_BUFFER_WORDS>();
                let _initial = r.words::<BLOCK_WORDS>();
                let mut outgoing = r.words::<DOUBLE_BUFFER_WORDS>();
                let mut exchange = DspAudioExchange::default();
                exchange.receive(flag, &adc, &incoming);
                for (actual, expected) in exchange
                    .working_words()
                    .iter()
                    .zip(r.words::<BLOCK_WORDS>())
                {
                    errors[0] += usize::from(*actual != expected);
                }
                for frame in 0..4 {
                    let interpolate = r.word() == 0;
                    let output = if wrong {
                        let logical = exchange.frame(frame).map_err(|_| "invalid frame")?;
                        let following = if frame < 3 {
                            exchange
                                .frame(frame + 1)
                                .map_err(|_| "invalid following frame")?[0]
                        } else {
                            exchange.trailing_sample()
                        };
                        let mut incorrect = VocoderFrame {
                            samples: core::array::from_fn(|n| {
                                if n < 16 { logical[n].0 } else { following.0 }
                            }),
                        };
                        let output = renderer
                            .processor
                            .process(&mut incorrect, interpolate, &renderer.tables)
                            .map_err(|e| format!("{e:?}"))?;
                        exchange
                            .replace_vocoder_frame(frame, &incorrect)
                            .map_err(|_| "invalid replacement")?;
                        output
                    } else {
                        renderer
                            .render_exchange_frame(&mut exchange, frame, interpolate)
                            .map_err(|e| format!("{e:?}"))?
                    };
                    nonzero += usize::from(output.left.0 != 0 || output.right.0 != 0);
                    cross_frame += usize::from(
                        renderer.processor.parameters[0xb4] >= 32
                            || renderer.processor.parameters[0xb5] >= 32,
                    );
                    for (kind, actual) in [
                        renderer.processor.parameters.to_vec(),
                        renderer.processor.state.to_vec(),
                        exchange.working_words().to_vec(),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        for (field, actual) in actual.into_iter().enumerate() {
                            let expected = r.word();
                            if u32::from(actual) != expected {
                                errors[kind + 1] += 1;
                                first.get_or_insert(serde_json::json!({"chip":chip,"scene":scene,"block":block,"frame":frame,"kind":kind+1,"field":field,"expected":expected,"actual":actual}));
                            }
                        }
                    }
                    for (field, actual) in [
                        (exchange.trailing_sample().0 as u32).rotate_left(16),
                        output.left.0 as u32,
                        output.right.0 as u32,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let expected = r.word();
                        if actual != expected {
                            errors[4] += 1;
                            first.get_or_insert(serde_json::json!({"chip":chip,"scene":scene,"block":block,"frame":frame,"kind":4,"field":field,"expected":expected,"actual":actual}));
                        }
                    }
                }
                exchange.transmit(flag, &mut outgoing);
                for (actual, expected) in outgoing.iter().zip(r.words::<DOUBLE_BUFFER_WORDS>()) {
                    errors[3] += usize::from(*actual != expected);
                }
            }
        }
    }
    let passed = errors == [0; 5] && r.at == bytes.len() && nonzero > 0 && cross_frame > 0;
    let report = serde_json::json!({"passed":passed,"errors_receive_parameters_state_memory_output":errors,"first_difference":first,"composed_four_frame_blocks":512,"original_buffer_calls":1024,"original_vocoder_sample_calls":2048,"nonzero_routed_pairs":nonzero,"cross_frame_route_samples":cross_frame,"negative_control_copy_view":wrong,"actual_vocoder_frame_base_even":true,"raw_buffer_and_history_parity_qualified":passed,"native_interprets_firmware":false,"whole_machine_controller_DMA_timing_qualified":false,"FXD03_arithmetic_or_wet_audio_qualified":false});
    let name = if wrong {
        "vocoder-exchange-copy-view-negative.json"
    } else {
        "vocoder-exchange-parity.json"
    };
    fs::write(
        root.join("runs/native-clone").join(name),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("native composed vocoder exchange differs".into());
    }
    Ok(())
}
