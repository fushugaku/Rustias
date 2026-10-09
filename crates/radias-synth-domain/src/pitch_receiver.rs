//! Immutable coefficient words used by the direct oscillator parameter receiver.
//! This contains data tables, not executable DSP instructions.
#[derive(Clone)]
pub struct PitchReceiverRom {
    pub words: [u16; 0x900],
}
impl PitchReceiverRom {
    pub fn word(&self, address: u16) -> u16 {
        self.words
            .get(usize::from(address.wrapping_sub(0x4000)))
            .copied()
            .unwrap_or(0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondaryPitchCoefficients {
    pub increment: crate::pitch::PhaseIncrement,
    pub edge: i16,
    pub bandwidth: i16,
}
