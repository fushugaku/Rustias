//! Original SH3 OSC2 semitone/fine-tune composition, 01f6c2 and 01f764.
#[derive(Clone, Copy)]
pub struct FineTuneTable {
    pub values: [i16; 128],
}
impl FineTuneTable {
    pub fn lookup(&self, code: i32) -> i32 {
        let code = code.clamp(0, 0x7f00) as u32;
        let index = (code >> 8) as usize;
        let base = self.values[index] as i32;
        let fraction = code & 255;
        if fraction == 0 {
            base
        } else {
            let delta = self.values[index + 1].wrapping_sub(self.values[index]) as i32;
            base + ((delta * fraction as i32) >> 8)
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondaryPitch {
    pub semitone: u8,
    pub fine_tune: u8,
    pub semitone_manual_offset: i16,
    pub fine_manual_offset: i16,
    pub virtual_patch_q16: i32,
}
impl Default for SecondaryPitch {
    fn default() -> Self {
        Self {
            semitone: 64,
            fine_tune: 64,
            semitone_manual_offset: 0,
            fine_manual_offset: 0,
            virtual_patch_q16: 0,
        }
    }
}
impl SecondaryPitch {
    pub fn relative_code(self, table: &FineTuneTable) -> i16 {
        let fine = table.lookup(
            ((self.fine_tune & 127) as i32 - 64) * 256 + self.fine_manual_offset as i32 + 0x4000,
        );
        let semitone =
            ((self.semitone & 127) as i32 - 64) * 256 + self.semitone_manual_offset as i32;
        (fine + semitone + (self.virtual_patch_q16 >> 8)).clamp(-32767, 32767) as i16
    }
}
