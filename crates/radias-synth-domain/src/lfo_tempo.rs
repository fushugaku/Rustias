//! Original SH3 0171e8/017272 tempo-feedback state and rate selection.
//! Clock event delivery and the global BPM table compiler are separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LfoTempoState {
    pub phase: u32,
    pub previous_increment: u32,
    pub reference_phase: u32,
    pub clock_count: u16,
    pub observed_clock_count: u16,
    pub correction_active: u8,
    pub correction_hold: u8,
    pub division: u8,
}

#[derive(Clone, Copy)]
pub struct LfoTempoTables {
    pub clock_steps: [u16; 64],
    /// Original fallback table; live clock rates are stored per LFO state.
    pub increments: [u32; 64],
    pub tempo_coefficients: [u32; 17],
    pub minimum_increment: u32,
    pub maximum_increment: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TempoSetting(u16);
impl TempoSetting {
    /// Original 0173a6 accepts BPM in tenths and clamps 10.0..300.0.
    pub fn clamped(raw: u32) -> Self {
        Self(raw.clamp(100, 3000) as u16)
    }
    pub fn tenths_bpm(self) -> u16 {
        self.0
    }
    pub fn clock_rate(self) -> u32 {
        self.0 as u32 * 7158
    }
}
impl LfoTempoTables {
    /// 0176b6: signed wrapping index addition, clamp to 0..16, MAC >> 12.
    pub fn compile_increment(&self, division: i32, offset: i32, clock_rate: u32) -> (u8, u32) {
        let index = division.wrapping_add(offset).clamp(0, 16) as usize;
        let product =
            (self.tempo_coefficients[index] as i32 as i64 * clock_rate as i32 as i64) as u64;
        (index as u8, (product >> 12) as u32)
    }
}

impl LfoTempoState {
    /// Original 017d80 pulse, shared by its 65-state clock dispatcher.
    pub fn clock_pulse_four(&mut self) {
        self.reference_phase = self.phase;
        if self.correction_hold != 0 {
            self.correction_active = 1;
        }
        self.correction_hold = 125;
        self.clock_count = self.clock_count.wrapping_add(4);
    }
    /// Original 017e48 pulse; activates correction immediately.
    pub fn clock_pulse_one(&mut self) {
        self.reference_phase = self.phase;
        self.correction_active = 1;
        self.correction_hold = 125;
        self.clock_count = self.clock_count.wrapping_add(1);
    }
    /// Original 017e72 reset retains the old observed counter and compiled rate.
    pub fn reset_clock(&mut self) {
        self.correction_active = 0;
        self.correction_hold = 125;
        self.phase = 0;
        self.reference_phase = 0;
        self.clock_count = 0;
    }
    /// Original 015b42/015bce tempo fields. The compiled rate at +4 belongs to
    /// the caller and is retained; correction flags always inherit the timbre.
    pub fn initialize_note(&mut self, raw_sync: u8, raw_division: u8, shared: Self) {
        self.correction_active = shared.correction_active;
        self.correction_hold = shared.correction_hold;
        if raw_sync & 0x60 == 0x40 {
            self.division = raw_division & 31;
            self.phase = 0;
            self.reference_phase = 0;
            self.clock_count = 0;
            self.observed_clock_count = 0;
        } else {
            self.division = shared.division;
            self.phase = shared.phase;
            self.reference_phase = shared.reference_phase;
            self.clock_count = shared.clock_count;
            self.observed_clock_count = shared.observed_clock_count;
        }
    }
    pub fn phase_correction(&mut self, division: u8, tables: &LfoTempoTables) -> i32 {
        let division = division & 63;
        if self.division != division {
            self.division = division;
            self.clock_count = 0;
            self.phase = 0;
            self.reference_phase = 0;
        }
        if self.observed_clock_count == self.clock_count {
            return 0;
        }
        self.observed_clock_count = self.clock_count;
        if self.clock_count == 0 {
            return 0;
        }
        let mut period = tables.clock_steps[division as usize] as u32;
        if period & 1 == 0 {
            period >>= 1;
        }
        // All 32 original table entries are nonzero. The caller supplies the
        // preserved table rather than an arbitrary division denominator.
        debug_assert!(period != 0);
        let remainder = self.clock_count as u32 % period;
        self.clock_count = remainder as u16;
        self.observed_clock_count = remainder as u16;
        let phase = if remainder == 0 {
            0
        } else {
            (((remainder << 16) / period) as u16 as u32) << 16
        };
        (phase.wrapping_sub(self.reference_phase) as i32) >> 5
    }

    pub fn increment(&mut self, correction: i32, tables: &LfoTempoTables) -> u32 {
        if self.correction_hold != 0 {
            self.correction_hold = self.correction_hold.wrapping_sub(1);
            if self.correction_hold == 0 {
                self.correction_active = 0;
            } else if self.correction_active != 0 {
                if correction >= 0 {
                    let (value, overflow) =
                        (self.previous_increment as i32).overflowing_add(correction);
                    return if overflow || value as u32 > tables.maximum_increment {
                        tables.maximum_increment
                    } else {
                        (value as u32).max(tables.minimum_increment)
                    };
                }
                let magnitude = correction.wrapping_neg() as u32;
                return if self.previous_increment > magnitude {
                    self.previous_increment
                        .wrapping_sub(magnitude)
                        .max(tables.minimum_increment)
                } else {
                    tables.minimum_increment
                };
            }
        }
        tables.increments[(self.division & 63) as usize]
    }
}
