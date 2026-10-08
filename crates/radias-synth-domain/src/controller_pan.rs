//! SH3 00250e: program, virtual-patch, timbre and MIDI pan composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanControl {
    pub position: u8,
    pub manual_offset: i16,
    pub modulation: i16,
    pub timbre_offset: i8,
    pub midi_pan: Option<u8>,
}
#[derive(Clone, Copy)]
pub struct PanTables {
    pub targets: [u16; 128],
}
impl PanTables {
    /// SH3 002b18 truncates the composed Q8 controller value before lookup.
    pub fn compile(&self, control: u16) -> u16 {
        self.targets[((control >> 8) & 127) as usize]
    }
}
impl Default for PanControl {
    fn default() -> Self {
        Self {
            position: 64,
            manual_offset: 0,
            modulation: 0,
            timbre_offset: 0,
            midi_pan: None,
        }
    }
}
fn clamp(value: i32) -> u16 {
    value.clamp(0, 32767) as u16
}
fn bend(value: u16, balance: i32) -> u16 {
    let value = value as u32;
    let result = if value < 16384 {
        (value * balance as u16 as u32) >> 6
    } else {
        32767u32.wrapping_sub(((32767 - value) * (127 - balance) as u16 as u32) >> 6)
    };
    clamp(result as i32)
}
impl PanControl {
    pub fn target(self) -> u16 {
        let raw = ((self.position as i8 as i32) << 8)
            + self.manual_offset as i32
            + self.modulation as i32 * 2;
        let value = clamp(raw);
        let balance = self.timbre_offset as i32 + 64;
        let mut target = if value == 16384 {
            (balance << 8) as u16
        } else {
            bend(value, balance)
        };
        if let Some(pan) = self.midi_pan {
            let pan = pan & 127;
            if pan < 64 {
                target = clamp(((target as u32 * pan as u32) >> 6) as i32);
            } else if pan > 64 {
                target =
                    clamp(32767u32.wrapping_sub(
                        (32767u16.wrapping_sub(target) as u32 * (127 - pan) as u32) >> 6,
                    ) as i32);
            }
        }
        target
    }
}
