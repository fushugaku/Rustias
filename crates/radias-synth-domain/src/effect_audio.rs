//! Audible native reconstructions of the RADIAS effect families.
//!
//! Stored fields, mix words and selected control tables come from SYS 2.00.
//! The FXD03 sample ISA is not decoded: these sample equations, delay topology,
//! interpolation and floating-point arithmetic are NOT qualified original DSP
//! behavior. Do not use this backend as an original-firmware audio oracle.
use crate::{Sample, pan::StereoFrame};

pub const ORIGINAL_AUDIO_PARITY_QUALIFIED: bool = false;
pub const DELAY_FRAMES: usize = 131_072;
pub const DELAY_WORDS: usize = DELAY_FRAMES * 2;
const MASK: usize = DELAY_FRAMES - 1;
const TAU: f32 = core::f32::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectAudioProgram {
    pub kind: u8,
    pub enabled: bool,
    pub master: bool,
    /// Original 20 stored property bytes; signed values retain their bias.
    pub parameters: [u8; 20],
}
pub fn stored_effects(program: &crate::program::Program) -> [EffectAudioProgram; 9] {
    core::array::from_fn(|slot| {
        let master = slot == 8;
        let offset = if master {
            1038
        } else {
            168 + 228 * (slot / 2) + 24 * (slot % 2)
        };
        let b = program.bytes();
        EffectAudioProgram {
            kind: b[offset] & 127,
            enabled: b[offset] & 128 != 0,
            master,
            parameters: b
                [offset + if master { 2 } else { 4 }..offset + if master { 22 } else { 24 }]
                .try_into()
                .unwrap(),
        }
    })
}
#[derive(Clone, Copy, Debug)]
pub struct BiquadCoefficients {
    pub numerator: [f64; 3],
    /// Feedback is added, matching the stored controller sign convention.
    pub feedback: [f64; 2],
}
impl Default for BiquadCoefficients {
    fn default() -> Self {
        Self {
            numerator: [1.0, 0.0, 0.0],
            feedback: [0.0; 2],
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectLfo {
    pub hz: f32,
    pub beats: f32,
    pub sync: bool,
    pub waveform: u8,
    pub shape: f32,
    pub phase: f32,
    pub spread: f32,
    pub key_sync: bool,
}
#[derive(Clone, Copy)]
pub struct EffectAudioSettings {
    pub program: EffectAudioProgram,
    pub sample_rate: f32,
    pub dry: f32,
    pub wet: f32,
    pub lfo: EffectLfo,
    pub lfo_tables: Option<&'static crate::lfo::LfoTables>,
    pub equalizers: [BiquadCoefficients; 4],
    pub eq_count: u8,
    /// Decoded original duration tables for delay algorithms, in samples.
    pub delay: [f32; 3],
    pub delay_sync: bool,
    pub compiled_tempo: u16,
    pub decimator_rate: f32,
    pub decimator_bits: u8,
    pub early_taps: [f32; 16],
    pub reverb_seconds: f32,
    /// Interpreted dynamics control payloads: threshold, inverse ratio, gain,
    /// sensitivity, attack, release. FXD03 arithmetic remains unqualified.
    pub dynamics: [f32; 6],
}
impl Default for EffectAudioSettings {
    fn default() -> Self {
        Self {
            program: Default::default(),
            sample_rate: 48_000.0,
            dry: 1.0,
            wet: 0.0,
            lfo: EffectLfo {
                hz: 1.0,
                ..Default::default()
            },
            lfo_tables: None,
            equalizers: [Default::default(); 4],
            eq_count: 0,
            delay: [1.0; 3],
            delay_sync: false,
            compiled_tempo: 1200,
            decimator_rate: 48_000.0,
            decimator_bits: 24,
            early_taps: [1.0; 16],
            reverb_seconds: 1.0,
            dynamics: [0.1, 0.25, 1.0, 0.5, 0.01, 0.001],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct EffectAudioContext {
    pub tempo_tenths: u16,
    pub note: u8,
    /// Typed controller input port, normalized to -1..1; index 0 means None.
    pub controllers: [f32; 13],
}
impl Default for EffectAudioContext {
    fn default() -> Self {
        Self {
            tempo_tenths: 1200,
            note: 60,
            controllers: [0.0; 13],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BiquadHistory {
    x: [f64; 2],
    y: [f64; 2],
}
impl BiquadHistory {
    fn sample(&mut self, x: f32, c: BiquadCoefficients) -> f32 {
        let x = f64::from(x);
        let y = c.numerator[0] * x
            + c.numerator[1] * self.x[0]
            + c.numerator[2] * self.x[1]
            + c.feedback[0] * self.y[0]
            + c.feedback[1] * self.y[1];
        self.x = [x, self.x[0]];
        // Wide internal headroom, with a finite bound for runaway feedback.
        let y = if y.is_finite() {
            y.clamp(-64.0, 64.0)
        } else {
            0.0
        };
        self.y = [y, self.y[0]];
        y as f32
    }
}

/// Small persistent sample state. Delay storage belongs to the application.
pub struct EffectAudioProcessor {
    settings: EffectAudioSettings,
    cursor: usize,
    phase: f32,
    rotor_phase: f32,
    rotor_hz: f32,
    horn_phase: f32,
    horn_hz: f32,
    rotary_toggles: [bool; 2],
    rotary_previous: [bool; 2],
    oscillator_phase: f32,
    pitch_phase: [f32; 2],
    random: u32,
    held_random: f32,
    envelope: [f32; 2],
    gain: [f32; 2],
    low: [[f32; 8]; 2],
    band: [f32; 2],
    mod_value: [f32; 2],
    allpass: [[f32; 12]; 2],
    previous: [f32; 2],
    held: [f32; 2],
    decimation_phase: f32,
    eq: [[BiquadHistory; 4]; 2],
    reverb_low: [f32; 8],
    current_mix: [f32; 2],
}
impl EffectAudioProcessor {
    pub fn new(settings: EffectAudioSettings) -> Self {
        Self {
            settings,
            cursor: 0,
            phase: settings.lfo.phase,
            rotor_phase: 0.0,
            rotor_hz: 0.0,
            horn_phase: 0.0,
            horn_hz: 0.0,
            rotary_toggles: [false; 2],
            rotary_previous: [false; 2],
            oscillator_phase: 0.0,
            pitch_phase: [0.0, 0.5],
            random: 0x5241_4449,
            held_random: 0.0,
            envelope: [0.0; 2],
            gain: [1.0; 2],
            low: [[0.0; 8]; 2],
            band: [0.0; 2],
            mod_value: [0.0; 2],
            allpass: [[0.0; 12]; 2],
            previous: [0.0; 2],
            held: [0.0; 2],
            decimation_phase: 1.0,
            eq: [[Default::default(); 4]; 2],
            reverb_low: [0.0; 8],
            current_mix: [settings.dry, settings.wet],
        }
    }
    pub fn settings(&self) -> &EffectAudioSettings {
        &self.settings
    }
    /// Returns whether the application must clear the old type's delay memory.
    /// Parameter edits retain oscillator phase, filter history and delay tails.
    pub fn configure(&mut self, settings: EffectAudioSettings) -> bool {
        if settings.program.kind != self.settings.program.kind
            || settings.program.master != self.settings.program.master
        {
            *self = Self::new(settings);
            true
        } else {
            self.settings = settings;
            false
        }
    }
    pub fn note_on(&mut self) {
        if self.settings.lfo.key_sync {
            self.phase = self.settings.lfo.phase;
        }
    }
    fn p(&self, i: usize) -> f32 {
        f32::from(self.settings.program.parameters[i])
    }
    fn n(&self, i: usize) -> f32 {
        self.p(i) / 127.0
    }
    fn signed(&self, i: usize) -> f32 {
        (self.p(i) - 64.0) / 63.0
    }
    fn noise(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        (self.random as i32 as f32) / 2_147_483_648.0
    }
    fn lfo_value(&self, channel: usize) -> f32 {
        let p = wrap(self.phase + channel as f32 * self.settings.lfo.spread);
        if let Some(tables) = self.settings.lfo_tables {
            use crate::lfo::{LfoState, LfoWave};
            let wave = match self.settings.lfo.waveform {
                0 => LfoWave::Saw,
                1 => LfoWave::BipolarPulse,
                2 => LfoWave::Triangle,
                4 => LfoWave::SampleHold,
                _ => LfoWave::Sine,
            };
            let phase = (p * 65536.0) as u16;
            let shape = (self.settings.lfo.shape * 63.0) as i8;
            let state = LfoState {
                random: (self.held_random * 32767.0) as i16,
                ..Default::default()
            };
            return tables.value_raw(wave, phase, shape, state) as f32 / 32768.0;
        }
        let duty = (0.5 + self.settings.lfo.shape * 0.45).clamp(0.05, 0.95);
        let skewed = if p < duty {
            0.5 * p / duty
        } else {
            0.5 + 0.5 * (p - duty) / (1.0 - duty)
        };
        match self.settings.lfo.waveform {
            0 => 2.0 * p - 1.0,
            1 => {
                if p < duty {
                    1.0
                } else {
                    -1.0
                }
            }
            2 => 1.0 - 4.0 * (skewed - 0.5).abs(),
            4 => self.held_random,
            _ => libm::sinf(TAU * skewed),
        }
    }
    fn advance_lfo(&mut self, context: &EffectAudioContext) {
        let lfo = self.settings.lfo;
        let hz = if lfo.sync {
            f32::from(context.tempo_tenths.max(1)) / (600.0 * lfo.beats.max(1.0 / 64.0))
        } else {
            lfo.hz
        };
        let next = self.phase + hz / self.settings.sample_rate;
        if next >= 1.0 {
            self.held_random = self.noise();
        }
        self.phase = wrap(next);
    }
    fn delay_frames(&self, i: usize, context: &EffectAudioContext) -> f32 {
        let ratio = if self.settings.delay_sync {
            f32::from(self.settings.compiled_tempo) / f32::from(context.tempo_tenths.max(1))
        } else {
            1.0
        };
        (self.settings.delay[i] * ratio).clamp(1.0, (DELAY_FRAMES - 2) as f32)
    }
    fn read(&self, memory: &[f32], channel: usize, distance: f32) -> f32 {
        let distance = distance.clamp(1.0, (DELAY_FRAMES - 2) as f32);
        let integer = distance as usize;
        let f = distance - integer as f32;
        let i = self.cursor.wrapping_sub(integer) & MASK;
        let j = i.wrapping_sub(1) & MASK;
        let a = memory[2 * i + channel];
        a + (memory[2 * j + channel] - a) * f
    }
    fn write(&self, memory: &mut [f32], value: [f32; 2]) {
        for c in 0..2 {
            memory[2 * self.cursor + c] = finite(value[c]).clamp(-8.0, 8.0);
        }
    }
    fn eq_sample(&mut self, channel: usize, mut x: f32, first: usize, end: usize) -> f32 {
        for i in first..end {
            x = self.eq[channel][i].sample(x, self.settings.equalizers[i]);
        }
        x
    }
    fn detect(&mut self, c: usize, x: f32, attack: f32, release: f32) -> f32 {
        let x = x.abs();
        let a = if x > self.envelope[c] {
            attack
        } else {
            release
        };
        self.envelope[c] += a * (x - self.envelope[c]);
        self.envelope[c]
    }
    fn lowpass(&mut self, c: usize, index: usize, x: f32, hz: f32) -> f32 {
        let a = 1.0
            - libm::expf(
                -TAU * hz.clamp(5.0, self.settings.sample_rate * 0.45) / self.settings.sample_rate,
            );
        self.low[c][index] += a * (x - self.low[c][index]);
        self.low[c][index]
    }
    fn damp(&mut self, c: usize, x: f32, high: f32, low: f32) -> f32 {
        let x = self.lowpass(c, 6, x, 18_000.0 * libm::powf(0.018, high.clamp(0.0, 1.0)));
        let bass = self.lowpass(c, 7, x, 20.0 + 780.0 * low);
        x - bass * low
    }
    fn state_filter(&mut self, c: usize, x: f32, hz: f32, resonance: f32, mode: u8) -> f32 {
        // Topology-preserving SVF; equations are reconstructed, not FXD opcodes.
        let g = libm::tanf(
            core::f32::consts::PI * hz.clamp(20.0, 17_000.0) / self.settings.sample_rate,
        );
        let k = 2.0 - 1.92 * resonance.clamp(0.0, 1.0);
        let a = 1.0 / (1.0 + g * (g + k));
        let v1 = a * (self.band[c] + g * (x - self.low[c][0]));
        let v2 = self.low[c][0] + g * v1;
        self.band[c] = finite(2.0 * v1 - self.band[c]).clamp(-16.0, 16.0);
        self.low[c][0] = finite(2.0 * v2 - self.low[c][0]).clamp(-16.0, 16.0);
        let high = x - k * v1 - v2;
        match mode {
            0 => v2,
            1 => high,
            2 => v1,
            3 => high + v2,
            _ => high + v2 - k * v1,
        }
    }
    fn dynamics(&mut self, input: [f32; 2], kind: u8) -> [f32; 2] {
        let controls = self.settings.dynamics;
        let attack = controls[4];
        let release = controls[5];
        let linked = !self.settings.program.master || self.p(1) == 0.0;
        let linked_signal = (input[0] + input[1]) * 0.5;
        core::array::from_fn(|c| {
            let env = self.detect(
                c,
                if linked { linked_signal } else { input[c] },
                attack,
                release,
            );
            let target = match kind {
                1 => (4.0 * controls[3] / libm::sqrtf(0.01 + env)).min(16.0),
                2 => {
                    let threshold = controls[0];
                    if env <= threshold {
                        1.0
                    } else {
                        libm::powf(threshold / env, 1.0 - controls[1])
                    }
                }
                _ => {
                    if env >= controls[0] {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
            let speed = if target < self.gain[c] {
                attack
            } else {
                release
            };
            self.gain[c] += speed * (target - self.gain[c]);
            let output = if kind == 1 {
                self.n(4) * 2.0
            } else {
                controls[2]
            };
            input[c] * self.gain[c] * output
        })
    }
    fn reverb(&mut self, input: [f32; 2], memory: &mut [f32]) -> [f32; 2] {
        // Eight-line orthogonal feedback delay network with input diffusion.
        // This topology is unqualified; original Rev Time and damping controls
        // are retained at the public boundary.
        let kind = self.p(1) as usize;
        let size = [1.0, 0.72, 0.48, 1.35, 0.22, 0.11][kind.min(5)];
        let lengths = [
            1493.0, 1601.0, 1747.0, 1867.0, 1999.0, 2131.0, 2269.0, 2381.0,
        ];
        let rt60 = self.settings.reverb_seconds;
        let predelay = self.p(4) / 127.0 * 0.2 * self.settings.sample_rate;
        let at = self.cursor & 8191;
        let mono = (input[0] + input[1]) * 0.5 * self.n(6);
        let pre_cursor = self.cursor & 65_535;
        let pre_at = 65_536 + pre_cursor;
        let delayed =
            memory[65_536 + (pre_cursor.wrapping_sub(predelay.min(65_534.0) as usize) & 65_535)];
        memory[pre_at] = mono;
        let excitation = if predelay < 1.0 { mono } else { delayed };
        for (i, length) in lengths.iter().enumerate() {
            let delay =
                (length * size * self.settings.sample_rate / 48_000.0).clamp(1.0, 8190.0) as usize;
            let index = i * 8192 + (at.wrapping_sub(delay) & 8191);
            let v = memory[index];
            self.reverb_low[i] +=
                (0.05 + 0.9 * (1.0 - self.p(3) / 100.0)) * (v - self.reverb_low[i]);
        }
        let y = self.reverb_low;
        let mean = y.iter().sum::<f32>() * 0.25;
        for i in 0..8 {
            let seconds = lengths[i] * size / 48_000.0;
            let feedback = libm::powf(0.001, seconds / rt60.max(0.1)).min(0.998);
            let sign = if i & 1 == 0 { 1.0 } else { -1.0 };
            memory[i * 8192 + at] =
                finite(excitation * 0.15 * sign + (mean - y[i]) * feedback).clamp(-8.0, 8.0);
        }
        let mut out = [0.0; 2];
        for (c, output) in out.iter_mut().enumerate() {
            let early = memory[65_536 + (pre_cursor.wrapping_sub(829 + 127 * c) & 65_535)];
            let late = (y[c] + y[c + 2] - y[c + 4] + y[c + 6]) * 0.45;
            let v = early * self.n(9) + late * self.n(10) + excitation * self.n(5);
            *output = self.tone(c, v, (self.p(7) - 64.0) * 0.5, (self.p(8) - 64.0) * 0.5);
        }
        out
    }
    fn tone(&mut self, c: usize, x: f32, low_db: f32, high_db: f32) -> f32 {
        let low = self.lowpass(c, 4, x, 300.0);
        let high = x - self.lowpass(c, 5, x, 3500.0);
        x + low * (libm::powf(10.0, low_db / 20.0) - 1.0)
            + high * (libm::powf(10.0, high_db / 20.0) - 1.0)
    }
    fn delay(
        &mut self,
        input: [f32; 2],
        memory: &mut [f32],
        context: &EffectAudioContext,
        kind: u8,
    ) -> [f32; 2] {
        let mono = 0.5 * (input[0] + input[1]);
        let mut y = [0.0; 2];
        if kind == 13 {
            let taps = core::array::from_fn::<_, 3, _>(|i| {
                self.read(memory, 0, self.delay_frames(i, context))
            });
            let feedback = self.n(12).min(0.995);
            let tail = self.damp(0, taps[1], self.p(13) / 100.0, self.p(14) / 100.0);
            self.write(memory, [mono * self.n(15) + tail * feedback; 2]);
            y = [
                taps[0] * self.n(9) + taps[1] * self.n(10) * 0.707,
                taps[2] * self.n(11) + taps[1] * self.n(10) * 0.707,
            ];
            return width(y, self.n(16));
        }
        let stereo = matches!(kind, 14 | 16 | 18);
        let feedback = if kind == 19 {
            self.n(9)
        } else if kind == 14 {
            self.n(8)
        } else {
            self.n(7)
        }
        .min(0.995);
        let mut write = [0.0; 2];
        for (c, out) in y.iter_mut().enumerate() {
            let lfo = self.lfo_value(c);
            let mut delay = self.delay_frames(c, context);
            if matches!(kind, 17 | 18) {
                delay += lfo * self.n(8) * 0.012 * self.settings.sample_rate;
            }
            if kind == 19 {
                delay += libm::sinf(TAU * self.phase * 0.13) * self.n(15) * 4.0;
            }
            *out = self.read(memory, c, delay);
        }
        if kind == 19 {
            y[0] *= self.n(7);
            y[1] *= self.n(8);
        }
        for c in 0..2 {
            let (hi, lo, trim) = match kind {
                14 => (self.p(9) / 100.0, self.p(10) / 100.0, self.n(11)),
                15 => (self.p(16) / 100.0, self.p(17) / 100.0, self.n(18)),
                16 => (self.p(17) / 100.0, self.p(18) / 100.0, self.n(19)),
                19 => (self.p(10) / 100.0, self.p(11) / 100.0, self.n(12)),
                _ => (0.0, 0.0, 1.0),
            };
            let tail = if kind == 14 && self.p(1) != 0.0 {
                y[1 - c]
            } else {
                y[c]
            };
            let tail = self.damp(c, tail, hi, lo);
            let x = if stereo { input[c] } else { mono };
            write[c] = x * trim + tail * feedback;
            if kind == 19 {
                let pre = self.lowpass(c, 1, write[c], 300.0 + 17_700.0 * self.n(16));
                write[c] = soft(pre * (1.0 + 8.0 * self.n(13))) / (1.0 + 2.0 * self.n(13));
            }
            if matches!(kind, 15 | 16) {
                let sign = if c == 0 { 1.0 } else { -1.0 };
                y[c] *= 1.0 - self.n(8) * 0.5 + sign * self.n(8) * 0.5 * self.lfo_value(c);
            }
        }
        self.write(memory, write);
        if kind == 14 {
            width(y, self.n(12))
        } else if kind == 19 {
            width(y, self.n(17))
        } else {
            y
        }
    }
    fn chorus(&mut self, input: [f32; 2], memory: &mut [f32], kind: u8) -> [f32; 2] {
        let mut out = [0.0; 2];
        for (c, out) in out.iter_mut().enumerate() {
            if kind == 21 {
                let mut value = 0.0;
                for tap in 0..3 {
                    let phase = self.phase + tap as f32 / 3.0 + c as f32 * 0.19;
                    let mod1 = libm::sinf(TAU * phase);
                    let mod2 = libm::sinf(TAU * (self.phase * 5.1 + tap as f32 * 0.21));
                    let delay = self.settings.sample_rate
                        * (0.018 + self.n(1) * (0.005 * mod1 + 0.002 * mod2));
                    value += self.read(memory, c, delay) / 3.0;
                }
                *out = value;
            } else {
                let delay = self.settings.delay[c]
                    + self.lfo_value(c) * self.n(1) * 0.010 * self.settings.sample_rate;
                let v = self.read(memory, c, delay);
                *out = self.tone(c, v, (self.p(7) - 64.0) * 0.5, (self.p(8) - 64.0) * 0.5);
            }
        }
        self.write(
            memory,
            input.map(|v| v * if kind == 20 { self.n(6) } else { 1.0 }),
        );
        out
    }
    fn phaser(&mut self, input: [f32; 2]) -> [f32; 2] {
        core::array::from_fn(|c| {
            let hz = 80.0
                * libm::powf(
                    140.0,
                    (self.n(2) + self.lfo_value(c) * self.n(3) * 0.5).clamp(0.0, 1.0),
                );
            let t = libm::tanf(core::f32::consts::PI * hz / self.settings.sample_rate);
            let a = (1.0 - t) / (1.0 + t);
            let mut x = input[c] + self.previous[c] * self.n(4) * 0.92;
            let stages = if self.p(1) == 0.0 { 4 } else { 8 };
            for stage in 0..stages {
                let y = -a * x + self.allpass[c][stage];
                self.allpass[c][stage] = x + a * y;
                x = y;
            }
            self.previous[c] = self.damp(c, x, self.p(14) / 100.0, 0.0);
            x
        })
    }
    fn pitch_shift(
        &mut self,
        input: [f32; 2],
        memory: &mut [f32],
        context: &EffectAudioContext,
    ) -> [f32; 2] {
        let semitone = self.p(1) - 64.0 + (self.p(2) - 64.0) / 100.0;
        let ratio = libm::powf(2.0, semitone / 12.0);
        let window = [0.060, 0.030, 0.120][(self.p(9) as usize).min(2)] * self.settings.sample_rate;
        let step = (1.0 - ratio) / window;
        self.pitch_phase[0] = wrap(self.pitch_phase[0] + step);
        self.pitch_phase[1] = wrap(self.pitch_phase[0] + 0.5);
        let offset = self.delay_frames(0, context);
        let mut y = [0.0; 2];
        for (c, out) in y.iter_mut().enumerate() {
            let channel = if self.settings.program.master { c } else { 0 };
            for phase in self.pitch_phase {
                let weight = 0.5 - 0.5 * libm::cosf(TAU * phase);
                *out += self.read(memory, channel, offset + phase * window) * weight;
            }
        }
        let mut writes = [0.0; 2];
        for c in 0..2 {
            let x = if self.settings.program.master {
                input[c]
            } else {
                0.5 * (input[0] + input[1])
            };
            let tail = self.damp(
                c,
                if self.p(7) == 0.0 {
                    y[c]
                } else {
                    self.read(memory, c, offset)
                },
                self.p(10) / 100.0,
                0.0,
            );
            writes[c] = x * self.n(11) + tail * self.n(8) * 0.98;
        }
        self.write(memory, writes);
        y
    }
    fn rotary(
        &mut self,
        input: [f32; 2],
        memory: &mut [f32],
        context: &EffectAudioContext,
    ) -> [f32; 2] {
        let mut switches = [self.p(1) != 0.0, self.p(5) != 0.0];
        for (switch, value) in switches.iter_mut().enumerate() {
            let source = self.p(if switch == 0 { 2 } else { 6 }) as usize;
            if source == 0 {
                continue;
            }
            let high = context.controllers[source.min(12)] >= 64.0 / 127.0;
            if self.p(if switch == 0 { 3 } else { 7 }) == 0.0 {
                if high && !self.rotary_previous[switch] {
                    self.rotary_toggles[switch] = !self.rotary_toggles[switch];
                }
                *value ^= self.rotary_toggles[switch];
            } else {
                *value = high;
            }
            self.rotary_previous[switch] = high;
        }
        let speed = if switches[0] {
            0.0
        } else if self.p(4) == 0.0 {
            if switches[1] { 6.5 } else { 0.65 }
        } else {
            (self.n(8) + context.controllers[(self.p(9) as usize).min(12)] * self.signed(10))
                .clamp(0.0, 1.0)
                * 8.0
        };
        let ratio = |value: f32| {
            if value == 0.0 {
                0.0
            } else {
                0.5 + (value - 1.0) * 0.02
            }
        };
        let horn_target = speed * ratio(self.p(13));
        let rotor_target = speed * ratio(self.p(15)) * 0.78;
        self.horn_hz += (0.00001 + 0.0004 * self.n(12) * self.n(12)) * (horn_target - self.horn_hz);
        self.rotor_hz +=
            (0.00001 + 0.0004 * self.n(14) * self.n(14)) * (rotor_target - self.rotor_hz);
        self.horn_phase = wrap(self.horn_phase + self.horn_hz / self.settings.sample_rate);
        self.rotor_phase = wrap(self.rotor_phase + self.rotor_hz / self.settings.sample_rate);
        let horn = libm::sinf(TAU * self.horn_phase);
        let rotor = libm::sinf(TAU * self.rotor_phase);
        let mono = (input[0] + input[1]) * 0.5 * self.n(18);
        self.write(memory, [mono; 2]);
        let out = core::array::from_fn(|c| {
            let sign = if c == 0 { 1.0 } else { -1.0 };
            let proximity = 1.0 - 0.8 * self.n(16);
            let delayed = self.read(memory, c, 16.0 + 12.0 * (1.0 + sign * horn) * proximity);
            let bass = self.lowpass(c, 1, mono, 800.0);
            let high = delayed - self.lowpass(c, 2, delayed, 800.0);
            let balance = self.p(11) / 100.0;
            (bass * (1.0 - balance) * (1.0 + sign * rotor * 0.4 * proximity)
                + high * balance * (1.0 + sign * horn * 0.7 * proximity))
                * 2.0
        });
        width(out, self.n(17) * 2.0)
    }
    /// Process one stereo sample without allocation or firmware execution.
    /// Memory must be DELAY_WORDS; errors leave the sample state untouched.
    pub fn process(
        &mut self,
        input: StereoFrame,
        memory: &mut [f32],
        context: &EffectAudioContext,
    ) -> Result<StereoFrame, &'static str> {
        if memory.len() != DELAY_WORDS {
            return Err("Effect delay storage length");
        }
        let kind = self.settings.program.kind;
        if kind > 30 {
            return Err("Invalid effect type");
        }
        if !self.settings.program.enabled || kind == 0 {
            return Ok(input);
        }
        let x = [
            input.left.0 as f32 / 2_147_483_648.0,
            input.right.0 as f32 / 2_147_483_648.0,
        ];
        let y = match kind {
            1..=3 => self.dynamics(x, kind),
            4 | 5 => core::array::from_fn(|c| {
                let source = if kind == 4 { 5 } else { 4 };
                let intensity = if kind == 4 { 6 } else { 5 };
                let modulation = match (kind, self.p(source) as u8) {
                    (4, 0) | (5, 1) => self.lfo_value(c),
                    (5, 0) => {
                        let env = self.detect(c, x[c], 0.01, 0.0001) * (1.0 + 15.0 * self.n(7));
                        let env = env.clamp(0.0, 1.0);
                        libm::powf(env, libm::powf(4.0, -self.signed(8))) * 2.0 - 1.0
                    }
                    _ => {
                        context.controllers
                            [(self.p(if kind == 4 { 15 } else { 16 }) as usize).min(12)]
                    }
                };
                let response = self.n(if kind == 4 { 7 } else { 6 });
                self.mod_value[c] +=
                    (0.00005 + response * response * 0.05) * (modulation - self.mod_value[c]);
                let modulation = self.mod_value[c];
                let control = if kind == 4 {
                    self.n(2)
                } else {
                    0.5 + self.signed(2) * 0.35
                };
                let wah = if kind == 5 { self.p(1) as usize } else { 0 };
                let base = if kind == 5 {
                    [180.0, 220.0, 270.0, 170.0, 240.0, 300.0][wah.min(5)]
                } else {
                    30.0
                };
                let span = if kind == 5 {
                    [15.0, 12.0, 10.0, 18.0, 14.0, 11.0][wah.min(5)]
                } else {
                    600.0
                };
                let hz = base
                    * libm::powf(
                        span,
                        (control + modulation * self.signed(intensity) * 0.5).clamp(0.0, 1.0),
                    );
                let resonance = if kind == 4 {
                    self.n(3)
                } else {
                    (0.5 + self.signed(3) * 0.5).clamp(0.0, 1.0)
                };
                let filter = if kind == 4 { self.p(1) as u8 } else { 4 };
                let v = self.state_filter(
                    c,
                    x[c] * if kind == 4 { self.n(4) } else { 1.0 },
                    hz,
                    resonance,
                    if kind == 5 {
                        2
                    } else {
                        match filter {
                            3 => 1,
                            4 => 2,
                            _ => 0,
                        }
                    },
                );
                if kind == 4 && filter <= 1 {
                    let v = self.lowpass(c, 2, v, hz);
                    if filter == 0 {
                        self.lowpass(c, 3, v, hz)
                    } else {
                        v
                    }
                } else {
                    v
                }
            }),
            6 => core::array::from_fn(|c| {
                self.eq_sample(c, x[c] * self.n(1), 0, usize::from(self.settings.eq_count))
            }),
            7 => core::array::from_fn(|c| {
                let v = self.eq_sample(c, x[c], 0, 1);
                let v = soft(v * (1.0 + 100.0 * self.n(1) * self.n(1)));
                self.eq_sample(c, v, 1, 4) * self.n(14)
            }),
            8 => core::array::from_fn(|c| {
                let cabinet = self.p(1);
                let v = x[c] - self.lowpass(c, 0, x[c], 45.0 + cabinet * 7.0);
                let v = self.lowpass(c, 1, v, 3500.0 + cabinet * 240.0);
                let v = self.lowpass(c, 2, v, 4800.0 - cabinet * 150.0);
                let air = x[c] - self.lowpass(c, 3, x[c], 9000.0);
                (v + air * self.n(2) * 0.15) * self.n(3)
            }),
            9 => core::array::from_fn(|c| {
                let mut v = x[c];
                for stage in 0..2 {
                    let b = if stage == 0 { 1 } else { 7 };
                    let lo = self.lowpass(c, stage * 2, v, 20.0 + self.n(b) * 1000.0);
                    v -= lo * self.n(b);
                    v = self.lowpass(
                        c,
                        stage * 2 + 1,
                        v,
                        20_000.0 * libm::powf(0.025, self.n(b + 1)),
                    );
                    let drive = libm::powf(10.0, (self.p(b + 2) - 64.0) / 20.0);
                    let bias = (self.p(b + 3) / 100.0 - 0.5) * self.p(b + 4) / 100.0;
                    let saturation = 1.0 + 8.0 * self.p(b + 4) / 100.0;
                    v = (soft(v * drive * saturation + bias) - soft(bias))
                        / libm::sqrtf(saturation);
                }
                let dc = self.lowpass(c, 5, v, 15.0);
                (v - dc) * self.n(12) * if self.p(6) != 0.0 { -1.0 } else { 1.0 }
            }),
            10 => {
                let modulation = libm::powf(2.0, self.lfo_value(0) * self.signed(6) * 4.0);
                self.decimation_phase +=
                    self.settings.decimator_rate * modulation / self.settings.sample_rate;
                let mut filtered = x;
                for c in 0..2 {
                    if self.p(1) != 0.0 {
                        filtered[c] = self.lowpass(c, 0, x[c], self.settings.decimator_rate * 0.45);
                    }
                }
                if self.decimation_phase >= 1.0 {
                    self.decimation_phase = wrap(self.decimation_phase);
                    let scale = (1u32 << self.settings.decimator_bits.clamp(1, 24)) as f32;
                    self.held = filtered.map(|v| libm::roundf(v * scale) / scale);
                }
                core::array::from_fn(|c| {
                    self.damp(c, self.held[c], self.p(2) / 100.0, 0.0) * self.n(5)
                })
            }
            11 => self.reverb(x, memory),
            12 => {
                self.write(memory, [(x[0] + x[1]) * 0.5 * self.n(4); 2]);
                core::array::from_fn(|c| {
                    let mut value = 0.0;
                    for i in 0..16 {
                        let gain = match self.p(1) as u8 {
                            0 => libm::powf(0.75, i as f32),
                            3 => libm::powf(0.8, (15 - i) as f32),
                            _ => libm::powf(0.9, i as f32),
                        };
                        let sign = if (i + c) % 3 == 0 { -1.0 } else { 1.0 };
                        let delay = self.settings.early_taps[i]
                            + if self.p(1) == 2.0 {
                                self.lfo_value(c) * 24.0
                            } else {
                                0.0
                            };
                        value += self.read(memory, 0, delay) * gain * sign * 0.18;
                    }
                    let value = self.damp(c, value, self.p(7) / 100.0, self.p(8) / 100.0);
                    self.tone(c, value, (self.p(5) - 64.0) * 0.5, (self.p(6) - 64.0) * 0.5)
                })
            }
            13..=19 => self.delay(x, memory, context, kind),
            20 | 21 => self.chorus(x, memory, kind),
            22 => {
                let mut y = [0.0; 2];
                let mut write = [0.0; 2];
                for c in 0..2 {
                    let delay =
                        (0.0001 + self.p(2) * 0.00015 + self.lfo_value(c) * self.n(4) * 0.004)
                            * self.settings.sample_rate;
                    y[c] = self.read(memory, c, delay);
                    let feedback = self.damp(c, y[c], self.p(15) / 100.0, 0.0) * self.n(5) * 0.98;
                    write[c] = self.lowpass(c, 1, x[c], 100.0 + 17_900.0 * self.n(3)) + feedback;
                    if self.p(1) != 0.0 {
                        y[c] = -y[c];
                    }
                }
                self.write(memory, write);
                y
            }
            23 => self.phaser(x),
            24 => {
                core::array::from_fn(|c| x[c] * (1.0 - self.n(1) * (0.5 + 0.5 * self.lfo_value(c))))
            }
            25 => {
                let freq = if self.p(1) == 0.0 {
                    10.0 * libm::powf(2000.0, self.n(2))
                } else {
                    440.0
                        * libm::powf(
                            2.0,
                            (f32::from(context.note) - 69.0 + self.p(3) - 64.0
                                + (self.p(4) - 64.0) / 100.0)
                                / 12.0,
                        )
                };
                self.oscillator_phase = wrap(
                    self.oscillator_phase
                        + freq * libm::powf(2.0, self.lfo_value(0) * self.signed(6) * 2.0)
                            / self.settings.sample_rate,
                );
                let carrier = match self.p(5) as u8 {
                    0 => 2.0 * self.oscillator_phase - 1.0,
                    1 => {
                        if self.oscillator_phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    _ => libm::sinf(TAU * self.oscillator_phase),
                };
                core::array::from_fn(|c| {
                    self.lowpass(c, 1, x[c], 20_000.0 * libm::powf(0.01, self.n(14))) * carrier
                })
            }
            26 => self.pitch_shift(x, memory, context),
            27 => {
                let duration = self.delay_frames(0, context).max(48.0);
                self.write(memory, x);
                core::array::from_fn(|c| {
                    let phase = wrap(self.phase + c as f32 * self.settings.lfo.spread);
                    let head = phase * duration;
                    self.read(
                        memory,
                        if self.settings.program.master { c } else { 0 },
                        1.0 + head,
                    ) * libm::sinf(core::f32::consts::PI * phase)
                })
            }
            28 => {
                let y = core::array::from_fn(|c| {
                    self.read(
                        memory,
                        c,
                        self.settings.sample_rate * (0.006 + 0.005 * self.n(1) * self.lfo_value(c)),
                    )
                });
                self.write(memory, x);
                y
            }
            29 => self.rotary(x, memory, context),
            30 => {
                let source = match self.p(7) as u8 {
                    0 => {
                        let envelope = self.detect(0, (x[0] + x[1]) * 0.5, 0.01, 0.0001)
                            * (1.0 + 15.0 * self.n(10));
                        libm::powf(envelope.clamp(0.0, 1.0), libm::powf(4.0, -self.signed(11)))
                            * 2.0
                            - 1.0
                    }
                    1 => self.lfo_value(0),
                    _ => context.controllers[(self.p(19) as usize).min(12)],
                };
                self.mod_value[0] +=
                    (0.00005 + self.n(9) * self.n(9) * 0.05) * (source - self.mod_value[0]);
                let control =
                    (self.signed(1) + self.mod_value[0] * self.signed(8)).clamp(-1.0, 1.0);
                let vowels = [
                    [800.0, 1150.0, 2900.0],
                    [350.0, 2000.0, 2800.0],
                    [325.0, 700.0, 2530.0],
                    [400.0, 1700.0, 2600.0],
                    [450.0, 800.0, 2830.0],
                ];
                let center = vowels[(self.p(3) as usize).min(4)];
                let edge = vowels[(self.p(if control > 0.0 { 2 } else { 4 }) as usize).min(4)];
                core::array::from_fn(|c| {
                    let input = soft((x[0] + x[1]) * 0.5 * (1.0 + 12.0 * self.n(6)));
                    let mut out = 0.0;
                    for band in 0..3 {
                        let hz = center[band] + control.abs() * (edge[band] - center[band]);
                        let coeff = bandpass(hz, 2.0 + 12.0 * self.n(5), self.settings.sample_rate);
                        out += self.eq[c][band].sample(input, coeff) * [1.0, 0.65, 0.35][band];
                    }
                    out * 2.0
                })
            }
            _ => unreachable!(),
        };
        self.advance_lfo(context);
        self.cursor = (self.cursor + 1) & MASK;
        for (current, target) in self
            .current_mix
            .iter_mut()
            .zip([self.settings.dry, self.settings.wet])
        {
            *current += 0.015625 * (target - *current);
        }
        if self.current_mix == [1.0, 0.0] {
            return Ok(input);
        }
        Ok(StereoFrame {
            left: to_sample(x[0] * self.current_mix[0] + finite(y[0]) * self.current_mix[1]),
            right: to_sample(x[1] * self.current_mix[0] + finite(y[1]) * self.current_mix[1]),
        })
    }
}
fn wrap(x: f32) -> f32 {
    x - libm::floorf(x)
}
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}
fn soft(x: f32) -> f32 {
    x / (1.0 + x.abs())
}
fn to_sample(x: f32) -> Sample {
    Sample((f64::from(finite(x)) * 2_147_483_648.0).clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}
fn width(x: [f32; 2], spread: f32) -> [f32; 2] {
    let mid = 0.5 * (x[0] + x[1]);
    let side = 0.5 * (x[0] - x[1]) * spread;
    [mid + side, mid - side]
}
fn bandpass(hz: f32, q: f32, rate: f32) -> BiquadCoefficients {
    let omega = TAU * hz / rate;
    let alpha = libm::sinf(omega) / (2.0 * q);
    let a0 = 1.0 + alpha;
    BiquadCoefficients {
        numerator: [f64::from(alpha / a0), 0.0, f64::from(-alpha / a0)],
        feedback: [
            f64::from(2.0 * libm::cosf(omega) / a0),
            f64::from(-(1.0 - alpha) / a0),
        ],
    }
}
