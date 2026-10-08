//! OSC2 waveform/modulation and original relative-pitch use case.
use radias_synth_domain::{
    controller_secondary::{FineTuneTable, SecondaryPitch},
    pitch::PitchCode,
    secondary_control::SecondaryModulation,
    waveform::Transfer,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SecondaryProgram {
    pub selection: u8,
    pub pitch: SecondaryPitch,
}
impl SecondaryProgram {
    pub fn modulation(self) -> SecondaryModulation {
        SecondaryModulation {
            ring: self.selection & 16 != 0,
            sync: self.selection & 32 != 0,
        }
    }
    pub fn transfer(self) -> Transfer {
        [
            Transfer::CorrectedRamp,
            Transfer::Pulse,
            Transfer::FoldedTriangle,
            Transfer::ParabolicSine,
        ][(self.selection & 3) as usize]
    }
    pub fn code(self, primary: PitchCode, table: &FineTuneTable, modulation: i32) -> PitchCode {
        let mut pitch = self.pitch;
        pitch.virtual_patch_q16 = modulation;
        PitchCode::new(
            (primary.raw() as i32 + pitch.relative_code(table) as i32).clamp(0, 32767) as u16,
        )
        .unwrap()
    }
}
