//! Direct fixed-point vocoder filter banks from the original Master DSP.
//! No instruction decoder or firmware execution is used by these sample kernels.
use crate::fixed::{multiply_q15, multiply_q31, saturate};
use crate::{
    Sample,
    fixed::{high_product, weighted_sum},
    pan::StereoFrame,
};

pub const BANDS: usize = 16;
pub const ANALYSIS_STATE_WORDS: usize = BANDS * 8 + 4;
pub const PARAMETER_WORDS: usize = 0x160;
pub const STATE_WORDS: usize = 0x12c;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vocoder {
    /// Original parameter/control publications, without executable code.
    pub parameters: [u16; PARAMETER_WORDS],
    /// Continuous front-end, analysis, envelope and carrier filter histories.
    pub state: [u16; STATE_WORDS],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpolationTables {
    pub scalar_offsets: [u16; 38],
    pub wide_offset: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VocoderFrame {
    pub samples: [i32; 17],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VocoderError {
    FrameRoute,
    FrequencyRoute,
    InterpolationRoute,
}
fn pair(words: &[u16], high: usize) -> i32 {
    ((u32::from(words[high]) << 16) | u32::from(words[high ^ 1])) as i32
}
fn put(words: &mut [u16], high: usize, value: i64) {
    let value = saturate(value) as u32;
    words[high] = (value >> 16) as u16;
    words[high ^ 1] = value as u16;
}
impl VocoderFrame {
    /// A333's four completed Master stereo buses start at frame word eight.
    /// Codec input occupies the first stereo pair; the caller owns its clock.
    pub fn from_buses(buses: [StereoFrame; 4], input: StereoFrame) -> Self {
        Self::from_sources(buses, [input, StereoFrame::default()])
    }
    /// Both input pairs remain separate so every original modulator and
    /// auxiliary-carrier route can select its actual frame lanes.
    pub fn from_sources(buses: [StereoFrame; 4], inputs: [StereoFrame; 2]) -> Self {
        let mut samples = [0; 17];
        for (index, input) in inputs.iter().enumerate() {
            samples[2 * index] = input.left.0;
            samples[2 * index + 1] = input.right.0;
        }
        for (index, bus) in buses.iter().enumerate() {
            samples[4 + 2 * index] = bus.left.0;
            samples[5 + 2 * index] = bus.right.0;
        }
        Self { samples }
    }
    /// Preserve all four pairs after the original routed stores. The vocoder
    /// overwrites its selected pair; unrelated timbres remain in the frame.
    pub fn buses(&self) -> [StereoFrame; 4] {
        core::array::from_fn(|index| StereoFrame {
            left: Sample(self.samples[4 + 2 * index]),
            right: Sample(self.samples[5 + 2 * index]),
        })
    }
    fn read(&self, word: usize) -> Result<i32, VocoderError> {
        let value = *self.samples.get(word / 2).ok_or(VocoderError::FrameRoute)?;
        Ok(if word & 1 == 0 {
            value
        } else {
            value.rotate_left(16)
        })
    }
    fn write(&mut self, word: usize, value: i32) -> Result<(), VocoderError> {
        let destination = self
            .samples
            .get_mut(word / 2)
            .ok_or(VocoderError::FrameRoute)?;
        *destination = if word & 1 == 0 {
            value
        } else {
            value.rotate_left(16)
        };
        Ok(())
    }
}
impl Vocoder {
    /// Original opcode37 replaces only each analysis output's high word.
    pub fn publish_formant(&mut self, bands: [u16; BANDS]) {
        for (band, value) in bands.into_iter().enumerate() {
            self.state[0x1a + 8 * band] = value;
        }
    }
    /// Original opcode35 parameter priming. Retain the sixteen band envelopes
    /// and the two unused trailing words, as the original clear ranges do.
    pub fn initialize(&mut self) {
        for source in [0xb8, 0xba, 0xbf, 0xf2] {
            self.parameters[source + 1] = self.parameters[source];
        }
        self.parameters[0xf6] = self.parameters[0xf4];
        self.parameters[0xf7] = self.parameters[0xf5];
        for source in (0x118..=0x15c).step_by(2) {
            self.parameters[source + 1] = self.parameters[source];
        }
        self.state[..0x94].fill(0);
        self.state[0xa4..0x12a].fill(0);
    }
    fn carrier(&mut self, frame: &VocoderFrame) -> Result<i32, VocoderError> {
        let p = &mut self.parameters;
        let bus = (i64::from(frame.read(usize::from(p[0xb4]))?)
            + i64::from(frame.read(usize::from(p[0xb5]))?))
            >> 1;
        let auxiliary = (i64::from(frame.read(usize::from(p[0xb6]))?)
            + i64::from(frame.read(usize::from(p[0xb7]))?))
            >> 1;
        let bus = 2 * multiply_q15(bus as i32, p[0xb9] as i16);
        let auxiliary = saturate(multiply_q15(auxiliary as i32, p[0xbb] as i16));
        let carrier = saturate(bus + multiply_q15(auxiliary, p[0xbc] as i16));
        put(&mut self.state, 0xa4, i64::from(carrier));

        // D2D9..D32A: signed shift smoothing and adjacent table interpolation.
        let shift = high_product(p[0xbd] as i16, p[0xbf] as i16)
            + high_product(p[0xbe] as i16, p[0xc0] as i16);
        p[0xc0] = (saturate(shift) >> 16) as u16;
        let direction = if shift < 0 { -4 } else { 4 };
        let magnitude = (saturate(shift) | 1).unsigned_abs() as i32;
        let start = 8i32 + i32::from(p[0xc1] as i16);
        for band in 0..BANDS {
            let first = 0xc2i32 + start + 2 * band as i32;
            let second = first + direction;
            if first < 0
                || second < 0
                || first as usize ^ 1 >= PARAMETER_WORDS
                || second as usize ^ 1 >= PARAMETER_WORDS
            {
                return Err(VocoderError::FrequencyRoute);
            }
            let base = pair(p, first as usize);
            let delta = i64::from(pair(p, second as usize)) - i64::from(base);
            let high = (saturate(delta) >> 16) as i16;
            put(
                p,
                0xf8 + 2 * band,
                i64::from(base) + multiply_q15(magnitude, high),
            );
        }
        Ok(saturate(multiply_q15(carrier, p[0xf3] as i16)))
    }
    fn interpolate(&mut self, tables: &InterpolationTables) -> Result<(), VocoderError> {
        let weights = [self.parameters[0x15e] as i16, self.parameters[0x15f] as i16];
        for offset in tables.scalar_offsets {
            let at = usize::from(offset);
            if at + 1 >= PARAMETER_WORDS {
                return Err(VocoderError::InterpolationRoute);
            }
            let value = high_product(self.parameters[at] as i16, weights[0])
                + high_product(self.parameters[at + 1] as i16, weights[1]);
            self.parameters[at + 1] = (saturate(value) >> 16) as u16;
        }
        let at = usize::from(tables.wide_offset);
        if at == 0 || at + 2 >= PARAMETER_WORDS {
            return Err(VocoderError::InterpolationRoute);
        }
        let first =
            ((u32::from(self.parameters[at - 1]) << 16) | u32::from(self.parameters[at])) as i32;
        let second = ((u32::from(self.parameters[at + 1]) << 16)
            | u32::from(self.parameters[at + 2])) as i32;
        put(
            &mut self.parameters,
            at + 1,
            weighted_sum([first, second], weights),
        );
        Ok(())
    }
    /// Direct composition of the complete D05C..D52A sample body. The caller
    /// supplies the original job's interpolation-enable decision and immutable
    /// publication offsets; this does not invent an IRQ or physical clock.
    pub fn process(
        &mut self,
        frame: &mut VocoderFrame,
        interpolate: bool,
        tables: &InterpolationTables,
    ) -> Result<StereoFrame, VocoderError> {
        let mut input = AnalysisInput {
            words: self.state[..20].try_into().unwrap(),
        };
        let p = &self.parameters;
        input.process(
            [
                frame.read(2 + usize::from(p[1]))?,
                frame.read(2 + usize::from(p[2]))?,
            ],
            AnalysisInputParameters {
                input_gain: p[3] as i16,
                attack: pair(p, 4),
                release: pair(p, 6),
                gate: pair(p, 8),
                high_pass_gain: p[10] as i16,
                high_pass_feedback: p[11] as i16,
                high_pass_frequency: p[12] as i16,
                high_pass_attack: p[13] as i16,
                high_pass_release: p[14] as i16,
            },
        );
        self.state[..20].copy_from_slice(&input.words);
        if self.parameters[15] == 0 {
            let coefficients = AnalysisCoefficients {
                bands: core::array::from_fn(|band| {
                    core::array::from_fn(|coefficient| {
                        pair(&self.parameters, 16 + 10 * band + 2 * coefficient)
                    })
                }),
            };
            let mut bank = AnalysisFilterBank {
                words: self.state[20..20 + ANALYSIS_STATE_WORDS]
                    .try_into()
                    .unwrap(),
            };
            bank.process(input.gated(), &coefficients);
            self.state[20..20 + ANALYSIS_STATE_WORDS].copy_from_slice(&bank.words);
        }
        if self.parameters[15] != 1 {
            let mut envelopes = BandEnvelopes {
                levels: self.state[0x94..0xa4].try_into().unwrap(),
            };
            envelopes.update(
                core::array::from_fn(|band| pair(&self.state, 0x1a + 8 * band)),
                pair(&self.parameters, 0xb0),
                pair(&self.parameters, 0xb2),
            );
            self.state[0x94..0xa4].copy_from_slice(&envelopes.levels);
        }
        let carrier = self.carrier(frame)?;
        let coefficients = SynthesisCoefficients {
            damping: pair(&self.parameters, 0xf6),
            frequencies: core::array::from_fn(|band| pair(&self.parameters, 0xf8 + 2 * band)),
        };
        let mut bank = SynthesisFilterBank {
            states: core::array::from_fn(|band| {
                core::array::from_fn(|field| pair(&self.state, 0xa6 + 8 * band + 2 * field))
            }),
        };
        bank.process(carrier, &coefficients);
        for (band, states) in bank.states.iter().enumerate() {
            for (field, value) in states.iter().enumerate() {
                put(
                    &mut self.state,
                    0xa6 + 8 * band + 2 * field,
                    i64::from(*value),
                );
            }
        }
        let mut left = 0i64;
        let mut right = 0i64;
        for (band, sample) in bank.outputs().iter().enumerate() {
            let envelope = self.state[0x94 + band] as i16;
            let level = self.parameters[0x119 + 4 * band] as i16;
            let pan = self.parameters[0x11b + 4 * band] as i16;
            let weighted = multiply_q15(multiply_q15(*sample, envelope) as i32, level) as i32;
            let complement = (saturate((32767 - i64::from(pan)) << 16) >> 16) as i16;
            right += multiply_q15(weighted, pan);
            left += multiply_q15(weighted, complement);
        }
        let high_pass = multiply_q15(input.high_pass(), self.parameters[0x159] as i16);
        left += high_pass + multiply_q15(pair(&self.state, 0), self.parameters[0x15b] as i16);
        right += high_pass + multiply_q15(pair(&self.state, 2), self.parameters[0x15b] as i16);
        let gain = self.parameters[0x15d] as i16;
        let left = saturate(multiply_q15(saturate(left), gain));
        let right = saturate(multiply_q15(saturate(right), gain));
        put(&mut self.state, 0x126, i64::from(left));
        put(&mut self.state, 0x128, i64::from(right));
        if interpolate {
            self.interpolate(tables)?;
        }
        let left_route = usize::from(self.parameters[0xb4]);
        let right_route = usize::from(self.parameters[0xb5]);
        frame.write(left_route, saturate(i64::from(left) << 2))?;
        frame.write(right_route, saturate(i64::from(right) << 2))?;
        Ok(StereoFrame {
            left: Sample(frame.read(left_route)?),
            right: Sample(frame.read(right_route)?),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BandEnvelopes {
    pub levels: [u16; BANDS],
}
impl BandEnvelopes {
    /// Whole D219..D270, including separate rising/falling Q31 rates and
    /// truncation to the source's signed high-word envelope bank.
    pub fn update(&mut self, input: [i32; BANDS], attack: i32, release: i32) {
        for (level, input) in self.levels.iter_mut().zip(input) {
            let rectified = (i64::from(input) | 1).abs();
            let difference = rectified - (i64::from(*level as i16) << 16);
            let rate = if difference < 0 { release } else { attack };
            *level = (saturate(rectified - multiply_q31(rate, difference as i32)) >> 16) as u16;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynthesisCoefficients {
    pub damping: i32,
    pub frequencies: [i32; BANDS],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SynthesisFilterBank {
    pub states: [[i32; 4]; BANDS],
}
impl SynthesisFilterBank {
    /// Entire D350..D3F2 carrier bank: two cascaded state-variable sections
    /// per band. Every intermediate store retains original Q31 saturation.
    pub fn process(&mut self, carrier: i32, coefficients: &SynthesisCoefficients) {
        for (state, frequency) in self.states.iter_mut().zip(coefficients.frequencies) {
            let drive = saturate(
                i64::from(carrier)
                    - multiply_q31(state[0], coefficients.damping)
                    - i64::from(state[1]),
            );
            state[0] = saturate(2 * multiply_q31(drive, frequency) + i64::from(state[0]));
            state[1] = saturate(2 * multiply_q31(state[0], frequency) + i64::from(state[1]));
            let drive = saturate(
                i64::from(state[0])
                    - multiply_q31(state[2], coefficients.damping)
                    - i64::from(state[3]),
            );
            state[2] = saturate(2 * multiply_q31(drive, frequency) + i64::from(state[2]));
            state[3] = saturate(2 * multiply_q31(state[2], frequency) + i64::from(state[3]));
        }
    }
    pub fn outputs(&self) -> [i32; BANDS] {
        self.states.map(|state| state[2])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisInputParameters {
    pub input_gain: i16,
    pub attack: i32,
    pub release: i32,
    pub gate: i32,
    pub high_pass_gain: i16,
    pub high_pass_feedback: i16,
    pub high_pass_frequency: i16,
    pub high_pass_attack: i16,
    pub high_pass_release: i16,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalysisInput {
    pub words: [u16; 20],
}
impl AnalysisInput {
    fn read(&self, high: usize) -> i32 {
        ((u32::from(self.words[high]) << 16) | u32::from(self.words[high ^ 1])) as i32
    }
    fn write(&mut self, high: usize, value: i64) {
        let value = saturate(value) as u32;
        self.words[high] = (value >> 16) as u16;
        self.words[high ^ 1] = value as u16;
    }
    /// Original D05C..D175: input gain, envelope gate, high-pass state and
    /// high-pass envelope. The MASM smoother consumes the signed17-bit high
    /// accumulator part; replacing it with a full Q31 multiply changes sound.
    pub fn process(&mut self, input: [i32; 2], p: AnalysisInputParameters) {
        let left = multiply_q15(input[0], p.input_gain);
        let right = multiply_q15(input[1], p.input_gain);
        self.write(0, left);
        self.write(2, right);
        let mono = (left + right) >> 1;
        self.write(4, mono);
        let rectified = (i64::from(self.read(4)) | 1).abs();
        let difference = rectified - i64::from(self.read(6));
        let rate = if difference < 0 { p.release } else { p.attack };
        self.write(6, rectified - multiply_q31(difference as i32, rate));
        let gated = if i64::from(self.read(6)) - i64::from(p.gate) < 0 {
            0
        } else {
            mono
        };
        self.write(8, gated);
        let drive = multiply_q15(self.read(8), p.high_pass_gain)
            - multiply_q15(self.read(10), p.high_pass_feedback)
            - i64::from(self.read(12));
        self.write(14, drive);
        let first =
            2 * multiply_q15(self.read(14), p.high_pass_frequency) + i64::from(self.read(10));
        self.write(10, first);
        let second =
            2 * multiply_q15(self.read(10), p.high_pass_frequency) + i64::from(self.read(12));
        self.write(12, second);
        let rectified = (i64::from(self.read(14)) | 1).abs();
        let difference = rectified - i64::from(self.read(16));
        let rate = if difference < 0 {
            p.high_pass_release
        } else {
            p.high_pass_attack
        };
        let mut high = (difference >> 16) & 0x1ffff;
        if high & 0x10000 != 0 {
            high -= 0x20000;
        }
        let product = if high == -32768 && rate == i16::MIN {
            i32::MAX
        } else {
            (high * i64::from(rate) * 2) as i32
        };
        self.write(16, rectified - i64::from(product));
        let output = multiply_q15(self.read(14), self.words[16] as i16);
        self.write(18, i64::from(saturate(output << 7)) << 3);
    }
    pub fn gated(&self) -> i32 {
        self.read(8)
    }
    pub fn high_pass(&self) -> i32 {
        self.read(18)
    }
}

/// Five original Q31 coefficients per band, in the D194 publication order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisCoefficients {
    pub bands: [[i32; 5]; BANDS],
}

/// Preserve the original overlapping halfword history and saturated stores.
/// Converting it to four independent i32 histories changes the source behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisFilterBank {
    pub words: [u16; ANALYSIS_STATE_WORDS],
}
impl Default for AnalysisFilterBank {
    fn default() -> Self {
        Self {
            words: [0; ANALYSIS_STATE_WORDS],
        }
    }
}
impl AnalysisFilterBank {
    fn read(&self, high: usize) -> i32 {
        ((u32::from(self.words[high]) << 16) | u32::from(self.words[high ^ 1])) as i32
    }
    fn product(&self, coefficient: i32, low: usize) -> i64 {
        let value = ((u32::from(self.words[low - 1]) << 16) | u32::from(self.words[low])) as i32;
        multiply_q31(coefficient, value)
    }
    fn write(&mut self, high: usize, value: i64) {
        let value = saturate(value) as u32;
        self.words[high] = (value >> 16) as u16;
        self.words[high ^ 1] = value as u16;
    }

    /// Entire sixteen-band D18E..D207 loop. The source's parallel history
    /// store happens before the next accumulator addition.
    pub fn process(&mut self, input: i32, coefficients: &AnalysisCoefficients) {
        let mut cursor = 1;
        for c in coefficients.bands {
            let mut first = multiply_q31(c[0], input);
            first += self.product(c[1], cursor);
            cursor -= 1;
            let previous = i64::from(self.read(cursor));
            cursor += 3;
            first += self.product(c[2], cursor);
            cursor -= 1;
            let history = self.read(cursor);
            cursor -= 2;
            first *= 2;
            let residual = previous - 2 * i64::from(history);
            self.write(cursor, i64::from(history));
            cursor += 2;
            self.write(cursor, first);
            cursor += 3;
            first += residual;
            first += self.product(c[3], cursor);
            cursor -= 1;
            cursor += 3;
            first += self.product(c[4], cursor);
            cursor -= 1;
            let history = self.read(cursor);
            cursor -= 2;
            first *= 2;
            self.write(cursor, i64::from(history));
            cursor += 2;
            self.write(cursor, first);
            cursor += 3;
        }
    }
    pub fn outputs(&self) -> [i32; BANDS] {
        core::array::from_fn(|band| self.read(6 + 8 * band))
    }
}
