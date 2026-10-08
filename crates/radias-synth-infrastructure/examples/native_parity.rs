//! Compare direct synthesis algorithms to unchanged original DSP execution.
use radias_synth_application::{compile_pitch, render_oscillator_block, render_waveform_block};
use radias_synth_domain::{
    Phase, Sample,
    oscillator::Oscillator,
    waveform::{Transfer, WaveformFrame, WaveformTable},
};
use radias_synth_infrastructure::{firmware::MasterTables, oracle::decode_waveform_records, wav};
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn compare(
    table: &WaveformTable,
    input: &[WaveformFrame],
    expected: &[Sample],
) -> Result<Vec<Sample>, String> {
    let mut output = vec![Sample(0); input.len()];
    render_waveform_block(table, input, &mut output).map_err(|_| "Block length")?;
    if let Some(index) = output.iter().zip(expected).position(|(a, b)| a != b) {
        return Err(format!(
            "Sample {index}: {} != {}; {:?}",
            output[index].0, expected[index].0, input[index]
        ));
    }
    // Irregular callback sizes must produce exactly the same samples.
    let mut split = vec![Sample(0); input.len()];
    let mut cursor = 0;
    for size in [1, 17, 64, 127, 256, 511].into_iter().cycle() {
        if cursor == input.len() {
            break;
        }
        let end = (cursor + size).min(input.len());
        render_waveform_block(table, &input[cursor..end], &mut split[cursor..end])
            .map_err(|_| "Split block length")?;
        cursor = end;
    }
    if split != output {
        return Err("Callback block partition changes output".into());
    }
    Ok(output)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
    let output = root.join("runs/native-clone");
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let image = MasterTables::from_host_stream(&source)?;
    let pitches = image.pitch()?;
    let waves = image.waveform()?;
    let scales = fs::read(output.join("original-scale.bin"))?;
    if scales.len() != 65536 * 12 {
        return Err("Scaling corpus length".into());
    }
    for (index, record) in scales.chunks_exact(12).enumerate() {
        let actual = radias_synth_domain::waveform::scale(
            Sample(word(record, 0) as i32),
            word(record, 4) as i16,
        );
        if actual.0 != word(record, 8) as i32 {
            return Err(format!("Original Q31/Q15 scaling differs at record {index}").into());
        }
    }
    let reference = fs::read(output.join("original-pitch.bin"))?;
    if reference.len() != 32768 * 4 {
        return Err("Pitch corpus length".into());
    }
    for code in 0..32768 {
        let actual = compile_pitch(&pitches, code as u16).unwrap().0;
        let expected = word(&reference, code * 4);
        if actual != expected {
            return Err(format!("Pitch {code}: {actual} != {expected}").into());
        }
    }
    let (input, expected) =
        decode_waveform_records(&fs::read(output.join("original-shapers.bin"))?)?;
    if input.len() != 65536 {
        return Err("Waveform corpus length".into());
    }
    compare(&waves, &input, &expected)?;
    let mut nonzero = [0usize; 4];
    for (frame, sample) in input.iter().zip(&expected) {
        let n = match frame.transfer {
            Transfer::CorrectedRamp => 0,
            Transfer::Pulse => 1,
            Transfer::ParabolicSine => 2,
            Transfer::FoldedTriangle => 3,
        };
        if sample.0 != 0 {
            nonzero[n] += 1;
        }
    }
    if nonzero.iter().any(|n| *n < 4000) {
        return Err("Oracle replaced by silence".into());
    }
    let mut live = Vec::new();
    for prefix in args.iter().skip(1) {
        let (inputs, expected) =
            decode_waveform_records(&fs::read(output.join(format!("{prefix}-stream.bin")))?)?;
        if !expected.iter().any(|s| s.0 != 0) {
            return Err("Live waveform observation is silent".into());
        }
        let samples = compare(&waves, &inputs, &expected)?;
        wav::write_mono(
            &output.join(format!("{prefix}-rust-waveform.wav")),
            &samples,
        )?;
        let state: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(format!("{prefix}-oscillator.json")))?)?;
        let pitch_code = state["pitch_code"].as_u64().ok_or("Pitch code absent")?;
        let increment =
            compile_pitch(&pitches, u16::try_from(pitch_code)?).ok_or("Invalid pitch code")?;
        if Some(increment.0 as u64) != state["phase_increment"].as_u64() {
            return Err("Observed pitch and original table differ".into());
        }
        let first = inputs[0];
        let edge_offset = first.edge_phase.wrapping_sub(first.phase) as u32;
        let mut oscillator = Oscillator::new(
            Phase(first.phase as u32),
            increment,
            edge_offset,
            first.transfer,
            first.parameters,
        );
        for (index, frame) in inputs.iter().enumerate() {
            if frame.phase as u32 != oscillator.phase().0
                || frame.edge_phase.wrapping_sub(frame.phase) as u32 != edge_offset
                || frame.transfer != first.transfer
                || frame.parameters != first.parameters
            {
                return Err(format!(
                    "Original stream changes the prepared oscillator at frame {index}"
                )
                .into());
            }
            let actual = oscillator.next_sample(&waves);
            if actual != expected[index] {
                return Err(format!("Autonomous oscillator sample {index} differs").into());
            }
        }
        let mut continuous = vec![Sample(0); expected.len()];
        let mut oscillator = Oscillator::new(
            Phase(first.phase as u32),
            increment,
            edge_offset,
            first.transfer,
            first.parameters,
        );
        for block in continuous.chunks_mut(127) {
            render_oscillator_block(&waves, &mut oscillator, block);
        }
        if continuous != expected {
            return Err("Stateful oscillator callback partition differs".into());
        }
        wav::write_mono(
            &output.join(format!("{prefix}-rust-oscillator.wav")),
            &continuous,
        )?;
        let mut changed = samples.clone();
        let middle = changed.len() / 2;
        changed[middle].0 ^= 1;
        if changed == expected {
            return Err("One-bit negative control failed".into());
        }
        wav::write_mono(&output.join(format!("{prefix}-one-bit.wav")), &changed)?;
        live.push(serde_json::json!({"name":prefix,"samples":samples.len(),"mismatches":0,
            "nonzero_samples":samples.iter().filter(|s|s.0!=0).count(),"block_partition_exact":true,
            "autonomous_phase_and_pitch_exact":true,"pitch_code":pitch_code,"phase_increment":increment.0}));
    }
    // Component throughput only: 24 voices, both oscillators, 256-frame blocks.
    // A whole synth with filters/modulation/effects is not being timed here.
    const BLOCK: usize = 256;
    const OSCILLATORS: usize = 48;
    let mut buffer = [Sample(0); BLOCK];
    let transfers = [
        Transfer::CorrectedRamp,
        Transfer::Pulse,
        Transfer::ParabolicSine,
        Transfer::FoldedTriangle,
    ];
    let mut oscillators: [Oscillator; OSCILLATORS] = std::array::from_fn(|i| {
        Oscillator::new(
            Phase((i as u32).wrapping_mul(0x1234_5678)),
            compile_pitch(&pitches, ((36 + i) * 256) as u16).unwrap(),
            380043264,
            transfers[i % transfers.len()],
            radias_synth_domain::waveform::ShapeParameters {
                subtract_edge: true,
                edge_coefficient: 12778,
                waveform_control: 23053,
                gain: 32767,
            },
        )
    });
    let mut times = Vec::with_capacity(512);
    let start = Instant::now();
    for _ in 0..512 {
        let frame_start = Instant::now();
        for oscillator in &mut oscillators {
            render_oscillator_block(
                black_box(&waves),
                black_box(oscillator),
                black_box(&mut buffer),
            );
            black_box(&buffer);
        }
        times.push(frame_start.elapsed().as_secs_f64());
    }
    let seconds = start.elapsed().as_secs_f64();
    times.sort_by(f64::total_cmp);
    let deadline = BLOCK as f64 / 48000.0;
    let result = serde_json::json!({"pitch_cases":32768,"scaling_cases":65536,"shaper_cases":input.len(),
        "shaper_mismatches":[0,0,0,0],"nonzero_reference_samples":nonzero,
        "live_waveform_streams":live,"direct_algorithms":true,"cpu_emulation_in_renderer":false,
        "ddd":{"domain":"no_std, no dependencies, fixed-point synthesis","application":"no_std, pitch compilation and block rendering",
            "infrastructure":"original tables, WAV and oracle input adapters"},
        "component_benchmark":{"oscillators":OSCILLATORS,"block_frames":BLOCK,"blocks":512,"seconds":seconds,
            "realtime_ratio":(512*BLOCK) as f64/48000.0/seconds,"callback_deadline_ms":deadline*1000.0,
            "p99_block_ms":times[506]*1000.0,"worst_block_ms":times[511]*1000.0,
            "deadline_misses":times.iter().filter(|t|**t>deadline).count(),
            "autonomous_oscillators":true,"waveform_functions":4,
            "scope":"waveform component, not complete native synth or device latency"},
        "scope":"original pitch and waveform functions; full patch/controller/voice/filter/FX parity remains unverified"});
    println!("{result}");
    fs::write(
        output.join("parity.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(())
}
