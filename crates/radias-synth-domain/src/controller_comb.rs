//! Original SYS01BFFC delay and01C87C feedback table interpolation.
use crate::amplifier_control::AmplifierTables;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombCutoffControl {
    pub link: bool,
    pub cutoff: u8,
    pub linked_cutoff: u8,
    pub manual_offset: i16,
    pub key_offset: i16,
    pub lfo_offset: i16,
    pub eg1_intensity: u8,
    pub linked_eg1_intensity: u8,
    pub eg1_manual_offset: i8,
    pub eg1_depth_modulation: i16,
    pub eg1_level: u16,
    pub velocity: u8,
    pub eg1_velocity_sensitivity: u8,
    pub additional_offset: i16,
    pub cutoff_modulation: i16,
}
impl CombCutoffControl {
    pub fn code(self, amplitude: &AmplifierTables) -> i32 {
        let cutoff = if self.link {
            self.linked_cutoff
        } else {
            self.cutoff
        };
        let intensity = if self.link {
            self.linked_eg1_intensity
        } else {
            self.eg1_intensity
        };
        let depth = ((intensity & 127) as i32 - 64
            + self.eg1_depth_modulation as i32
            + self.eg1_manual_offset as i32)
            .clamp(-63, 63);
        let envelope =
            amplitude.envelope_level(self.eg1_level, self.velocity, self.eg1_velocity_sensitivity)
                as i16 as i32;
        ((cutoff as i8 as i32) << 8)
            + self.manual_offset as i32
            + self.key_offset as i32
            + (self.lfo_offset as i32 >> 7)
            + ((depth * envelope) >> 5)
            + self.additional_offset as i32
            + 2 * self.cutoff_modulation as i32
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombResonanceControl {
    pub link: bool,
    pub resonance: u8,
    pub linked_resonance: u8,
    pub modulation: i16,
    pub manual_offset: i8,
}
impl CombResonanceControl {
    pub fn level(self) -> i32 {
        let level = if self.link {
            self.linked_resonance
        } else {
            self.resonance
        };
        ((level & 127) as i32 + self.modulation as i32 + self.manual_offset as i32).clamp(0, 127)
    }
}
pub struct CombControlTables {
    pub delays: [u32; 128],
    pub feedback: [u32; 128],
}
impl CombControlTables {
    pub fn delay(&self, code: i32) -> u32 {
        lookup(&self.delays, code, true)
    }
    pub fn feedback(&self, code: i32) -> u32 {
        lookup(&self.feedback, code, false)
    }
    /// Complete frequency-dependent SYS01C592 Comb feedback branch.
    pub fn compile_feedback(&self, cutoff_code: i32, resonance: CombResonanceControl) -> u32 {
        let correction = (((cutoff_code.clamp(0, 0x7f00) - 0x7f00) as i64 * 0x78f1) >> 15) as i32;
        let level = resonance.level();
        let lookup_code = ((((level << 8) + correction) >> 1) + 0x4000).clamp(0, 32767);
        let coefficient = self.feedback(lookup_code);
        let gain = (level << 11).clamp(0, 32767) as u64;
        ((coefficient as u64 * gain) >> 15) as u32
    }
}
fn lookup(table: &[u32; 128], code: i32, descending: bool) -> u32 {
    if code < 0 {
        return table[0];
    }
    if code >= 0x7f00 {
        return table[127];
    }
    let index = (code as u32 >> 8) as usize;
    let fraction = code as u32 & 255;
    let first = table[index];
    if fraction == 0 {
        return first;
    }
    if descending {
        first.wrapping_sub((first.wrapping_sub(table[index + 1]) >> 8).wrapping_mul(fraction))
    } else {
        first.wrapping_add((table[index + 1].wrapping_sub(first) >> 8).wrapping_mul(fraction))
    }
}
