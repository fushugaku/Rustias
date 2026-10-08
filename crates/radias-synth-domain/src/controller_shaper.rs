//! Original SYS002DE4 controller clamp and WS depth transfers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WaveshaperType {
    Decimator = 0,
    HardClip = 1,
    OctSaw = 2,
    MultiTriangle = 3,
    MultiSine = 4,
    SubSaw = 5,
    SubSquare = 6,
    SubTriangle = 7,
    SubSine = 8,
    Pickup = 9,
    LevelBoost = 10,
}
impl WaveshaperType {
    pub fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Decimator,
            1 => Self::HardClip,
            2 => Self::OctSaw,
            3 => Self::MultiTriangle,
            4 => Self::MultiSine,
            5 => Self::SubSaw,
            6 => Self::SubSquare,
            7 => Self::SubTriangle,
            8 => Self::SubSine,
            9 => Self::Pickup,
            10 => Self::LevelBoost,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShaperControl {
    pub depth: u8,
    pub manual_offset: i16,
    pub modulation: i16,
}

impl ShaperControl {
    pub fn drive_depth(self) -> i16 {
        (((self.depth & 127) as i32) * 256 + self.manual_offset as i32 + 2 * self.modulation as i32)
            .clamp(0, 32767) as i16
    }

    pub fn hard_clip_depth(self) -> i16 {
        let depth = self.drive_depth() as u64;
        let squared = depth * depth;
        //45 logical64 shifts precede the separate Q15 normalization product.
        let quartic = (squared * squared) >> 45;
        (512 + ((quartic * 32256) >> 15)) as i16
    }

    pub fn waveshaper_depth(self, kind: WaveshaperType) -> i16 {
        let (base, scale) = match kind {
            WaveshaperType::HardClip => return self.hard_clip_depth(),
            WaveshaperType::MultiTriangle => (2162, 30605),
            WaveshaperType::MultiSine => (1081, 31686),
            WaveshaperType::Pickup => (409, 32358),
            WaveshaperType::LevelBoost => (6553, 26214),
            _ => return self.drive_depth(),
        };
        let code = self.drive_depth() as u64;
        // The original two multiplies truncate separately.
        (base + ((((code * code) >> 15) * scale) >> 15)) as i16
    }
}
