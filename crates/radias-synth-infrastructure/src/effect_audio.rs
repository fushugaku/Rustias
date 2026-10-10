//! Stored-program and immutable-table adapter for audible native effects.
//! The backend is explicitly unqualified against original FXD03 audio.
use radias_synth_application::effect_audio::{EFFECT_SLOTS, EffectAudioRack, MASTER_EFFECT_SLOT};
use radias_synth_domain::{
    delay_time::{DelayClock, DelayTimeState},
    effect_audio::{
        BiquadCoefficients, DELAY_FRAMES, EffectAudioProgram, EffectAudioSettings, EffectLfo,
    },
    effect_control::{EffectKind, EffectMix, MixContext},
    program::Program,
};
#[path = "effect_audio_data.rs"]
mod data;
pub const ORIGINAL_AUDIO_PARITY_QUALIFIED: bool = false;
#[derive(Clone, Copy, Debug)]
pub struct EffectProperty {
    pub name: &'static str,
    pub default: u8,
    pub minimum: i16,
    pub maximum: i16,
    pub zero: u8,
}
impl EffectProperty {
    pub const EMPTY: Self = Self {
        name: "",
        default: 0,
        minimum: 0,
        maximum: 0,
        zero: 0,
    };
}
#[derive(Clone, Copy, Debug)]
pub struct EffectDefinition {
    pub name: &'static str,
    pub count: u8,
    pub properties: [EffectProperty; 20],
}
pub fn definition(kind: u8, master: bool) -> Option<&'static EffectDefinition> {
    let definition = data::CATALOG[usize::from(master)].get(usize::from(kind))?;
    #[cfg(all(feature = "web-expanded", target_arch = "wasm32"))]
    if master && kind == 11 {
        // Original MFX has six reverb types; the insert range has three.
        static MASTER_REVERB: std::sync::OnceLock<EffectDefinition> = std::sync::OnceLock::new();
        return Some(MASTER_REVERB.get_or_init(|| {
            let mut d = *definition;
            d.properties[1].maximum = 5;
            d
        }));
    }
    Some(definition)
}
pub fn default_program(kind: u8, master: bool) -> Result<EffectAudioProgram, &'static str> {
    let d = definition(kind, master).ok_or("Effect type outside original catalog")?;
    Ok(EffectAudioProgram {
        kind,
        master,
        enabled: kind != 0,
        parameters: d.properties.map(|p| p.default),
    })
}
pub fn programs_from_stored(program: &Program) -> [EffectAudioProgram; EFFECT_SLOTS] {
    let native = radias_synth_domain::effect_audio::stored_effects(program);
    core::array::from_fn(|i| {
        if i < 8 {
            native[i]
        } else if i == MASTER_EFFECT_SLOT {
            native[8]
        } else {
            EffectAudioProgram::default()
        }
    })
}
pub fn prepare_rack(
    programs: [EffectAudioProgram; EFFECT_SLOTS],
    tempo: u16,
) -> Result<Box<EffectAudioRack>, String> {
    let mut settings = [EffectAudioSettings::default(); EFFECT_SLOTS];
    for (slot, p) in programs.into_iter().enumerate() {
        if p.master != (slot == MASTER_EFFECT_SLOT) {
            return Err("Effect bank/slot mismatch".into());
        }
        settings[slot] = compile(p, tempo).map_err(|e| format!("FX {}: {e}", slot + 1))?;
    }
    let mut rack = Box::new(EffectAudioRack::new(settings));
    rack.set_tempo(tempo);
    Ok(rack)
}
/// Browser value labels use these same immutable native control tables.
pub fn display_tables() -> serde_json::Value {
    let delay = data::delay_time();
    serde_json::json!({
        "eqHz": data::equalizer().frequency.to_vec(),
        "freeRatio": delay.free_ratio.to_vec(), "syncRatio": delay.sync_ratio.to_vec(),
        "lcrMs": delay.lcr_milliseconds.to_vec(), "stereoMs": delay.stereo_milliseconds.to_vec(),
        "modMonoMs": data::MOD_MONO_MS.to_vec(), "modStereoMs": data::MOD_STEREO_MS.to_vec(),
        "chorusTenthsMs": data::CHORUS_TENTHS_MS.to_vec(),
        "lfoHz": data::LFO_FREQUENCY.iter().map(|&v| f64::from(v) * 1000.0 / 4_294_967_296.0).collect::<Vec<_>>(),
        "ringHz": data::RING_FREQUENCY.iter().map(|&v| f64::from(v) * 12000.0 / 4194303.0).collect::<Vec<_>>(),
        "decimatorHz": data::DECIMATOR_RATE.iter().map(|&v| f64::from(v) * 48_000.0 / 4_194_304.0).collect::<Vec<_>>(),
        "largeReverb": data::REVERB_LARGE_INDEX.to_vec(), "smallReverb": data::REVERB_SMALL_INDEX.to_vec(),
        "preDelayMs": data::ER_PRE_DELAY.to_vec(), "earlyMs": data::ER_SIZE.to_vec(),
        "inverseRatio": data::LIMITER_INVERSE_RATIO.to_vec(),
        "attackTenthsMs": data::ATTACK_TENTHS_MS.to_vec(), "releaseTenthsMs": data::RELEASE_TENTHS_MS.to_vec(),
        "masterStereoMs": data::MASTER_STEREO_MS.to_vec(), "masterLcrMs": data::MASTER_LCR_MS.to_vec(),
        "masterGrainMs": data::MASTER_GRAIN_MS.to_vec(), "masterModStereoMs": data::MASTER_MOD_STEREO_MS.to_vec(), "masterModMonoMs": data::MASTER_MOD_MONO_MS.to_vec()
    })
}
fn peaking(frequency: u8, q: u8, gain: i8) -> Result<BiquadCoefficients, &'static str> {
    let c = data::equalizer()
        .peaking(frequency, q, gain)
        .ok_or("EQ coefficient preparation failed")?;
    // Interpreted Q formats, not an established FXD03 multiplier model.
    let scale = |word: u32, shift: u32| f64::from(word as i32) / f64::from(1u32 << (31 - shift));
    Ok(BiquadCoefficients {
        numerator: [scale(c[0], c[5]), scale(c[1], c[6]), scale(c[2], c[7])],
        feedback: [scale(c[3], 1), scale(c[4], 0)],
    })
}
fn shelf(frequency: u8, gain: i8, high: bool) -> Result<BiquadCoefficients, &'static str> {
    let eq = data::equalizer();
    let c = if high {
        eq.high_shelf(frequency, gain)
    } else {
        eq.low_shelf(frequency, gain)
    }
    .ok_or("Shelf coefficient preparation failed")?;
    let denominator = if high { 1_048_576.0 } else { 4_194_304.0 };
    Ok(BiquadCoefficients {
        numerator: [
            f64::from(c[0] as i32) / denominator,
            f64::from(c[1] as i32) / denominator,
            0.0,
        ],
        feedback: [f64::from(c[2] as i32) / 4_194_304.0, 0.0],
    })
}
pub fn compile(
    program: EffectAudioProgram,
    tempo: u16,
) -> Result<EffectAudioSettings, &'static str> {
    let def = definition(program.kind, program.master).ok_or("Invalid effect kind")?;
    if program.kind == 0 {
        return Ok(EffectAudioSettings {
            program,
            compiled_tempo: tempo.max(1),
            ..Default::default()
        });
    }
    for (value, property) in program
        .parameters
        .iter()
        .zip(def.properties)
        .take(usize::from(def.count))
    {
        let decoded = i16::from(*value) - i16::from(property.zero);
        if !(property.minimum..=property.maximum).contains(&decoded) {
            return Err("Effect property outside original range");
        }
    }
    let p = program.parameters;
    let mut result = EffectAudioSettings {
        program,
        compiled_tempo: tempo.max(1),
        ..Default::default()
    };
    result.lfo_tables = Some(&data::LFO);
    if program.kind == 0 {
        return Ok(result);
    }
    let mix = EffectMix::compile(
        EffectKind::new(program.kind).unwrap(),
        p[0],
        MixContext {
            byte1: p[1],
            byte5: p[5],
            byte6: p[6],
        },
    )
    .ok_or("Invalid original Dry/Wet")?;
    result.dry = mix.dry as f32 / 8_388_607.0;
    result.wet = mix.wet as f32 / 8_388_607.0;
    let property = |name: &str| {
        def.properties
            .iter()
            .take(usize::from(def.count))
            .position(|p| p.name == name)
    };
    let value = |name: &str, fallback: u8| property(name).map_or(fallback, |i| p[i]);
    let frequency = value("LFO Freq", 32);
    let divisions = [
        32.0,
        16.0,
        8.0,
        4.0,
        3.0,
        2.0,
        1.5,
        4.0 / 3.0,
        1.0,
        0.75,
        2.0 / 3.0,
        0.5,
        1.0 / 3.0,
        0.25,
        1.0 / 6.0,
        0.125,
        0.0625,
    ];
    let lfo_index = property("LFO Freq");
    let sync_index = lfo_index
        .and_then(|i| i.checked_sub(1))
        .filter(|&i| def.properties[i].name == "TempoSync");
    result.lfo = EffectLfo {
        hz: data::LFO_FREQUENCY[usize::from(frequency)] as f32 * (1000.0 / 4_294_967_296.0),
        beats: divisions[usize::from(value("Sync Note", 8)).min(16)],
        sync: sync_index.is_some_and(|i| p[i] != 0),
        waveform: value("LFO Wave", 3),
        shape: (f32::from(value("LFO Shape", 64)) - 64.0) / 63.0,
        phase: f32::from(value("InitPhase", 0)) / 36.0,
        spread: (f32::from(value("LFOSpread", 64)) - 64.0) / 36.0,
        key_sync: value("Key Sync", 0) != 0,
    };
    if program.kind == 20 {
        result.lfo.hz = data::LFO_FREQUENCY[usize::from(p[2])] as f32 * (1000.0 / 4_294_967_296.0);
    }
    if program.kind == 21 {
        result.lfo.hz = 0.1 + 3.0 * f32::from(p[2]) / 127.0;
    }
    if program.kind == 29 {
        result.lfo.hz = if p[5] == 0 { 0.7 } else { 6.5 };
    }
    if program.kind == 19 {
        result.lfo.hz = data::LFO_FREQUENCY[usize::from(p[14])] as f32 * (1000.0 / 4_294_967_296.0);
    }
    if program.kind == 6 {
        result.eq_count = if program.master { 4 } else { 2 };
        for band in 0..usize::from(result.eq_count) {
            let b = 4 + 3 * band;
            let gain = (i16::from(p[b + 2]) - 64) as i8;
            result.equalizers[band] = if band == 0 && p[2] != 0 {
                shelf(p[b], gain, false)?
            } else if band == usize::from(result.eq_count) - 1 && p[3] != 0 {
                shelf(p[b], gain, true)?
            } else {
                peaking(p[b], p[b + 1], gain)?
            };
        }
    }
    if program.kind == 7 {
        result.eq_count = 4;
        for band in 0..4 {
            let b = 2 + 3 * band;
            result.equalizers[band] = peaking(p[b], p[b + 1], (i16::from(p[b + 2]) - 64) as i8)?;
        }
    }
    let clock = DelayClock {
        tempo: tempo.max(1),
        status: 0,
    };
    let state = DelayTimeState {
        capacity: (2 * DELAY_FRAMES - 4) as u32,
        ..Default::default()
    };
    let mut time = data::delay_time();
    let times = match program.kind {
        13 => {
            result.delay_sync = p[1] != 0;
            time.lcr(&p, state, clock)
        }
        14 => {
            result.delay_sync = p[2] != 0;
            time.stereo(&p, state, clock)
        }
        15 | 16 => {
            result.delay_sync = p[1] != 0;
            time.auto_pan(&p, state, clock, program.kind == 16)
        }
        17..=19 => {
            result.delay_sync = p[1] != 0;
            time.stereo_milliseconds = if program.kind == 18 {
                data::MOD_STEREO_MS
            } else {
                data::MOD_MONO_MS
            };
            let mut mapped = [0; 20];
            mapped[2..8].copy_from_slice(&p[1..7]);
            time.stereo(&mapped, state, clock)
        }
        _ => None,
    };
    if (13..=19).contains(&program.kind) {
        let times = times.ok_or("Original delay-time preparation failed")?;
        result.delay = times
            .frames
            .map(|v| (v as f32).clamp(1.0, (DELAY_FRAMES - 2) as f32));
    }
    if program.kind == 26 {
        let ratio = if p[3] != 0 {
            time.sync_ratio[usize::from(p[4])]
        } else {
            time.free_ratio[usize::from(p[4])]
        };
        result.delay_sync = p[3] != 0;
        result.delay[0] = if p[3] != 0 {
            48_000.0 * 600.0 / f32::from(tempo.max(1))
                * f32::from(ratio)
                * f32::from(time.notes[usize::from(p[6])])
                / 192_000.0
        } else {
            f32::from(ratio) * f32::from(time.lcr_milliseconds[usize::from(p[5])]) * 0.048
        };
    }
    if program.kind == 20 {
        result.delay[0] = f32::from(data::CHORUS_TENTHS_MS[usize::from(p[4])]) * 4.8;
        result.delay[1] = f32::from(data::CHORUS_TENTHS_MS[usize::from(p[5])]) * 4.8;
    }
    if program.kind == 27 {
        result.delay_sync = p[1] != 0;
        result.delay[0] = if p[1] != 0 {
            48_000.0 * 600.0 / f32::from(tempo.max(1))
                * f32::from(time.sync_ratio[usize::from(p[2])])
                * f32::from(time.notes[usize::from(p[4])])
                / 192_000.0
        } else {
            48.0 * (1.0 + f32::from(p[3]) * 4.0) * f32::from(time.free_ratio[usize::from(p[2])])
                / 1000.0
        };
    }
    if program.kind == 10 {
        result.decimator_bits = 4 + p[4];
        result.decimator_rate =
            data::DECIMATOR_RATE[usize::from(p[3])] as f32 / 4_194_304.0 * 48_000.0;
    }
    if (1..=3).contains(&program.kind) {
        let kind = program.kind;
        let threshold = if kind == 2 {
            data::LIMITER_THRESHOLD[usize::from(p[3])] as f32 / 8_388_607.0
        } else if kind == 3 {
            data::GATE_THRESHOLD[usize::from(p[2])] as f32 / 8_388_607.0
        } else {
            0.0
        };
        let ratio = if kind == 2 {
            data::LIMITER_INVERSE_RATIO[usize::from(p[2])] as f32 / 8_388_607.0
        } else {
            0.25
        };
        let gain = if kind == 1 {
            1.0
        } else {
            data::DYNAMICS_GAIN[usize::from(p[5] - 23)] as f32 / 524_288.0
        };
        let sensitivity = if kind == 1 {
            data::COMPRESSOR_SENSITIVITY[usize::from(p[2] - 1)] as f32 / 8_388_607.0
        } else {
            0.0
        };
        let attack = data::DYNAMICS_ATTACK[usize::from(p[if kind == 2 { 4 } else { 3 }])] as f32
            / 16_777_216.0;
        let release = if kind == 3 {
            data::DYNAMICS_RELEASE[usize::from(p[4])] as f32 / 16_777_216.0
        } else {
            1.0 - (-1.0f32 / (0.12 * 48_000.0)).exp()
        };
        result.dynamics = [threshold, ratio, gain, sensitivity, attack, release];
    }
    if program.kind == 11 {
        let small = if program.master { p[1] >= 4 } else { p[1] == 2 };
        let index = if small {
            data::REVERB_SMALL_INDEX[usize::from(p[2])]
        } else {
            data::REVERB_LARGE_INDEX[usize::from(p[2])]
        };
        result.reverb_seconds = f32::from(index + 1) * 0.1;
    }
    if program.kind == 12 {
        let size = u32::from(data::ER_SIZE[usize::from(p[2])]);
        let pre = u32::from(data::ER_PRE_DELAY[usize::from(p[3])]);
        result.early_taps =
            data::ER_TAPS.map(|tap| ((u32::from(tap) * size / 1000 + pre) * 48).max(1) as f32);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radias_synth_domain::{
        Sample,
        effect_audio::{DELAY_WORDS, EffectAudioContext, EffectAudioProcessor},
        pan::StereoFrame,
    };
    #[test]
    fn generated_catalog_and_tables_equal_the_pinned_system() {
        let root = crate::reference_root();
        let system = std::fs::read(root.join("firmware/RADIAS_SYS_0200.bin")).unwrap();
        let library = crate::effects::EffectLibrary::from_system(&system).unwrap();
        for master in [false, true] {
            for kind in 0..31 {
                let bank = if master {
                    radias_synth_domain::effect_control::EffectBank::Master
                } else {
                    radias_synth_domain::effect_control::EffectBank::Insert
                };
                let d = definition(kind, master).unwrap();
                let k = EffectKind::new(kind).unwrap();
                assert_eq!(d.name, library.name(bank, k).unwrap());
                assert_eq!(d.count, library.parameter_count(bank, k).unwrap());
                compile(default_program(kind, master).unwrap(), 1200).unwrap();
            }
        }
        let expected = library.equalizer_tables().unwrap();
        let actual = data::equalizer();
        assert_eq!(expected.frequency, actual.frequency);
        assert_eq!(
            library.tremolo_ring_mod_tables().unwrap().fixed_frequency,
            data::RING_FREQUENCY
        );
        assert_eq!(expected.pole, actual.pole);
        assert_eq!(expected.q, actual.q);
        assert_eq!(expected.gain, actual.gain);
        assert_eq!(expected.curve_a, actual.curve_a);
        assert_eq!(expected.curve_b, actual.curve_b);
        let expected = library.delay_time_tables().unwrap();
        let actual = data::delay_time();
        assert_eq!(expected.free_ratio, actual.free_ratio);
        assert_eq!(expected.sync_ratio, actual.sync_ratio);
        assert_eq!(expected.notes, actual.notes);
        assert_eq!(expected.lcr_milliseconds, actual.lcr_milliseconds);
        assert_eq!(expected.stereo_milliseconds, actual.stereo_milliseconds);
        for (offset, table) in [
            (0x9501c, data::ATTACK_TENTHS_MS),
            (0x9511c, data::RELEASE_TENTHS_MS),
            (0xac46c, data::MASTER_STEREO_MS),
            (0xac56c, data::MASTER_LCR_MS),
            (0xac66c, data::MASTER_GRAIN_MS),
            (0xac96c, data::MASTER_MOD_STEREO_MS),
            (0xaca6c, data::MASTER_MOD_MONO_MS),
        ] {
            let original: [u16; 128] = core::array::from_fn(|i| {
                u16::from_be_bytes(
                    system[offset + 2 * i..offset + 2 * i + 2]
                        .try_into()
                        .unwrap(),
                )
            });
            assert_eq!(table, original);
        }
        assert!(!std::hint::black_box(ORIGINAL_AUDIO_PARITY_QUALIFIED));
    }
    #[test]
    fn disabled_and_zero_wet_are_exact_bypasses() {
        let mut memory = vec![0.0; DELAY_WORDS];
        let context = EffectAudioContext::default();
        for kind in 0..31 {
            let mut p = default_program(kind, false).unwrap();
            p.enabled = false;
            let mut processor = EffectAudioProcessor::new(compile(p, 1200).unwrap());
            for input in [i32::MIN, -123456789, 17, 123456789, i32::MAX] {
                let sample = StereoFrame {
                    left: Sample(input),
                    right: Sample(!input),
                };
                assert_eq!(
                    processor.process(sample, &mut memory, &context).unwrap(),
                    sample
                );
            }
            p.enabled = true;
            p.parameters[0] = 0;
            let mut processor = EffectAudioProcessor::new(compile(p, 1200).unwrap());
            let sample = StereoFrame {
                left: Sample(123456789),
                right: Sample(-7654321),
            };
            assert_eq!(
                processor.process(sample, &mut memory, &context).unwrap(),
                sample,
                "kind {kind}"
            );
        }
    }
    #[test]
    fn stereo_delay_echo_uses_the_original_prepared_sample_address() {
        let mut p = default_program(14, false).unwrap();
        p.parameters[0] = 100;
        p.parameters[2] = 0;
        p.parameters[4] = 10;
        p.parameters[5] = 20;
        p.parameters[8] = 0;
        let settings = compile(p, 1200).unwrap();
        let expected = settings.delay.map(|n| n as usize);
        assert_ne!(expected[0], expected[1]);
        let mut processor = EffectAudioProcessor::new(settings);
        let mut memory = vec![0.0; DELAY_WORDS];
        let context = EffectAudioContext::default();
        let mut first = [None; 2];
        for frame in 0..=expected[0].max(expected[1]) {
            let input = if frame == 0 {
                StereoFrame {
                    left: Sample(0x20000000),
                    right: Sample(0x20000000),
                }
            } else {
                Default::default()
            };
            let out = processor.process(input, &mut memory, &context).unwrap();
            for (channel, value) in [out.left.0, out.right.0].into_iter().enumerate() {
                if value != 0 && first[channel].is_none() {
                    first[channel] = Some(frame);
                }
            }
        }
        assert_eq!(first, [Some(expected[0]), Some(expected[1])]);
    }
    #[test]
    fn tremolo_has_the_expected_independent_sine_amplitude() {
        let p = default_program(24, false).unwrap();
        let mut settings = compile(p, 1200).unwrap();
        settings.lfo.hz = 4.0;
        settings.lfo.spread = 0.25;
        settings.lfo.shape = 0.0;
        settings.lfo.waveform = 3;
        // Independently check the sine equation. The production table-backed
        // LFO uses the separately qualified, quantized original waveform.
        settings.lfo_tables = None;
        let mut processor = EffectAudioProcessor::new(settings);
        let mut memory = vec![0.0; DELAY_WORDS];
        let context = EffectAudioContext::default();
        let mut worst = 0.0f64;
        for frame in 0..12000 {
            let x = StereoFrame {
                left: Sample(0x20000000),
                right: Sample(0x20000000),
            };
            let y = processor.process(x, &mut memory, &context).unwrap();
            for (c, v) in [y.left.0, y.right.0].into_iter().enumerate() {
                let phase = 4.0 * frame as f64 / 48000.0 + c as f64 * 0.25;
                let expected = 0.25 * (0.5 - 0.5 * (std::f64::consts::TAU * phase).sin());
                worst = worst.max((v as f64 / 2147483648.0 - expected).abs());
            }
        }
        assert!(worst < 0.00005, "sine amplitude error {worst}");
    }
    #[test]
    fn all_bypass_native_rack_preserves_the_original_dry_projection() {
        let mut actual = crate::standalone::StandaloneSynth::new();
        let mut dry = crate::standalone::StandaloneSynth::new();
        let programs = core::array::from_fn(|i| default_program(0, i == 8).unwrap());
        actual.engine.apply(crate::synthesizer::Command::Effects(
            prepare_rack(programs, 1200).unwrap(),
        ));
        for synth in [&mut actual, &mut dry] {
            for timbre in 0..4 {
                synth.control(timbre, 71, 1);
                synth.engine.apply(crate::synthesizer::Command::Note(
                    timbre,
                    48 + 3 * timbre,
                    96,
                ));
            }
        }
        for _ in 0..4096 {
            assert_eq!(actual.engine.sample(), dry.engine.sample());
        }
    }
}
