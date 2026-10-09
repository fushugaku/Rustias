use radias_synth_application::VoiceRenderer;
use radias_synth_domain::pan::StereoFrame;
use radias_synth_domain::{
    Phase, Sample,
    envelope::EnvelopeLevel,
    filter::{FilterCoefficients, FilterState, ResonantFilter},
    fixed::{high_product, saturate},
    mixer::OscillatorMix,
    oscillator::Oscillator,
    pitch::PhaseIncrement,
    primary_oscillator::PrimaryRampOscillator,
    voice::{Voice, VoiceParameters},
    waveform::{ShapeParameters, Transfer},
};
use radias_synth_infrastructure::prepared::PreparedVoice;
use radias_synth_infrastructure::{firmware::MasterTables, wav};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().map(String::as_str).unwrap_or(".."));
    let name = args.get(1).map(String::as_str).unwrap_or("native-voice");
    let output = root.join("runs/native-clone");
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let raw = fs::read(output.join(format!("{name}-voice-inputs.bin")))?;
    if raw.is_empty() || !raw.len().is_multiple_of(696) {
        return Err("Voice reference length".into());
    }
    let word =
        |record: &[u8], i: usize| u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap());
    let first = &raw[..696];
    let param = |i: usize| word(first, 1 + i) as i16;
    let pair = |i: usize| (word(first, 1 + i) << 16) | word(first, 2 + i);
    let secondary = Oscillator::new(
        Phase(word(first, 162)),
        PhaseIncrement(pair(38)),
        (param(40) as i32 as u32).wrapping_shl(16),
        Transfer::CorrectedRamp,
        ShapeParameters {
            subtract_edge: param(43) != 0,
            edge_coefficient: param(44),
            waveform_control: param(45),
            gain: 32767,
        },
    );
    let mut voice = Voice {
        mixer_noise: Default::default(),
        previous_primary: Phase(0),
        primary: PrimaryRampOscillator {
            phase: Phase(word(first, 161)),
            offset: param(15),
        }
        .into(),
        secondary,
        filter: ResonantFilter {
            state: FilterState {
                first: pair(136) as i32,
                second: pair(138) as i32,
                post: [pair(140) as i32, pair(142) as i32],
            },
        },
        second_filter: Default::default(),
        waveshaper: Default::default(),
        envelope: EnvelopeLevel(param(126)),
        previous_secondary: Sample(0),
    };
    let mut rendered = vec![[Sample(0); 8]; word(first, 0) as usize];
    let mut errors = [0usize; 6];
    for (index, r) in raw.chunks_exact(696).enumerate() {
        let param = |i: usize| word(r, 1 + i) as i16;
        let pair = |i: usize| ((word(r, 1 + i) << 16) | word(r, 2 + i)) as i32;
        let c = VoiceParameters {
            routing: None,
            shaper: None,
            secondary_modulation: Default::default(),
            primary: radias_synth_infrastructure::prepared::parameters(r)?.primary,
            primary_pitch_code: param(2) as u16,
            mix: OscillatorMix {
                primary_gain: param(48),
                secondary_gain: param(50),
                noise_gain: param(52),
            },
            filter: FilterCoefficients {
                input_gain: param(55),
                feedback: pair(58),
                integrator_gain: pair(66),
                post_gain: param(69),
                post_feedback: param(71),
                mix: [param(73), param(75), param(77), param(79), param(81)],
            },
            envelope_target: param(125),
            envelope_rate: param(124),
            pan_position: saturate(
                high_product(param(127), word(r, 163) as i16)
                    + high_product(param(128), word(r, 164) as i16),
            ),
        };
        if c.mix.secondary_gain != 0 || c.mix.noise_gain != 0 {
            return Err("Voice fixture includes unqualified secondary/noise routing".into());
        }
        if param(1) as u16 != 0xbe88 || param(46) as u16 != 0xb468 {
            return Err("Unsupported fixture generator/filter".into());
        }
        let result = voice.next_sample(&table, c);
        let actual = [
            result.mixed.0,
            result.filtered.0,
            result.level as i32,
            result.amplified.0,
            result.stereo.left.0,
            result.stereo.right.0,
        ];
        let expected = [
            word(r, 165) as i32,
            word(r, 166) as i32,
            word(r, 167) as i16 as i32,
            word(r, 168) as i32,
            word(r, 169) as i32,
            word(r, 170) as i32,
        ];
        for i in 0..6 {
            if actual[i] != expected[i] {
                if errors[i] < 2 {
                    eprintln!("Voice {index} stage {i}: {} != {}", actual[i], expected[i]);
                }
                errors[i] += 1;
            }
        }
        if word(r, 0) as usize != rendered.len() {
            return Err("Voice/mix frame schedule discontinuity".into());
        }
        if word(r, 172) != 0 || word(r, 173) != 2 {
            return Err("Unsupported voice output bus".into());
        }
        let mut frame = [Sample(0); 8];
        frame[0] = Sample(saturate((result.stereo.left.0 as i64) << 5));
        frame[1] = Sample(saturate((result.stereo.right.0 as i64) << 5));
        rendered.push(frame);
    }
    let samples = rendered.len();
    wav::write_buses(
        &output.join(format!("{name}-rust-complete-mix.wav")),
        &rendered,
    )?;
    let plan = PreparedVoice::from_reference_parameters(&raw)?;
    let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
    let mut standalone = vec![StereoFrame::default(); plan.reference_voice_frames];
    for block in standalone.chunks_mut(127) {
        renderer.render(&table, &plan.events, block);
    }
    let mut frames = vec![[Sample(0); 8]; plan.reference_start_frame as usize];
    frames.extend(standalone.iter().map(|s| {
        [
            s.left,
            s.right,
            Sample(0),
            Sample(0),
            Sample(0),
            Sample(0),
            Sample(0),
            Sample(0),
        ]
    }));
    if frames != rendered {
        return Err("Autonomous block renderer differs from original voice pipeline".into());
    }
    wav::write_buses(
        &output.join(format!("{name}-rust-autonomous-mix.wav")),
        &frames,
    )?;
    let report = serde_json::json!({"voice_frames":raw.len()/696,"mix_frames":samples,"stage_mismatches":errors,"control_events":plan.events.len(),"autonomous_block_renderer_exact":true,"scope":"primary ramp voice, mixer, resonant filter, amplifier smoothing and stereo native buses","passed":errors.iter().all(|e|*e==0)});
    println!("{report}");
    fs::write(
        output.join(format!("{name}-voice-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors.iter().any(|n| *n != 0) {
        return Err("Native voice parity failed".into());
    }
    Ok(())
}
