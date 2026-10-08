//! Original channel Expression storage and timbre amplitude receive context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobalPerformance {
    pub channel: u8,
    /// Raw Global byte0F; SYS002202 selects bit20 only for value1, else bit40.
    pub amplitude_receive_mode: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpressionState {
    values: [u8; 16],
}
impl Default for ExpressionState {
    fn default() -> Self {
        Self { values: [127; 16] }
    }
}
impl ExpressionState {
    pub fn set(&mut self, channel: u8, value: u8) {
        self.values[(channel & 15) as usize] = value & 127;
    }
    pub fn value(&self, channel: u8) -> u8 {
        self.values[(channel & 15) as usize]
    }
    pub fn gain(&self, channel: u8, receive_flags: u8, global: GlobalPerformance) -> u16 {
        let mask = if global.amplitude_receive_mode == 1 {
            32
        } else {
            64
        };
        let value = if receive_flags & mask != 0 {
            self.value(channel)
        } else {
            127
        };
        (value as u16) << 8
    }
}
/// SYS027F90 returns a channel mask, including the Global channel.
pub fn expression_channel_mask(channel: u8, timbre_channels: [u8; 4], global: u8) -> u16 {
    let channel = channel & 15;
    if timbre_channels.iter().any(|&c| c & 15 == channel) || global & 15 == channel {
        1 << channel
    } else {
        0
    }
}
