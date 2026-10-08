//! Original OSC1 controller composition and mode dispatch, SYS 0206ee/020b64.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PrimaryControl {
    pub control1: u8,
    pub control2: u8,
    pub control1_manual_offset: i16,
    pub control1_modulation: i16,
    pub control2_modulation: i16,
    pub control2_manual_offset: i8,
    pub lfo1: i16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryControlState {
    pub base: i32,
    pub curved: i32,
    pub linear: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryTarget {
    Waveform(i16),
    Cross(i16),
    Unison(i16),
    Vpm(i16),
}
impl PrimaryControl {
    /// Original VPM ratio table, SYS 020f8e..020fbe / 04286a.
    pub fn vpm_ratio(self) -> i16 {
        let code = (i32::from(self.control2 & 127)
            + i32::from(self.control2_modulation)
            + i32::from(self.control2_manual_offset))
        .clamp(0, 127);
        (((code >> 2) + 1) * 512) as i16
    }
    /// CTRL2 phase depth sent by the Unison initializer, independently of LFO.
    pub fn phase_code(self) -> u16 {
        ((i32::from(self.control2 & 127)
            + i32::from(self.control2_modulation)
            + i32::from(self.control2_manual_offset))
        .clamp(0, 127)
            * 258) as u16
    }
    pub fn compose(self) -> PrimaryControlState {
        let depth = ((self.control2 & 127) as i32
            + self.control2_modulation as i32
            + self.control2_manual_offset as i32)
            .clamp(0, 127)
            * 2;
        let tracked = (depth * self.lfo1 as i32) >> 8;
        // Original muls.w narrows the first tracking product before squaring
        // the depth; omitting this truncation changes extreme controls.
        let curved = ((tracked as i16 as i32) * depth) >> 8;
        let base = self.control1_manual_offset as i32
            + self.control1_modulation as i32 * 2
            + (self.control1 & 127) as i32 * 258;
        PrimaryControlState {
            base,
            curved: base + curved,
            linear: base + tracked,
        }
    }
}
impl PrimaryControlState {
    pub fn target(self, selection: u8) -> Option<PrimaryTarget> {
        if selection & !0x33 != 0 {
            return None;
        }
        let positive = |x: i32| x.clamp(0, 32767) as i64;
        Some(match selection & 48 {
            0 => PrimaryTarget::Waveform(match selection & 3 {
                0 | 1 => self.curved.clamp(-32767, 32767) as i16,
                2 => ((positive(self.curved) * 0x553f) >> 15) as i16,
                _ => {
                    let x = positive(self.linear);
                    ((x * x) >> 15) as i16
                }
            }),
            16 => {
                let x = positive(self.curved);
                PrimaryTarget::Cross(((x * x) >> 15) as i16)
            }
            32 => {
                let x = positive(self.base);
                PrimaryTarget::Unison(((x * x * x) >> 30).clamp(-32767, 32767) as i16)
            }
            _ => {
                let x = positive(self.base);
                PrimaryTarget::Vpm(((x * x) >> 16) as i16)
            }
        })
    }
}
