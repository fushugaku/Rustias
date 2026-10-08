//! Lossless SYS 2.00 program aggregate. Packed source fields remain available.
pub const PROGRAM_BYTES: usize = 1790;
pub const TIMBRE_BYTES: usize = 228;
pub const TIMBRE_COMMON_START: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    raw: [u8; PROGRAM_BYTES],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramLength;

impl Program {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProgramLength> {
        Ok(Self {
            raw: bytes.try_into().map_err(|_| ProgramLength)?,
        })
    }
    pub fn bytes(&self) -> &[u8; PROGRAM_BYTES] {
        &self.raw
    }
    pub fn name(&self) -> &[u8; 12] {
        self.raw[..12].try_into().unwrap()
    }
    pub fn timbre(&self, index: usize) -> Option<Timbre<'_>> {
        if index >= 4 {
            return None;
        }
        let start = TIMBRE_COMMON_START + index * TIMBRE_BYTES;
        Some(Timbre {
            raw: self.raw[start..start + TIMBRE_BYTES].try_into().unwrap(),
        })
    }
    pub fn tempo_tenths(&self) -> u16 {
        u16::from_le_bytes([self.raw[1060], self.raw[1061]])
    }
    pub fn drum_timbre(&self) -> u8 {
        (self.raw[24] >> 5) & 7
    }
    pub fn drum_program(&self) -> crate::drum::DrumProgram {
        crate::drum::DrumProgram::from_raw(self.raw[24], self.raw[25], self.raw[26], self.raw[27])
    }
    pub fn arpeggiator_flags(&self) -> u8 {
        self.raw[1062]
    }
    pub fn vocoder_flags(&self) -> u8 {
        self.raw[960]
    }
}

#[derive(Clone, Copy)]
pub struct Timbre<'a> {
    raw: &'a [u8; TIMBRE_BYTES],
}
impl<'a> Timbre<'a> {
    pub fn from_bytes(raw: &'a [u8; TIMBRE_BYTES]) -> Self {
        Self { raw }
    }
    pub fn bytes(self) -> &'a [u8; TIMBRE_BYTES] {
        self.raw
    }
    pub fn synthesis(self) -> &'a [u8] {
        &self.raw[16..]
    }
    pub fn enabled(self) -> bool {
        self.raw[0] & 128 != 0
    }
    pub fn channel(self, global: u8) -> u8 {
        if self.raw[4] < 16 {
            self.raw[4]
        } else {
            global & 15
        }
    }
    pub fn key_window(self) -> [u8; 2] {
        [self.raw[6], self.raw[7]]
    }
    pub fn accepts(self, note: u8, channel: u8, global: u8) -> bool {
        self.enabled()
            && self.channel(global) == channel
            && note < 128
            && self.raw[6] <= note
            && note <= self.raw[7]
    }
}
