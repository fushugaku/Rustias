//! Exact first-oscillator pitch conversion recovered from SYS 2.00 D5D4..D602.
//! The controller's note/modulation conversion is a separate algorithm.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchCode(u16);

impl PitchCode {
    /// Original DSP command parameter: nonnegative Q8 semitones, not MIDI.
    pub const fn new(code: u16) -> Option<Self> {
        if code < 0x8000 {
            Some(Self(code))
        } else {
            None
        }
    }

    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PhaseIncrement(pub u32);

/// Immutable original coefficients supplied by the firmware-data adapter.
pub struct PitchTable {
    pub notes: [u32; 128],
    pub fractions: [i16; 128],
}

impl PitchTable {
    pub fn increment(&self, pitch: PitchCode) -> PhaseIncrement {
        let code = pitch.raw() as usize;
        let base = self.notes[code >> 8] as i64;
        let fraction = self.fractions[(code >> 1) & 127] as i64;
        // The original fractional product truncates, rather than rounding.
        let increment = base + ((base * fraction) >> 15);
        PhaseIncrement(increment.clamp(0, i32::MAX as i64) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Phase;

    #[test]
    fn phase_wraps_and_pitch_rejects_negative_dsp_codes() {
        assert!(PitchCode::new(0x7fff).is_some());
        assert!(PitchCode::new(0x8000).is_none());
        let mut phase = Phase(0xffff_ffff);
        phase.advance(PhaseIncrement(2));
        assert_eq!(phase, Phase(1));
    }
}
