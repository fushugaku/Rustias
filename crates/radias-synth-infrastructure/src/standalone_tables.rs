//! Analytic, firmware-free data ports for every native synthesis controller.
use crate::prepared::ControlMap;
use radias_synth_application::{amplifier::ControllerTables, modulation::VoiceModulationTables};
use radias_synth_domain::{
    amplifier_control::AmplifierTables,
    bandlimit::BandwidthTable,
    controller_comb::CombControlTables,
    controller_filter::ControllerFilterTables,
    controller_filter2::Filter2ControlTables,
    controller_mixer::MixerScales,
    controller_secondary::FineTuneTable,
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
    filter_control::FilterMixTable,
    lfo::LfoTables,
    lfo_tempo::LfoTempoTables,
    modulation::ModulationTables,
    note_pitch::NotePitchTables,
    pitch::PitchTable,
    portamento::{PortamentoCurves, PortamentoRates},
    voice_group::VoiceGroupTables,
};
pub const RATE: f64 = 48_000.0;
pub const NORMALIZATION: i32 = 0x4000_0000;
pub fn seconds(value: u8) -> f64 {
    0.003 * 2000.0f64.powf(value as f64 / 127.0)
}
fn curve(kind: usize, x: f64) -> f64 {
    match kind {
        0 => x.powi(3),
        1 => x * x,
        2 => x * x * (3.0 - 2.0 * x),
        3 | 5 => x,
        4 => x.sqrt(),
        6 => 1.0 - (1.0 - x).powi(3),
        7 => x.powf(1.5),
        _ => x.powf(0.25 + (kind as f64 - 8.0) / 4.0),
    }
}
pub fn controllers() -> ControllerTables {
    ControllerTables {
        curves: EnvelopeCurves {
            values: core::array::from_fn(|c| {
                core::array::from_fn(|i| (curve(c, i as f64 / 256.0) * 65536.0) as u16)
            }),
        },
        timing: EnvelopeTimingTables {
            increments: core::array::from_fn(|_| {
                core::array::from_fn(|i| {
                    (0x00ff_ffff as f64 / (2000.0 * seconds(i as u8))).round() as u32
                })
            }),
            scale: core::array::from_fn(|i| {
                (256.0 * 2.0f64.powf((i as f64 - 64.0) / 32.0)).round() as u16
            }),
            key_tracking: core::array::from_fn(|i| ((i as i32 - 64) * 256) as i16),
        },
        amplifier: AmplifierTables {
            velocity: core::array::from_fn(|i| {
                (((i as f64 / 127.0) - 1.0) * 32768.0).round() as i16
            }),
            midi_volume: core::array::from_fn(|i| {
                ((i as f64 / 127.0).powi(2) * 8192.0).round() as u16
            }),
            program_volume: core::array::from_fn(|i| {
                if i == 0 {
                    32768
                } else {
                    (32768.0 / ((i + 1) as f64).sqrt()) as u16
                }
            }),
            key_tracking: core::array::from_fn(|i| ((i as i32 - 64) * 256) as i16),
        },
    }
}
pub fn pitch() -> PitchTable {
    PitchTable {
        notes: core::array::from_fn(|i| {
            (440.0 * 2.0f64.powf((i as f64 - 69.0) / 12.0) * 4294967296.0 / RATE).round() as u32
        }),
        fractions: core::array::from_fn(|i| {
            ((2.0f64.powf(i as f64 / (128.0 * 12.0)) - 1.0) * 32768.0).round() as i16
        }),
    }
}
pub fn bandwidth() -> BandwidthTable {
    BandwidthTable { gains: [0; 129] }
}
pub fn fine() -> FineTuneTable {
    FineTuneTable {
        values: core::array::from_fn(|i| ((i as i32 - 64) * 4) as i16),
    }
}
pub fn note_pitch() -> NotePitchTables {
    let ratios: [[f64; 12]; 2] = [
        [
            1.0,
            16.0 / 15.0,
            9.0 / 8.0,
            6.0 / 5.0,
            5.0 / 4.0,
            4.0 / 3.0,
            45.0 / 32.0,
            3.0 / 2.0,
            8.0 / 5.0,
            5.0 / 3.0,
            9.0 / 5.0,
            15.0 / 8.0,
        ],
        [
            1.0,
            25.0 / 24.0,
            9.0 / 8.0,
            6.0 / 5.0,
            5.0 / 4.0,
            4.0 / 3.0,
            25.0 / 18.0,
            3.0 / 2.0,
            8.0 / 5.0,
            5.0 / 3.0,
            9.0 / 5.0,
            15.0 / 8.0,
        ],
    ];
    let cents = core::array::from_fn(|b| {
        core::array::from_fn(|i| match b {
            0 | 1 => (1200.0 * ratios[b][i].log2() - i as f64 * 100.0).round() as i8,
            2 => [0, 14, 4, 18, 8, -2, 12, 2, 16, 6, 20, 10][i],
            3 => [0, -24, -7, 10, -14, 3, -21, -3, -28, -10, 7, -17][i],
            4 => {
                if i % 2 == 0 {
                    0
                } else {
                    50
                }
            }
            _ => (i as i8 - 5) * 2,
        })
    });
    NotePitchTables {
        fine_tune: core::array::from_fn(|i| (i as i32 - 64) * 1024),
        cents,
        scaled_root: [[0; 12]; 2],
        scaled_note: core::array::from_fn(|b| {
            core::array::from_fn(|i| (cents[b][i] as f64 * 1.28) as i8)
        }),
        vibrato: fine(),
    }
}
pub fn lfo() -> LfoTables {
    LfoTables {
        warp: core::array::from_fn(|i| {
            (8192.0 / (64 - i).max(1) as f64).min(65535.0).round() as u16
        }),
        sine: core::array::from_fn(|i| {
            (((i as f64 - 1.0) / 512.0 * std::f64::consts::FRAC_PI_2).sin() * 32767.0).round()
                as i16
        }),
        frequency: core::array::from_fn(|i| {
            (0.02 * 1500.0f64.powf(i as f64 / 127.0) * 4294967296.0 / 500.0).round() as u32
        }),
        initial_phase: core::array::from_fn(|i| (i * 2048) as u16),
        frequency_scale: core::array::from_fn(|i| {
            (2048.0 * 2.0f64.powf((i as f64 - 64.0) / 16.0)).round() as u32
        }),
    }
}
pub fn modulation() -> VoiceModulationTables {
    VoiceModulationTables {
        lfo: lfo(),
        pitch: pitch(),
        bandwidth: bandwidth(),
        matrix: ModulationTables {
            pitch_depth: core::array::from_fn(|i| ((i as i32 - 64) * 96) as i16),
            lfo_rate_depth: core::array::from_fn(|i| ((i as i32 - 64) * 256) as i16),
            key_linear_depth: core::array::from_fn(|i| ((i as i32 - 64) * 256) as i16),
            key_cutoff_depth: core::array::from_fn(|i| ((i as i32 - 64) * 400) as i16),
            key_lfo_rate_depth: core::array::from_fn(|i| ((i as i32 - 64) * 256) as i16),
        },
    }
}
pub fn tempo() -> LfoTempoTables {
    const BEATS: [f64; 17] = [
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
    LfoTempoTables {
        clock_steps: core::array::from_fn(|i| (BEATS[i.min(16)] * 48.0).round().max(2.0) as u16),
        increments: core::array::from_fn(|i| {
            (4294967296.0 * 2.0 / (500.0 * BEATS[i.min(16)])) as u32
        }),
        tempo_coefficients: core::array::from_fn(|i| {
            (4294967296.0 * 4096.0 / (600.0 * 500.0 * 7158.0 * BEATS[i])) as u32
        }),
        minimum_increment: 1,
        maximum_increment: 0x7fff_ffff,
    }
}
fn frequency(code: f64) -> u32 {
    let hz = (30.0 * 200.0f64.powf(code / 127.0)).clamp(5.0, 10000.0);
    ((std::f64::consts::PI * hz / RATE).sin() * 2147483647.0) as u32
}
pub fn filter() -> ControllerFilterTables {
    ControllerFilterTables {
        frequency: core::array::from_fn(|i| frequency(i as f64 - 36.0)),
        key_depth: core::array::from_fn(|i| ((i as i32 - 64) * 64) as i16),
    }
}
pub fn filter2() -> Filter2ControlTables {
    Filter2ControlTables {
        resonance: core::array::from_fn(|i| (i as f64 / 127.0 * 0.95 * 2147483647.0) as i32),
        input_gain: [24576; 128],
        linked_serial_input_gain: [24576; 128],
    }
}
pub fn control_map() -> ControlMap {
    ControlMap {
        frequencies: core::array::from_fn(|i| frequency(i as f64) as i32),
        resonances: filter2().resonance,
        input_gains: [24576; 128],
        normalization: NORMALIZATION,
    }
}
pub fn mix() -> FilterMixTable {
    // Continuous 24dB LP -> 12dB LP -> BP -> HP -> dry morph.
    let points = [
        [32767, 0, 0, 0, 0],
        [0, 0, 0, 32767, 0],
        [0, 0, 0, 0, 32767],
        [0, 32767, 0, 0, 0],
        [0, 0, 32767, 0, 0],
    ];
    FilterMixTable {
        weights: core::array::from_fn(|r| {
            core::array::from_fn(|i| {
                let x = i as f64 / 32.0;
                let a = (x.floor() as usize).min(3);
                let f = x - a as f64;
                ((1.0 - f) * points[a][r] as f64 + f * points[a + 1][r] as f64).round() as i16
            })
        }),
    }
}
pub fn comb() -> CombControlTables {
    CombControlTables {
        delays: core::array::from_fn(|i| {
            ((RATE / (30.0 * 200.0f64.powf(i as f64 / 127.0))).clamp(2.0, 4000.0) * 65536.0) as u32
        }),
        feedback: core::array::from_fn(|i| (i as f64 / 127.0 * 0.98 * 2147483647.0) as u32),
    }
}
pub fn portamento() -> radias_synth_application::portamento::PortamentoTables {
    radias_synth_application::portamento::PortamentoTables {
        rates: PortamentoRates {
            values: core::array::from_fn(|i| {
                if i == 0 {
                    0
                } else {
                    (0x00ff_ffff as f64 / (2000.0 * seconds(i as u8))).round() as u32
                }
            }),
        },
        curves: PortamentoCurves {
            values: core::array::from_fn(|c| {
                core::array::from_fn(|i| (curve(c, i as f64 / 256.0) * 65536.0) as u16)
            }),
        },
    }
}
pub fn groups() -> VoiceGroupTables {
    VoiceGroupTables {
        detune: core::array::from_fn(|bank| {
            core::array::from_fn(|i| {
                if bank == 0 {
                    0
                } else {
                    ((i as f64 / bank as f64 - 0.5) * 512.0) as i16
                }
            })
        }),
        pan: core::array::from_fn(|bank| {
            core::array::from_fn(|i| {
                if bank == 0 {
                    0
                } else {
                    ((i as f64 / bank as f64 - 0.5) * 240.0) as i16
                }
            })
        }),
    }
}
pub fn mixer() -> MixerScales {
    MixerScales {
        primary: [32767; 64],
        secondary: [[32767; 4]; 2],
    }
}
pub fn noise() -> radias_synth_application::noise::NoiseTables {
    radias_synth_application::noise::NoiseTables {
        pitch: pitch(),
        noise: radias_synth_domain::noise_control::NoisePitchTable {
            curve_scales: core::array::from_fn(|i| {
                (16384.0 * 2.0f64.powf((60.0 - i as f64) / 24.0)).clamp(256.0, 32767.0) as i16
            }),
        },
        counters: radias_synth_domain::controller_noise::FormantCounterSeeds {
            values: core::array::from_fn(|i| ((i as u32 * 2053 + 97) & 32767) as i16),
        },
    }
}

/// Q15 reciprocal edge slopes for the native sub-oscillator kernels.
/// The table index is the high byte of a half-cycle phase increment. The
/// kernel scales its slope by 256, so an edge spanning i * 2^24 uses 2^14/i.
/// This analytic data port contains no firmware words.
pub fn shapers() -> radias_synth_domain::waveshaper::ShaperTables {
    radias_synth_domain::waveshaper::ShaperTables {
        sub_edges: core::array::from_fn(|i| if i == 0 { 32767 } else { (16384 / i) as i16 }),
    }
}
