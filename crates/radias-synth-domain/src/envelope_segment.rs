//! Controller envelope segments, original SH3 014264/014808/014bfc.
//! The controller clock and ADSR transitions are separate from DSP smoothing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeCurves {
    pub values: [[u16; 256]; 8],
}

/// Original segment increments and controller time scaling tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeTimingTables {
    pub increments: [[u32; 128]; 8],
    pub scale: [u16; 128],
    pub key_tracking: [i16; 128],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeTiming {
    pub curve: u8,
    pub time: u8,
    pub velocity: u8,
    pub velocity_sensitivity: u8,
    pub note: u8,
    pub key_tracking: u8,
}

impl EnvelopeTimingTables {
    /// SH3 013eb4/0144e8/014980, including the two fixed-point truncations.
    pub fn increment(&self, timing: EnvelopeTiming) -> u32 {
        let velocity = (((timing.velocity_sensitivity & 127) as i32 - 64)
            * (timing.velocity as i32 - 64))
            >> 6;
        let key = ((self.key_tracking[(timing.key_tracking & 127) as usize] as i32)
            * (timing.note as i32 - 60))
            >> 14;
        let factor = ((self.scale[(velocity.clamp(-63, 63) + 64) as usize] as u32)
            * (self.scale[(key.clamp(-63, 63) + 64) as usize] as u32))
            >> 8;
        let table = &self.increments[(timing.curve & 7) as usize];
        let product = (table[(timing.time & 127) as usize] as u64 * factor as u64) >> 8;
        if product > u32::MAX as u64 {
            table[0]
        } else {
            (product as u32).clamp(table[127], table[0])
        }
    }
}

impl EnvelopeCurves {
    /// Original 014d8a: table interpolation with a 65536 final endpoint.
    pub fn evaluate(&self, curve: u8, phase: u32) -> u32 {
        if phase == 0 {
            return 0;
        }
        let position = phase as u16 as u32;
        if curve & 7 == 3 {
            return position;
        }
        let index = (position >> 8) as usize;
        let lower = self.values[(curve & 7) as usize][index] as u32;
        let upper = if index == 255 {
            65_536
        } else {
            self.values[(curve & 7) as usize][index + 1] as u32
        };
        lower.wrapping_add(upper.wrapping_sub(lower).wrapping_mul(position & 255) >> 8)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvelopeSegment {
    pub phase: u32,
    pub increment: u32,
    pub start: u16,
    pub difference: i16,
    pub level: u16,
    /// The first segment update runs at four times the stored increment.
    pub regular_increment: bool,
}

impl EnvelopeSegment {
    pub fn begin(&mut self, target: u16, increment: u32) {
        self.start = self.level;
        self.difference = target.wrapping_sub(self.level) as i16;
        self.increment = increment;
        self.phase = 0;
    }

    /// Returns completion before the caller chooses the next ADSR stage.
    pub fn advance(&mut self, curves: &EnvelopeCurves, curve: u8) -> bool {
        let increment = if self.regular_increment {
            self.increment
        } else {
            self.increment.wrapping_shl(2)
        };
        let next = self.phase.wrapping_add(increment);
        let complete = next >= 0x00ff_ffdc;
        self.phase = if complete { 0x00ff_ffff } else { next };
        let shaped = curves.evaluate(curve, self.phase >> 8);
        let product = shaped.wrapping_mul(self.difference as i32 as u32);
        self.level = self.start.wrapping_add((product >> 16) as u16);
        if complete {
            self.regular_increment = false;
        }
        complete
    }
}
