//! Original SH3 amplifier target compilation, 002202..002396.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmplifierTables {
    pub velocity: [i16; 128],
    pub midi_volume: [u16; 128],
    pub program_volume: [u16; 128],
    pub key_tracking: [i16; 128],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmplifierControl {
    pub level: u8,
    pub level_offset: i8,
    /// Original timbre controller gain before the SH3 doubling operation.
    pub source_gain: u16,
    pub envelope_level: u16,
    pub velocity: u8,
    pub velocity_sensitivity: u8,
    pub modulation: [i16; 2],
    pub midi_volume: Option<u8>,
    /// Legacy name: original per-actor1EA Unison gain-bank index.
    pub program_volume: u8,
}

impl AmplifierTables {
    /// Original SYS0020E2 amplitude key curve, separate from EG2 time tracking.
    pub fn key_modulation(&self, tracking: u8, relative_pitch: i16) -> i16 {
        let depth = self.key_tracking[(tracking & 127) as usize] as i32;
        let value = ((depth * relative_pitch as i32) >> 11).clamp(-32767, 32767);
        let curved = if value > 0 {
            let remaining = 32768 - value;
            32768 - ((remaining * remaining) >> 15)
        } else if value < 0 {
            let remaining = 32768 + value;
            ((remaining * remaining) >> 15) - 32768
        } else {
            0
        };
        (curved >> 1).clamp(-32767, 32767) as i16
    }
    /// Shared EG1/EG2/EG3 velocity level scaling, original 01642c.
    pub fn envelope_level(&self, level: u16, velocity: u8, sensitivity: u8) -> u32 {
        let depth = (sensitivity & 127) as i32 - 64;
        let scaled = if depth == 0 {
            level as u32
        } else {
            let index = if depth < 0 {
                127 - (velocity & 127)
            } else {
                velocity & 127
            };
            let factor =
                (32768 + ((self.velocity[index as usize] as i32 * depth.abs()) >> 6)) as u16;
            (factor as u32 * level as u32) >> 15
        };
        scaled >> 1
    }
    pub fn target(&self, control: AmplifierControl) -> i16 {
        let level =
            ((control.level & 127) as i32 + control.level_offset as i32).clamp(0, 127) as u32;
        let mut value = (level * 258 * control.source_gain.wrapping_shl(1) as u32) >> 16;
        let envelope = self.envelope_level(
            control.envelope_level,
            control.velocity,
            control.velocity_sensitivity,
        );
        value = (value * ((envelope << 1) & 65535)) >> 16;
        value = (value * 0x6439) >> 15;
        let depth = ((control.modulation[0] as i32 + control.modulation[1] as i32) * 2)
            .clamp(-32767, 32767);
        let shaped = ((((depth * value as i32) >> 15) + value as i32) * 2) as i64;
        let mut result = ((shaped * shaped) >> 16).clamp(0, 32767) as u32;
        if let Some(volume) = control.midi_volume {
            result = ((result * self.midi_volume[(volume & 127) as usize] as u32) >> 13).min(32767);
        }
        let gain = self.program_volume[(control.program_volume & 127) as usize] as u32;
        if gain != 0 {
            result = ((result * gain) >> 15).min(32767);
        }
        result as i16
    }
}
