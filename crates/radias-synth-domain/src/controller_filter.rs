//! Original SH3 Filter1 cutoff composition, 01b648/01b8be/01bb6e.
use crate::amplifier_control::AmplifierTables;

#[derive(Clone, Copy)]
pub struct ControllerFilterTables {
    pub frequency: [u32; 164],
    pub key_depth: [i16; 128],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControllerFilter {
    pub cutoff: u8,
    pub cutoff_offset: i16,
    pub lfo_offset: i16,
    pub key_tracking: u8,
    pub key_manual_offset: i8,
    pub key_modulation: i16,
    pub relative_pitch: i16,
    pub eg1_intensity: u8,
    pub eg1_manual_offset: i8,
    pub eg1_depth_modulation: i16,
    pub eg1_level: u16,
    pub velocity: u8,
    pub eg1_velocity_sensitivity: u8,
    pub additional_offset: i16,
    pub cutoff_modulation: i16,
}
impl ControllerFilterTables {
    pub fn frequency(&self, code: i32) -> u32 {
        let code = code.wrapping_add(0x2400);
        if code < 0 {
            return self.frequency[0];
        }
        if code as u32 >= 0xa300 {
            return self.frequency[163];
        }
        let index = (code as usize) >> 8;
        let fraction = code as u32 & 255;
        let base = self.frequency[index];
        base.wrapping_add(
            (self.frequency[index + 1].wrapping_sub(base) >> 8).wrapping_mul(fraction),
        )
    }
}
impl ControllerFilter {
    pub fn key_offset(&self, tables: &ControllerFilterTables) -> i16 {
        let depth = ((self.key_tracking & 127) as i32 - 64
            + self.key_manual_offset as i32
            + self.key_modulation as i32)
            .clamp(-63, 63);
        ((tables.key_depth[(depth + 64) as usize] as i32 * self.relative_pitch as i32) >> 12) as i16
    }
    pub fn code(&self, tables: &ControllerFilterTables, amplitude: &AmplifierTables) -> i32 {
        let depth = ((self.eg1_intensity & 127) as i32 - 64
            + self.eg1_manual_offset as i32
            + self.eg1_depth_modulation as i32)
            .clamp(-63, 63);
        let envelope =
            amplitude.envelope_level(self.eg1_level, self.velocity, self.eg1_velocity_sensitivity)
                as i16 as i32;
        ((self.cutoff as i8 as i32) << 8)
            + self.cutoff_offset as i32
            + self.key_offset(tables) as i32
            + (self.lfo_offset as i32 >> 7)
            + ((depth * envelope) >> 5)
            + self.additional_offset as i32
            + self.cutoff_modulation as i32 * 2
    }
    pub fn frequency(&self, tables: &ControllerFilterTables, amplitude: &AmplifierTables) -> u32 {
        tables.frequency(self.code(tables, amplitude))
    }
}
