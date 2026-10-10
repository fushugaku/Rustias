//! Production full-frame API compared with original C55 mix/vocoder stages.
//! Native dry voice words are declared stage inputs, not a whole-voice oracle.
use radias_synth_application::{dsp_audio_exchange::DspAudioExchange, vocoder::VocoderRenderer};
use radias_synth_domain::vocoder::{Vocoder, VocoderFrame};
use radias_synth_infrastructure::{
    standalone::StandaloneSynth, synthesizer::Command, vocoder_tables,
};
use std::{env, fs, io::Write, path::Path};
fn word(out: &mut impl Write, v: u32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn word(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.data[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
}
fn rig() -> StandaloneSynth {
    let mut instrument = StandaloneSynth::new();
    for timbre in 0..4 {
        assert!(instrument.control(timbre, 71, 1));
        assert!(instrument.control(timbre, 0, i32::from(timbre)));
        for note in 0..6 {
            instrument
                .engine
                .apply(Command::Note(timbre, 48 + 3 * timbre + note, 96));
        }
    }
    assert_eq!(instrument.engine.active_count(), 24);
    instrument
}
fn generate(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let raw = fs::read(root.join("runs/native-clone/vocoder-audio-parameters.bin"))?;
    if raw.len() != 704 {
        return Err("wrong original parameter extent".into());
    }
    let mut inputs =
        fs::File::create(root.join("runs/native-clone/native-sample-frame-input.bin"))?;
    let mut actual =
        fs::File::create(root.join("runs/native-clone/native-sample-frame-native.bin"))?;
    for output in [&mut inputs, &mut actual] {
        word(output, 0x4e534631)?;
        word(output, 16)?;
        word(output, 128)?;
    }
    let mut seed = 0x4e534631u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for scene in 0..16 {
        let mut candidate = rig();
        let mut source = rig();
        let mut processor = Vocoder {
            parameters: core::array::from_fn(|n| u16::from_le_bytes([raw[2 * n], raw[2 * n + 1]])),
            state: [0; 300],
        };
        processor.parameters[0] = u16::from(scene % 4 != 0);
        processor.parameters[15] = (scene % 3) as u16;
        processor.parameters[0xb4] = if scene >= 14 {
            32
        } else {
            8 + 4 * (scene % 4) as u16
        };
        processor.parameters[0xb5] = processor.parameters[0xb4] + if scene >= 14 { 1 } else { 2 };
        for value in processor.parameters.iter().chain(&processor.state) {
            word(&mut inputs, u32::from(*value))?;
        }
        candidate.engine.set_vocoder(Some(Box::new(VocoderRenderer {
            processor,
            tables: vocoder_tables::interpolation(),
        })));
        let mut invalid = DspAudioExchange::default();
        let before = candidate.engine.vocoder().unwrap().processor.clone();
        if candidate
            .engine
            .sample_exchange_frame(&mut invalid, 4, true)
            .is_ok()
            || candidate.engine.vocoder().unwrap().processor != before
        {
            return Err("invalid frame advanced state".into());
        }
        for frame in 0..128 {
            if frame == 96 {
                for timbre in 0..4 {
                    for note in 0..6 {
                        for instrument in [&mut candidate, &mut source] {
                            instrument.engine.apply(Command::Note(
                                timbre,
                                48 + 3 * timbre + note,
                                0,
                            ));
                        }
                    }
                }
            }
            let busy = (scene + frame) % 3;
            let mut data = VocoderFrame {
                samples: core::array::from_fn(|_| next() as i32),
            };
            word(&mut inputs, busy as u32)?;
            for value in data.samples {
                word(&mut inputs, value as u32)?;
            }
            let buses = source.engine.sample_buses()[0];
            for bus in buses {
                word(&mut inputs, bus.left.0 as u32)?;
                word(&mut inputs, bus.right.0 as u32)?;
            }
            if frame % 2 == 0 {
                candidate
                    .engine
                    .sample_frame(&mut data, busy == 0)
                    .map_err(|e| format!("{e:?}"))?;
            } else {
                let mut exchange = DspAudioExchange::default();
                let index = frame % 4;
                exchange
                    .replace_vocoder_frame(index, &data)
                    .map_err(|_| "invalid input frame")?;
                candidate
                    .engine
                    .sample_exchange_frame(&mut exchange, index, busy == 0)
                    .map_err(|e| format!("{e:?}"))?;
                data = exchange
                    .vocoder_frame(index)
                    .map_err(|_| "invalid output frame")?;
            }
            if candidate.engine.active_count() != source.engine.active_count() {
                return Err("full-frame API advanced allocation differently".into());
            }
            let state = &candidate.engine.vocoder().unwrap().processor;
            for value in state.parameters.iter().chain(&state.state) {
                word(&mut actual, u32::from(*value))?;
            }
            for value in data.samples {
                word(&mut actual, value as u32)?;
            }
        }
    }
    inputs.flush()?;
    actual.flush()?;
    println!(
        "2048 production frames generated through direct and exchange APIs; native dry voice words are declared inputs"
    );
    Ok(())
}
fn compare(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let a = fs::read(root.join("runs/native-clone/native-sample-frame-native.bin"))?;
    let b = fs::read(root.join("runs/native-clone/native-sample-frame-original.bin"))?;
    let mut actual = Reader { data: &a, at: 0 };
    let mut expected = Reader { data: &b, at: 0 };
    for value in [0x4e534631, 16, 128] {
        if actual.word() != value || expected.word() != value {
            return Err("wrong full-frame header".into());
        }
    }
    let mut errors = [0usize; 3];
    let mut first = None;
    let mut nonzero = 0;
    for scene in 0..16 {
        for frame in 0..128 {
            for (kind, count) in [352, 300, 17].into_iter().enumerate() {
                for field in 0..count {
                    let a = actual.word();
                    let b = expected.word();
                    if a != b {
                        errors[kind] += 1;
                        first.get_or_insert(serde_json::json!({"scene":scene,"frame":frame,"kind":kind,"field":field,"expected":b,"actual":a}));
                    }
                    if kind == 2 && a != 0 {
                        nonzero += 1;
                    }
                }
            }
        }
    }
    let passed = errors == [0; 3] && actual.at == a.len() && expected.at == b.len() && nonzero > 0;
    let report = serde_json::json!({"passed":passed,"errors_parameters_history_frame":errors,"first_difference":first,"production_sample_frames":2048,"full_frame_direct_calls":1024,"full_frame_exchange_calls":1024,"initial_voices_per_scene":24,"timbres":4,"checked_frame_lanes":17,"disabled_vocoder_scene_count":4,"cross_frame_route_scene_count":2,"nonzero_frame_words":nonzero,"original_mix_and_vocoder_stages_executed":true,"native_dry_voice_words_are_declared_reference_inputs":true,"standalone_controller_tables_are_analytic":true,"whole_native_voice_or_instrument_parity_qualified":false,"FXD03_arithmetic_or_wet_audio_qualified":false,"native_interprets_firmware":false});
    fs::write(
        root.join("runs/native-clone/native-sample-frame-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("production full-frame path differs from original stages".into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    let root = Path::new(args.get(1).ok_or("repository required")?);
    match args.get(2).map(String::as_str) {
        Some("generate") => generate(root),
        Some("compare") => compare(root),
        _ => Err("expected generate or compare".into()),
    }
}
