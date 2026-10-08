//! Four original fixed-point waveform transfer functions, SYS 2.00 B2AC..B453.
//! These transform a supplied phase block; voice allocation and phase generation
//! belong to the oscillator/voice model, rather than an instruction dispatcher.
use crate::Sample;

const ONE: i32 = 0x7fff_0000;

/// Mathematical transfer functions; patch selector dispatch is qualified separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    CorrectedRamp,
    Pulse,
    ParabolicSine,
    FoldedTriangle,
}

/// Coefficients prepared by the original controller/DSP parameter path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeParameters {
    pub subtract_edge: bool,
    pub edge_coefficient: i16,
    pub waveform_control: i16,
    pub gain: i16,
}

/// Original interpolation table, including both boundary neighbors.
pub struct WaveformTable {
    pub correction: [i16; 129],
    pub shapers: crate::waveshaper::ShaperTables,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveformFrame {
    pub transfer: Transfer,
    pub phase: i32,
    pub edge_phase: i32,
    pub parameters: ShapeParameters,
}

#[inline]
fn sat(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}
#[inline]
fn add(left: i32, right: i32) -> i32 {
    sat(left as i64 + right as i64)
}
#[inline]
fn sub(left: i32, right: i32) -> i32 {
    sat(left as i64 - right as i64)
}
#[inline]
fn shift(value: i32, bits: u32) -> i32 {
    sat((value as i64) << bits)
}
#[inline]
fn absolute(value: i32) -> i32 {
    value.saturating_abs()
}

/// Original scalar multiplier: signed 17-bit high operand, doubled product,
/// 32-bit multiplier output, then arithmetic saturation. No float conversion.
#[inline]
fn scalar(value: i32, coefficient: i16) -> i32 {
    let high = (value as i64) >> 16;
    let product = high * coefficient as i64 * 2;
    if high == -32768 && coefficient == -32768 {
        return i32::MAX;
    }
    product as i32
}

#[inline]
pub fn scale(sample: Sample, coefficient: i16) -> Sample {
    // The original splits Q31 into an unsigned low half and signed high
    // half. SMUL saturates the -32768 * -32768 high product before adding
    // the shifted low product; a single full-width multiply loses one bit.
    let low = (sample.0 as u32 & 65535) as i64 * coefficient as i64 * 2;
    let high = sample.0 >> 16;
    let high_product = if high == -32768 && coefficient == -32768 {
        i32::MAX as i64
    } else {
        high as i64 * coefficient as i64 * 2
    };
    Sample(sat(high_product + (low >> 16)))
}

#[inline]
fn edge_window(distance: i32, parameters: ShapeParameters, shift_bits: u32) -> i32 {
    let product = scalar(distance, parameters.edge_coefficient);
    let ramp = sub(ONE, shift(product, shift_bits));
    if parameters.subtract_edge {
        sub(ONE, scalar(ramp, (ramp >> 16) as i16))
    } else {
        ONE
    }
}

impl WaveformTable {
    pub fn correction_at(&self, coordinate: i32) -> i64 {
        let index = ((coordinate >> 25) + 64) as usize;
        let fraction = (coordinate >> 10) & 32767;
        let left = (self.correction[index] as i64) << 16;
        let delta = ((self.correction[index + 1] as i64) << 16) - left;
        let product = ((delta >> 16) * fraction as i64 * 2) as i32;
        left + product as i64
    }

    pub fn sample(
        &self,
        transfer: Transfer,
        phase: i32,
        edge_phase: i32,
        p: ShapeParameters,
    ) -> Sample {
        // The entry's two 40-bit adds are stored as 32 bits before M40 is
        // cleared; overflow here wraps. Subsequent arithmetic saturates.
        let edge = absolute(edge_phase.wrapping_add(ONE) | 1);
        let window = edge_window(edge, p, 10);
        let shaped = match transfer {
            Transfer::CorrectedRamp => {
                let shifted_phase = phase.wrapping_add(ONE);
                let coordinate = shift(scalar(shifted_phase, p.waveform_control), 7);
                let index = ((coordinate >> 25) + 64) as usize;
                let fraction = (coordinate >> 10) & 32767;
                let left = (self.correction[index] as i32) << 16;
                let right = (self.correction[index + 1] as i32) << 16;
                let delta = sub(right, left);
                let interpolated = sat(left as i64 + ((delta as i64 >> 16) * fraction as i64 * 2));
                let value = add(interpolated, shifted_phase);
                scale(Sample(value), (window >> 16) as i16)
            }
            Transfer::Pulse => {
                let doubled = phase.wrapping_mul(2);
                let limiting_edge = if p.subtract_edge { edge } else { ONE };
                let distance = absolute(doubled | 1).min(limiting_edge);
                // This transfer always subtracts the squared ramp.
                let pulse_window = edge_window(
                    distance,
                    ShapeParameters {
                        subtract_edge: true,
                        ..p
                    },
                    8,
                );
                scale(Sample(shift(phase, 31)), (pulse_window >> 16) as i16)
            }
            Transfer::ParabolicSine => {
                let slope = sub(ONE, absolute(phase | 1));
                let value = shift(scale(Sample(phase), (slope >> 16) as i16).0, 2);
                scale(Sample(value), (window >> 16) as i16)
            }
            Transfer::FoldedTriangle => {
                let biased_phase = phase | 1;
                let sign = (shift(biased_phase, 31) >> 16) as i16;
                let folded = absolute(biased_phase.wrapping_mul(2) | 1);
                let value = scale(Sample(folded), sign);
                scale(value, (window >> 16) as i16)
            }
        };
        scale(shaped, p.gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_signed_and_phase_boundary_goldens() {
        // Captured from unchanged Master B320/B380/B3E8, ST1=6740.
        // These transfers do not read the interpolation table.
        let table = WaveformTable {
            correction: [0; 129],
            shapers: crate::waveshaper::ShaperTables {
                sub_edges: [0; 129],
            },
        };
        let cases = [
            (
                Transfer::Pulse,
                -65536,
                -1,
                true,
                32766u16,
                1u16,
                32768u16,
                33423360,
            ),
            (
                Transfer::ParabolicSine,
                -1,
                i32::MAX,
                false,
                32767,
                65535,
                49152,
                2,
            ),
            (
                Transfer::ParabolicSine,
                i32::MAX,
                0,
                true,
                16384,
                32766,
                1,
                -8,
            ),
            (
                Transfer::FoldedTriangle,
                i32::MIN + 1,
                1,
                false,
                49152,
                0,
                32767,
                -3,
            ),
            (
                Transfer::FoldedTriangle,
                -65536,
                -1,
                true,
                32766,
                1,
                32768,
                131062,
            ),
        ];
        for (transfer, phase, edge, subtract_edge, coefficient, control, gain, expected) in cases {
            let parameters = ShapeParameters {
                subtract_edge,
                edge_coefficient: coefficient as i16,
                waveform_control: control as i16,
                gain: gain as i16,
            };
            assert_eq!(
                table.sample(transfer, phase, edge, parameters),
                Sample(expected)
            );
        }
    }
}
