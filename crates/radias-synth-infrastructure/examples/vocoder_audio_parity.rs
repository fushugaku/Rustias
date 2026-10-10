use radias_synth_domain::{
    Sample,
    vocoder::{InterpolationTables, Vocoder, VocoderFrame},
};
use radias_synth_infrastructure::wav;
use std::{fs, hint::black_box, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let parameters = fs::read(root.join("runs/native-clone/vocoder-audio-parameters.bin"))?;
    let raw = fs::read(root.join("runs/native-clone/vocoder-audio-inputs.bin"))?;
    if parameters.len() != 704 || raw.is_empty() || raw.len() % 68 != 0 {
        return Err("Incomplete continuous input/profile".into());
    }
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let origin = u16::from_be_bytes(image[..2].try_into().unwrap()) as usize;
    let source = |address: usize| {
        u16::from_be_bytes(
            image[2 + 2 * (address - origin)..4 + 2 * (address - origin)]
                .try_into()
                .unwrap(),
        )
    };
    let tables = InterpolationTables {
        scalar_offsets: core::array::from_fn(|i| source(0x494b + i)),
        wide_offset: source(0x4971),
    };
    let prototype = Vocoder {
        parameters: core::array::from_fn(|i| {
            u16::from_le_bytes(parameters[2 * i..2 * i + 2].try_into().unwrap())
        }),
        state: [0; 300],
    };
    let frames: Vec<_> = raw
        .chunks_exact(68)
        .map(|b| VocoderFrame {
            samples: core::array::from_fn(|i| {
                i32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
            }),
        })
        .collect();
    let mut vocoder = prototype.clone();
    let mut audio = Vec::with_capacity(frames.len());
    for input in &frames {
        let mut frame = input.clone();
        let result = vocoder
            .process(&mut frame, true, &tables)
            .map_err(|e| format!("Vocoder frame: {e:?}"))?;
        audio.push([result.left, result.right]);
    }
    wav::write_stereo(
        &root.join("runs/native-clone/vocoder-audio-native.wav"),
        &audio,
    )?;
    let original = fs::read(root.join("runs/native-clone/vocoder-audio-original-final.bin"))?;
    let actual: Vec<_> = vocoder
        .parameters
        .iter()
        .chain(&vocoder.state)
        .flat_map(|x| u32::from(*x).to_le_bytes())
        .collect();
    if original != actual {
        return Err("Complete continuous vocoder final state differs".into());
    }
    let mut bus_renderer = radias_synth_application::vocoder::VocoderRenderer {
        processor: prototype.clone(),
        tables: tables.clone(),
    };
    for (index, frame) in frames.iter().enumerate() {
        let input = radias_synth_domain::pan::StereoFrame {
            left: Sample(frame.samples[0]),
            right: Sample(frame.samples[1]),
        };
        if radias_synth_domain::vocoder::VocoderFrame::from_buses(frame.buses(), input)
            != *frame
        {
            return Err("Fixture contains additional unmodeled frame inputs".into());
        }
        let output = bus_renderer
            .render_buses(frame.buses(), input, true)
            .map_err(|e| format!("Bus frame: {e:?}"))?;
        // This source program routes its vocoder output to Master timbre one.
        if [output[0].left, output[0].right] != audio[index] {
            return Err("Instrument bus boundary differs from original audio".into());
        }
        for (timbre, bus) in output.iter().enumerate().skip(1) {
            if *bus != frame.buses()[timbre] {
                return Err("Vocoder changed an unrelated timbre bus".into());
            }
        }
    }
    if bus_renderer.processor != vocoder {
        return Err("Bus boundary changed continuous vocoder histories".into());
    }
    let mut callback_partitions = Vec::new();
    for length in [1, 16, 64, 128, 512] {
        let mut renderer = radias_synth_application::vocoder::VocoderRenderer {
            processor: prototype.clone(),
            tables: tables.clone(),
        };
        let mut input = frames.clone();
        let updates = vec![true; input.len()];
        let mut output = vec![radias_synth_domain::pan::StereoFrame::default(); input.len()];
        for (index, chunk) in input.chunks_mut(length).enumerate() {
            let first = index * length;
            renderer
                .render_block(
                    chunk,
                    &updates[first..first + chunk.len()],
                    &mut output[first..first + chunk.len()],
                )
                .map_err(|e| format!("Callback: {e:?}"))?;
        }
        if renderer.processor != vocoder
            || output
                .iter()
                .zip(&audio)
                .any(|(a, b)| [a.left, a.right] != *b)
        {
            return Err("Vocoder callback partition changes complete sound/state".into());
        }
        callback_partitions.push(length);
    }
    let mut vocoder = prototype;
    vocoder.parameters[15] = 0;
    let count = 480_000usize;
    let start = Instant::now();
    let mut check = 0i32;
    for at in 0..count {
        let mut frame = frames[at % frames.len()].clone();
        let output = vocoder
            .process(black_box(&mut frame), true, black_box(&tables))
            .map_err(|e| format!("Benchmark: {e:?}"))?;
        check = check.rotate_left(1) ^ output.left.0 ^ output.right.0;
    }
    black_box(check);
    let seconds = start.elapsed().as_secs_f64();
    let result = serde_json::json!({"complete_continuous_frames":audio.len(),"nonzero_output_pairs":audio.iter().filter(|s|s!=&&[Sample(0),Sample(0)]).count(),
        "final_parameters_and_histories_exact":true,"native_executes_firmware_instructions":false,
        "callback_partition_lengths_verified":callback_partitions,
        "benchmark_frames":count,"benchmark_seconds":seconds,"kernel_speed_times_realtime":(count as f64/48000.0)/seconds,
        "benchmark_scope":"Whole native vocoder sample body with live analysis, all16 carrier bands and parameter interpolation; excludes device/whole-instrument/FXD03",
        "FXD03_or_complete_native_instrument_verified":false});
    fs::write(
        root.join("runs/native-clone/vocoder-audio-native.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
