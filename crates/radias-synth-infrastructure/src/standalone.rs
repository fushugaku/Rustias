//! Firmware-free profile for the shared native generator.
//! Tuning and envelope tables are generated mathematically, not read from ROM.
use crate::{
    prepared::PreparedVoice,
    synthesizer::{Command, Synthesizer},
};
use radias_synth_application::{
    amplifier::{AmplifierProgram, ControllerTables},
    mixer::MixerProgram,
};
use radias_synth_domain::{
    Phase,
    amplifier_control::AmplifierTables,
    bandlimit::BandwidthTable,
    control_slew::SlewWeights,
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
    filter::FilterCoefficients,
    mixer::OscillatorMix,
    oscillator::Oscillator,
    pan::VoiceBus,
    pitch::{PhaseIncrement, PitchTable},
    primary_oscillator::PrimaryParameters,
    voice::{Voice, VoiceParameters},
    waveform::{ShapeParameters, Transfer, WaveformTable},
    waveshaper::ShaperTables,
};

pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy)]
pub struct Settings {
    pub waveform: u8,
    pub cutoff: u8,
    pub resonance: u8,
    pub filter_type: u8,
    pub adsr: [u8; 4],
    pub level: u8,
    pub pan: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            waveform: 0,
            cutoff: 100,
            resonance: 20,
            filter_type: 0,
            adsr: [12, 48, 100, 48],
            level: 100,
            pan: 64,
        }
    }
}

/// IDs are part of the small browser adapter ABI: wave, cutoff, resonance,
/// attack, decay, sustain, release, level, pan, filter type.
pub struct StandaloneSynth {
    pub engine: Synthesizer,
    pub settings: [Settings; 4],
}

pub fn envelope_seconds(value: u8) -> f64 {
    0.003 * 2000.0f64.powf(value as f64 / 127.0)
}

fn controller_tables() -> ControllerTables {
    ControllerTables {
        curves: EnvelopeCurves {
            values: core::array::from_fn(|_| core::array::from_fn(|i| (i * 256) as u16)),
        },
        timing: EnvelopeTimingTables {
            increments: core::array::from_fn(|_| {
                core::array::from_fn(|i| {
                    (0x00ff_ffff as f64 / (2000.0 * envelope_seconds(i as u8))).round() as u32
                })
            }),
            scale: [256; 128],
            key_tracking: [0; 128],
        },
        amplifier: AmplifierTables {
            velocity: core::array::from_fn(|i| {
                (((i as f64 / 127.0) - 1.0) * 32768.0).round() as i16
            }),
            midi_volume: core::array::from_fn(|i| {
                ((i as f64 / 127.0).powi(2) * 8192.0).round() as u16
            }),
            program_volume: [32768; 128],
            key_tracking: [0; 128],
        },
    }
}

pub fn filter_coefficients(settings: Settings) -> FilterCoefficients {
    let hz = 30.0 * 200.0f64.powf(settings.cutoff as f64 / 127.0);
    let gain = (std::f64::consts::PI * hz / SAMPLE_RATE as f64).sin();
    let damping = 0.75 - 0.69 * settings.resonance as f64 / 127.0;
    let mut mix = [0; 5];
    mix[match settings.filter_type {
        1 => 1,
        2 => 4,
        _ => 3,
    }] = 32767;
    FilterCoefficients {
        input_gain: 24576,
        feedback: (damping * 2147483647.0) as i32,
        integrator_gain: (gain * 2147483647.0) as i32,
        post_gain: 32767,
        post_feedback: 0,
        mix,
    }
}

fn plan(waveform: u8) -> PreparedVoice {
    let increment = PhaseIncrement((261.625565 * 4294967296.0 / SAMPLE_RATE as f64) as u32);
    let primary = PrimaryParameters::waveform(waveform, increment, 0).unwrap();
    let filter = filter_coefficients(Settings::default());
    PreparedVoice {
        initial: Voice {
            primary: Default::default(),
            secondary: Oscillator::new(
                Phase(0),
                increment,
                0,
                Transfer::ParabolicSine,
                ShapeParameters {
                    subtract_edge: false,
                    edge_coefficient: 0,
                    waveform_control: 0,
                    gain: 32767,
                },
            ),
            filter: Default::default(),
            second_filter: Default::default(),
            waveshaper: Default::default(),
            envelope: Default::default(),
            previous_secondary: Default::default(),
            previous_primary: Phase(0),
            mixer_noise: Default::default(),
        },
        parameters: VoiceParameters {
            primary,
            primary_pitch_code: 60 * 256,
            mix: OscillatorMix {
                primary_gain: 32767,
                secondary_gain: 0,
                noise_gain: 0,
            },
            filter,
            routing: None,
            shaper: None,
            envelope_target: 0,
            envelope_rate: 0x1d4,
            pan_position: 0x4000_0000,
            secondary_modulation: Default::default(),
        },
        events: Vec::new(),
        control_slew: SlewWeights {
            target: 0x1d4,
            memory: 0x7e2d,
        },
        reference_start_frame: 0,
        reference_voice_frames: 0,
        bus: VoiceBus::new(0).unwrap(),
    }
}

impl Default for StandaloneSynth {
    fn default() -> Self {
        Self::new()
    }
}
impl StandaloneSynth {
    pub fn new() -> Self {
        let tuning = PitchTable {
            notes: core::array::from_fn(|i| {
                (440.0 * 2.0f64.powf((i as f64 - 69.0) / 12.0) * 4294967296.0 / SAMPLE_RATE as f64)
                    .round() as u32
            }),
            fractions: core::array::from_fn(|i| {
                ((2.0f64.powf(i as f64 / (128.0 * 12.0)) - 1.0) * 32768.0).round() as i16
            }),
        };
        let mut engine = Synthesizer::new(
            (0..4).map(plan).collect(),
            WaveformTable {
                correction: [0; 129],
                shapers: ShaperTables {
                    sub_edges: [0; 129],
                },
            },
            Some((tuning, BandwidthTable { gains: [0; 129] })),
            Some(controller_tables()),
            None,
            None,
            None,
        )
        .unwrap();
        engine.apply(Command::PanTables(
            Box::new(radias_synth_domain::controller_pan::PanTables {
                targets: core::array::from_fn(|i| (i as u32 * 32697 / 127) as u16),
            }),
            SlewWeights {
                target: 0x1d4,
                memory: 0x7e2d,
            },
        ));
        let settings = [Settings::default(); 4];
        for (i, setting) in settings.iter().enumerate() {
            let timbre = i as u8;
            engine.apply(Command::Timbre(timbre, true, timbre));
            engine.apply(Command::Filter(timbre, filter_coefficients(*setting)));
            engine.apply(Command::Mixer(
                timbre,
                MixerProgram {
                    levels: [127, 0, 0],
                    ..Default::default()
                },
            ));
            let mut amplifier = AmplifierProgram::default();
            amplifier.level = setting.level;
            amplifier.envelope.adsr = setting.adsr;
            amplifier.envelope.curve = 3;
            engine.apply(Command::AmplifierProgram(timbre, amplifier));
        }
        Self { engine, settings }
    }
    pub fn control(&mut self, timbre: u8, parameter: u32, value: u8) -> bool {
        let Some(settings) = self.settings.get_mut(timbre as usize) else {
            return false;
        };
        if value > 127 {
            return false;
        }
        match parameter {
            0 if value < 4 => {
                settings.waveform = value;
                self.engine.apply(Command::Waveform(timbre, value as usize));
            }
            1 | 2 | 9 => {
                match parameter {
                    1 => settings.cutoff = value,
                    2 => settings.resonance = value,
                    9 if value < 3 => settings.filter_type = value,
                    _ => return false,
                }
                self.engine
                    .apply(Command::Filter(timbre, filter_coefficients(*settings)));
            }
            3..=6 => {
                settings.adsr[(parameter - 3) as usize] = value;
                self.engine.apply(Command::Envelope(timbre, settings.adsr));
            }
            7 => {
                settings.level = value;
                self.engine.apply(Command::AmplifierLevel(timbre, value));
            }
            8 => {
                settings.pan = value;
                self.engine.apply(Command::Pan(
                    timbre,
                    radias_synth_domain::controller_pan::PanControl {
                        position: value,
                        ..Default::default()
                    },
                ));
            }
            _ => return false,
        }
        true
    }
    pub fn note(&mut self, timbre: u8, note: u8, velocity: u8) -> bool {
        if timbre >= 4 || note > 127 || velocity > 127 {
            return false;
        }
        self.engine.apply(Command::Note(timbre, note, velocity));
        true
    }
    pub fn stop(&mut self) {
        self.engine.apply(Command::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(check: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(check)
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn notes_render_without_rom_and_release_to_silence() {
        run(|| {
            let mut synth = StandaloneSynth::new();
            synth.note(0, 69, 100);
            let peak = (0..12_000)
                .map(|_| synth.engine.sample().left.0.abs())
                .max()
                .unwrap();
            assert!(peak > 100_000, "Native voice is silent");
            synth.note(0, 69, 0);
            for _ in 0..48_000 {
                synth.engine.sample();
            }
            assert_eq!(synth.engine.active_count(), 0);
            assert_eq!(synth.engine.sample().left.0, 0);
        });
    }
    #[test]
    fn each_waveform_and_timbre_uses_the_native_voice_pool() {
        run(|| {
            for waveform in 0..4 {
                let mut synth = StandaloneSynth::new();
                assert!(synth.control(3, 0, waveform));
                synth.note(3, 60, 127);
                assert_eq!(synth.engine.held_count(), 1);
                assert!((0..6000).any(|_| synth.engine.sample().right.0 != 0));
                synth.stop();
                assert_eq!(synth.engine.active_count(), 0);
            }
        });
    }
    #[test]
    fn allocation_is_bounded_and_invalid_controls_leave_settings_unchanged() {
        run(|| {
            let mut synth = StandaloneSynth::new();
            for note in 40..80 {
                synth.note(0, note, 100);
            }
            assert!(synth.engine.active_count() <= 24);
            assert!(!synth.control(4, 1, 64));
            assert!(!synth.control(0, 0, 4));
            assert!(!synth.control(0, 9, 3));
            assert_eq!(synth.settings[0].filter_type, 0);
        });
    }
    #[test]
    fn callback_boundaries_do_not_change_samples() {
        run(|| {
            let mut one = StandaloneSynth::new();
            let mut split = StandaloneSynth::new();
            one.note(0, 64, 100);
            split.note(0, 64, 100);
            let expected: Vec<_> = (0..1024).map(|_| one.engine.sample()).collect();
            let mut actual = Vec::new();
            for count in [17, 111, 256, 640] {
                actual.extend((0..count).map(|_| split.engine.sample()));
            }
            assert_eq!(expected, actual);
        });
    }
}
