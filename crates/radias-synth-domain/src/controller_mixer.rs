//! SH3 oscillator level composition and squared DSP gain, 0026b6..002c1a.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerLevel {
    pub level: u8,
    pub manual_offset: i16,
    pub modulation: i16,
    pub scale: u16,
}
impl MixerLevel {
    pub fn composed(self) -> u16 {
        ((((self.level as i8 as i32) << 8)
            + self.manual_offset as i32
            + self.modulation as i32 * 2)
            * 2)
        .clamp(0, 65535) as u16
    }
    pub fn gain(self) -> i16 {
        let value = self.composed() as u32;
        let squared = (value * value) >> 16;
        ((squared * self.scale as u32) >> 16) as i16
    }
}
#[derive(Clone, Copy)]
pub struct MixerScales {
    pub primary: [u16; 64],
    pub secondary: [[u16; 4]; 2],
}
impl MixerScales {
    pub fn primary(&self, selection: u8) -> u16 {
        self.primary[((selection & 15) as usize * 4) + ((selection >> 4) & 3) as usize]
    }
    pub fn secondary(&self, selection: u8) -> u16 {
        self.secondary[usize::from(selection & 16 != 0)][(selection & 3) as usize]
    }
}
