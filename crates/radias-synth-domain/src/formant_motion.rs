//! Original 03acdc Formant Motion playback and recording conversions.
//! The caller owns the ten-control-tick service cadence and DSP publication.
pub const BANDS: usize = 16;
pub const MAX_FRAMES: usize = 750;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionLength;
#[derive(Clone, Copy)]
pub struct MotionRecord<'a> {
    frames: &'a [u8],
}
impl<'a> MotionRecord<'a> {
    pub fn new(frames: &'a [u8]) -> Result<Self, MotionLength> {
        if !frames.len().is_multiple_of(BANDS) || frames.len() / BANDS > MAX_FRAMES {
            return Err(MotionLength);
        }
        Ok(Self { frames })
    }
    pub fn frame_count(self) -> usize {
        self.frames.len() / BANDS
    }
    pub fn bytes(self) -> &'a [u8] {
        self.frames
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormantPlayback {
    pub index: u16,
}
impl Default for FormantPlayback {
    fn default() -> Self {
        Self { index: u16::MAX }
    }
}
impl FormantPlayback {
    /// One native service. Empty records publish the original sixteen zero
    /// words and leave the previous index intact; nonempty records loop.
    pub fn advance(&mut self, record: MotionRecord<'_>) -> [u16; BANDS] {
        let count = record.frame_count();
        if count == 0 {
            return [0; BANDS];
        }
        let next = self.index.wrapping_add(1);
        self.index = if usize::from(next) >= MAX_FRAMES || usize::from(next) >= count {
            0
        } else {
            next
        };
        let at = usize::from(self.index) * BANDS;
        core::array::from_fn(|band| {
            let value = u16::from(record.frames[at + band]);
            value * value / 2
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormantRecording {
    pub count: u32,
    pub active: bool,
}
impl Default for FormantRecording {
    fn default() -> Self {
        Self {
            count: 0,
            active: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingTick {
    Captured { index: u16, frame: [u8; BANDS] },
    LimitReached,
    Inactive,
}
impl FormantRecording {
    /// Native recording stops on the next service after the750th capture.
    /// The lookup is immutable SYS data, not an approximate square root.
    pub fn capture(&mut self, envelopes: [u16; BANDS], quantizer: &[u8; 2048]) -> RecordingTick {
        if !self.active {
            return RecordingTick::Inactive;
        }
        if self.count >= MAX_FRAMES as u32 {
            self.active = false;
            return RecordingTick::LimitReached;
        }
        let index = self.count as u16;
        self.count += 1;
        RecordingTick::Captured {
            index,
            frame: core::array::from_fn(|band| {
                quantizer[usize::from((envelopes[band] & 0x7ff0) >> 4)]
            }),
        }
    }
}
