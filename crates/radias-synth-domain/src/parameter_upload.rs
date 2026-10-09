//! Original E3da/E3e8 upload cursor. The destination is a23-bit word address;
//! saved counters retain all32 bits and update independently of packet length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadProgress {
    pub destination: u32,
    pub remaining: u32,
}
impl UploadProgress {
    pub fn word_address(self) -> u32 {
        self.destination & 0x7f_ffff
    }
    pub fn advance(&mut self) {
        self.destination = self.destination.wrapping_add(1);
        self.remaining = self.remaining.wrapping_sub(1);
    }
}

/// The original16-bit repeat register receives count-1. A zero packet count
/// therefore selects65,536 transfers rather than an empty upload.
pub fn transfer_count(packet_count: u16) -> u32 {
    u32::from(packet_count.wrapping_sub(1)) + 1
}
