//! Primary pitch composition, original SH3 01f586..01f5d6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerPitch {
    pub base_q16: i32,
    pub vibrato_depth: i32,
    pub lfo2: i16,
    pub virtual_patch_q16: i32,
}
impl ControllerPitch {
    pub fn code(self) -> u16 {
        let gain = (self.vibrato_depth >> 7) as i16 as i32;
        let vibrato = (gain * self.lfo2 as i32) >> 8;
        let value = self
            .base_q16
            .wrapping_add(vibrato)
            .wrapping_add(self.virtual_patch_q16)
            .wrapping_add(128);
        (value >> 8).clamp(0, 32767) as u16
    }
}
