//! Five-phase primary Unison carriers, original Master C584..C977.
use crate::{
    Phase, Sample,
    fixed::{multiply_q15, saturate},
    pitch::PhaseIncrement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnisonParameters {
    pub increments: [PhaseIncrement; 5],
    pub detune: u16,
    pub correction_gain: i16,
    pub level: i16,
}

impl UnisonParameters {
    pub fn retune(&mut self, increment: PhaseIncrement) {
        if let Some(pitch) =
            crate::unison_pitch::UnisonDetuneTable::ORIGINAL.compile(increment, self.detune)
        {
            self.increments = pitch.increments;
            self.correction_gain = crate::unison_pitch::unison_bandwidth(increment).0;
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnisonOscillator {
    pub phases: [Phase; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnisonWaveform {
    Saw,
    Pulse { bandwidth: i16 },
    Triangle,
    Sine { normalization: i32 },
}
impl UnisonOscillator {
    pub fn next_sample(&mut self, p: UnisonParameters) -> Sample {
        self.next_sample_waveform(p, UnisonWaveform::Saw)
    }
    pub fn next_sample_waveform(
        &mut self,
        p: UnisonParameters,
        waveform: UnisonWaveform,
    ) -> Sample {
        let mut sum = 0i64;
        for (phase, increment) in self.phases.iter_mut().zip(p.increments) {
            phase.retreat(increment);
            let value = phase.0 as i32;
            let shaped = match waveform {
                UnisonWaveform::Saw => {
                    let correction = saturate(multiply_q15(value, p.correction_gain) << 8);
                    (value as i64 - correction as i64) as i32
                }
                UnisonWaveform::Pulse { bandwidth } => {
                    let folded = (0x4000_0000i64 - i64::from(value | 1).abs()) as i32;
                    saturate(multiply_q15(folded, bandwidth) << 8)
                }
                UnisonWaveform::Triangle => saturate(0x4000_0000i64 - i64::from(value | 1).abs()),
                UnisonWaveform::Sine { normalization } => {
                    let folded = saturate(i64::from(normalization) - i64::from(value | 1).abs());
                    // The original carrier deliberately uses only the phase's
                    // high word here, unlike a full Q31 multiply.
                    saturate(multiply_q15(folded, (value >> 16) as i16))
                }
            };
            sum += multiply_q15(shaped, p.level);
        }
        Sample(saturate(
            if matches!(
                waveform,
                UnisonWaveform::Triangle | UnisonWaveform::Sine { .. }
            ) {
                sum << 1
            } else {
                sum
            },
        ))
    }
}
