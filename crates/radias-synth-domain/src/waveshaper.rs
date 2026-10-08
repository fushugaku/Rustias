//! Original Master CB90..D055 Drive and eleven WS kernels, without a CPU.
use crate::{
    Sample,
    fixed::{high_product, multiply_q15, saturate},
    pitch::PhaseIncrement,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriveState {
    pub previous_scaled_input: i32,
    pub previous_output: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveCoefficients {
    pub depth: i16,
    pub normalization: i16,
    pub feedback_gain: i16,
    pub threshold: i16,
    pub curves: [i16; 2],
}
impl DriveCoefficients {
    pub fn gain_target(self) -> i16 {
        (high_product(self.depth, self.normalization) >> 16) as i16
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drive {
    pub state: DriveState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveOutput {
    pub sample: Sample,
    /// Original p87 target; feedback_gain is the separately slewed p88.
    pub gain_target: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaperCoefficients {
    Drive(DriveCoefficients),
    HardClip { depth: i16 },
    Decimator { depth: i16 },
    MultiTriangle { depth: i16 },
    MultiSine { depth: i16 },
    OctSaw { depth: i16 },
    LevelBoost { depth: i16 },
    SubOscillator(SubOscillatorCoefficients),
    Pickup { depth: i16, pitch_current: i16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubOscillatorWaveform {
    Saw,
    Square,
    Triangle,
    Sine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubOscillatorCoefficients {
    pub waveform: SubOscillatorWaveform,
    pub depth: i16,
    pub target_depth: i16,
    pub gain_current: i16,
}
pub struct ShaperTables {
    pub sub_edges: [i16; 129],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaperState {
    pub words: [i32; 4],
    pub startup_counter: u16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Waveshaper {
    pub state: ShaperState,
}
#[derive(Clone, Copy, Debug)]
pub struct ShaperSignal {
    pub input: Sample,
    pub primary_pitch_code: u16,
    pub primary_increment: PhaseIncrement,
}

impl ShaperCoefficients {
    pub fn depth(self) -> i16 {
        match self {
            Self::Drive(c) => c.depth,
            Self::HardClip { depth }
            | Self::Decimator { depth }
            | Self::MultiTriangle { depth }
            | Self::MultiSine { depth }
            | Self::OctSaw { depth }
            | Self::LevelBoost { depth } => depth,
            Self::SubOscillator(c) => c.depth,
            Self::Pickup { depth, .. } => depth,
        }
    }
    pub fn set_depth(&mut self, value: i16) {
        match self {
            Self::Drive(c) => c.depth = value,
            Self::HardClip { depth }
            | Self::Decimator { depth }
            | Self::MultiTriangle { depth }
            | Self::MultiSine { depth }
            | Self::OctSaw { depth }
            | Self::LevelBoost { depth } => *depth = value,
            Self::SubOscillator(c) => c.depth = value,
            Self::Pickup { depth, .. } => *depth = value,
        }
    }
    pub fn gain_current(self) -> Option<i16> {
        match self {
            Self::Drive(c) => Some(c.feedback_gain),
            Self::SubOscillator(c) => Some(c.gain_current),
            Self::Pickup { pitch_current, .. } => Some(pitch_current),
            _ => None,
        }
    }
    pub fn set_gain_current(&mut self, value: i16) {
        match self {
            Self::Drive(c) => c.feedback_gain = value,
            Self::SubOscillator(c) => c.gain_current = value,
            Self::Pickup { pitch_current, .. } => *pitch_current = value,
            _ => {}
        }
    }
    pub fn gain_target(self, pitch: u16) -> Option<i16> {
        match self {
            Self::Drive(c) => Some(c.gain_target()),
            Self::SubOscillator(c) => Some(
                (high_product(
                    match c.waveform {
                        SubOscillatorWaveform::Saw => 6144,
                        SubOscillatorWaveform::Square => 4096,
                        SubOscillatorWaveform::Triangle => 8192,
                        SubOscillatorWaveform::Sine => 32767,
                    },
                    c.target_depth,
                ) >> 16) as i16,
            ),
            Self::Pickup { .. } => Some(pitch as i16),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaperPosition {
    PreFilter,
    PreAmp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaperParameters {
    pub position: ShaperPosition,
    pub coefficients: ShaperCoefficients,
}

impl Waveshaper {
    pub fn process(
        &mut self,
        signal: ShaperSignal,
        c: ShaperCoefficients,
        tables: &ShaperTables,
    ) -> Sample {
        let input = signal.input;
        match c {
            ShaperCoefficients::Drive(c) => {
                let mut drive = Drive {
                    state: DriveState {
                        previous_scaled_input: self.state.words[0],
                        previous_output: self.state.words[1],
                    },
                };
                let output = drive.sample(input, c).sample;
                self.state.words[0] = drive.state.previous_scaled_input;
                self.state.words[1] = drive.state.previous_output;
                output
            }
            ShaperCoefficients::HardClip { depth } => hard_clip(input, depth),
            ShaperCoefficients::Decimator { depth } => {
                let mut drive = Drive {
                    state: DriveState {
                        previous_scaled_input: self.state.words[0],
                        previous_output: self.state.words[1],
                    },
                };
                let output = drive.decimate(input, depth);
                self.state.words[0] = drive.state.previous_scaled_input;
                self.state.words[1] = drive.state.previous_output;
                output
            }
            ShaperCoefficients::MultiTriangle { depth } => multi_triangle(input, depth),
            ShaperCoefficients::MultiSine { depth } => multi_sine(input, depth),
            ShaperCoefficients::OctSaw { depth } => oct_saw(input, depth),
            ShaperCoefficients::LevelBoost { depth } => level_boost(input, depth),
            ShaperCoefficients::SubOscillator(c) => self.sub_oscillator(signal, c, tables),
            ShaperCoefficients::Pickup {
                depth,
                pitch_current,
            } => self.pickup(signal, depth, pitch_current),
        }
    }

    fn sub_oscillator(
        &mut self,
        s: ShaperSignal,
        c: SubOscillatorCoefficients,
        t: &ShaperTables,
    ) -> Sample {
        let half = s.primary_increment.0 >> 1;
        let phase = self.state.words[0].wrapping_sub(half as i32);
        self.state.words[0] = phase;
        let absolute = ((phase | 1) as i64).abs();
        let wave = match c.waveform {
            SubOscillatorWaveform::Square => {
                let distance = (0x40000000 - absolute) as i32;
                saturate(multiply_q15(distance, t.sub_edges[(half >> 24) as usize]) * 256)
            }
            SubOscillatorWaveform::Saw => {
                let slope = (i32::MAX as i64 - absolute) as i32;
                let window =
                    (saturate(multiply_q15(slope, t.sub_edges[(half >> 24) as usize]) * 256) >> 16)
                        as i16;
                saturate(multiply_q15(phase, window))
            }
            SubOscillatorWaveform::Triangle => {
                let folded = ((phase | 1).wrapping_mul(2) | 1) as i64;
                self.state.words[1] = folded.abs() as i32;
                saturate(multiply_q15(
                    self.state.words[1],
                    if phase < 0 { -32768 } else { 32767 },
                ))
            }
            SubOscillatorWaveform::Sine => saturate(multiply_q15(
                phase,
                ((i32::MAX as i64 - absolute) >> 16) as i16,
            )),
        };
        Sample(saturate(
            s.input.0 as i64 + multiply_q15(wave, c.gain_current),
        ))
    }

    fn pickup(&mut self, s: ShaperSignal, depth: i16, pitch: i16) -> Sample {
        let square = |value: i64| ((value >> 16) * (value >> 16) * 2) as i32;
        let frequency = square(0x7fff0000 - ((pitch as i64) << 16)) as i64 + 0x00a30000;
        let frequency = (frequency >> 16) as i16;
        let scaled = saturate(multiply_q15(s.input.0, depth) * 8);
        let driven =
            saturate((multiply_q15(scaled, frequency) + high_product(819, pitch) + 0x028f0000) * 8);
        let absolute = ((driven | 1) as i64).abs();
        let slope = i32::MAX as i64 - ((absolute >> 16) * 26214 * 2) as i32 as i64;
        let second = square(slope);
        let fourth = square(second as i64);
        let alpha = square(i32::MAX as i64 - fourth as i64);
        let mut value =
            self.state.words[0] as i64 - alpha as i64 + multiply_q15(self.state.words[1], 32440);
        self.state.words[0] = alpha;
        if self.state.startup_counter as i16 != 1 {
            value = 0;
            self.state.startup_counter = self.state.startup_counter.wrapping_add(1);
        }
        self.state.words[1] = saturate(value);
        let mut normalization = 0x7fff0000 - ((depth as i64) << 16);
        for _ in 0..4 {
            normalization = square(normalization) as i64;
        }
        let gain = (saturate((normalization >> 1) + 0x40000000) >> 16) as i16;
        Sample(saturate(multiply_q15(self.state.words[1], gain) * 2))
    }
}

impl Drive {
    pub fn decimate(&mut self, input: Sample, depth: i16) -> Sample {
        let ratio = 0x7fff0000 - high_product(27525, depth);
        let square = (high_product((ratio >> 16) as i16, (ratio >> 16) as i16)) as i32;
        let step = (((ratio >> 16) * (square >> 16) as i64) * 2) as i32;
        let old = self.state.previous_scaled_input;
        let count = old.wrapping_sub(step);
        self.state.previous_scaled_input = count;
        if old as i64 - (count as i64) < 0 || depth == 0 {
            self.state.previous_output = input.0;
        }
        Sample(self.state.previous_output)
    }
    pub fn sample(&mut self, input: Sample, c: DriveCoefficients) -> DriveOutput {
        let scaled_input = saturate(multiply_q15(input.0, c.depth) << 2);
        let drive = saturate(
            input.0 as i64
                + self.state.previous_scaled_input as i64
                + multiply_q15(self.state.previous_output, c.feedback_gain),
        );
        let threshold = ((c.threshold as i64 - c.depth as i64) << 16).max(0);
        let excess = ((drive as i64).abs() - threshold).max(0);
        let factor = excess >> 16;
        // The original scalar multiplier truncates its product to signed32,
        // including ABS(i32::MIN)'s positive guard bit, before the next MAC.
        let squared = (factor * factor * 2) as i32;
        let curve = c.curves[usize::from(drive < 0)];
        let correction = high_product((squared >> 16) as i16, curve) as i32;
        // CBDA/CBDD are raw stores. Keep the original wrapping result here.
        let sample = Sample(drive.wrapping_sub(correction));
        self.state = DriveState {
            previous_scaled_input: scaled_input,
            previous_output: sample.0,
        };
        DriveOutput {
            sample,
            gain_target: c.gain_target(),
        }
    }
}

/// Original gain, guard-bit shift, SAT32 and arithmetic division by four.
pub fn hard_clip(input: Sample, depth: i16) -> Sample {
    Sample(saturate(multiply_q15(input.0, depth) << 8) >> 2)
}

pub fn multi_triangle(input: Sample, depth: i16) -> Sample {
    let ramp = (multiply_q15(input.0, depth) * 16) as i32;
    Sample((((ramp.wrapping_add(0x40000000) | 1) as i64).abs() - 0x40000000) as i32)
}

pub fn multi_sine(input: Sample, depth: i16) -> Sample {
    let ramp = (multiply_q15(input.0, depth) * 16) as i32;
    let slope = i32::MAX as i64 - ((ramp | 1) as i64).abs();
    Sample(saturate(multiply_q15(ramp, (slope >> 16) as i16) * 2))
}

pub fn oct_saw(input: Sample, depth: i16) -> Sample {
    let mut value = saturate(input.0 as i64 * 4) as i64;
    if depth <= 16383 {
        let displacement = (depth as i64) << 17;
        if value > 0x3fff0000 {
            value -= displacement;
        }
        if value + 0x3fff0000 < 0 {
            value += displacement;
        }
    } else {
        let displacement = ((depth as i64) - 16383) << 16;
        if value >= 0 {
            if value > 0x5fff0000 {
                value -= i32::MAX as i64;
            } else if value > 0x3fff0000 {
                value += displacement - i32::MAX as i64;
            } else if value > 0x1fff0000 {
                value -= displacement;
            }
        } else if value < -0x60000000 {
            value += i32::MAX as i64;
        } else if value < -0x40000000 {
            value += i32::MAX as i64 - displacement;
        } else if value < -0x20000000 {
            value += displacement;
        }
    }
    Sample(saturate(multiply_q15(saturate(value), 8192 + (depth >> 2))))
}

pub fn level_boost(input: Sample, depth: i16) -> Sample {
    let driven = saturate(multiply_q15(input.0, depth) * 4);
    let normalization = if driven < 0 { -i32::MAX } else { i32::MAX };
    let excess = (((driven | 1) as i64).abs() - 0x36190000).max(0);
    let factor = excess >> 16;
    let square = (factor * factor * 2) as i32;
    let cube = ((square >> 16) as i64 * factor * 2) as i32;
    let correction = (high_product((normalization >> 16) as i16, (cube >> 16) as i16)) as i32;
    let shaped = saturate(driven as i64 - correction as i64);
    Sample(saturate(
        shaped as i64 + (high_product((shaped >> 16) as i16, 7809) as i32) as i64,
    ))
}
