//! OSC1 controller inputs compile independently of oscillator state and devices.
use radias_synth_domain::controller_primary::{PrimaryControl, PrimaryTarget};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PrimaryProgram {
    pub selection: u8,
    pub control: PrimaryControl,
}
impl PrimaryProgram {
    pub fn compile_waveform(
        self,
        increment: radias_synth_domain::pitch::PhaseIncrement,
        bandwidth: i16,
    ) -> Option<radias_synth_domain::primary_oscillator::PrimaryParameters> {
        use radias_synth_domain::primary_oscillator::PrimaryParameters;
        match self.selection {
            0..=3 => PrimaryParameters::waveform(self.selection, increment, bandwidth),
            4 | 5 => PrimaryParameters::noise_waveform(self.selection, increment),
            16..=19 => PrimaryParameters::cross_waveform(self.selection & 3, increment, bandwidth),
            32..=35 => PrimaryParameters::unison_waveform(self.selection & 3, increment),
            48..=51 => PrimaryParameters::vpm_waveform(self.selection & 3, increment, bandwidth),
            _ => None,
        }
    }
    pub fn target(self, lfo1: i16, modulation: [i16; 2]) -> Option<PrimaryTarget> {
        let mut control = self.control;
        control.lfo1 = lfo1;
        control.control1_modulation = modulation[0];
        control.control2_modulation = modulation[1];
        control.compose().target(self.selection)
    }
    /// Live Waveform, Cross, Unison and VPM controller encodings.
    pub fn waveform_control(self, lfo1: i16, modulation: [i16; 2]) -> Option<i16> {
        if self.selection & !0x33 != 0 {
            return None;
        }
        match self.target(lfo1, modulation)? {
            PrimaryTarget::Waveform(v)
            | PrimaryTarget::Cross(v)
            | PrimaryTarget::Unison(v)
            | PrimaryTarget::Vpm(v) => Some(v),
        }
    }
}
