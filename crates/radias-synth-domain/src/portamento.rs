//! Original SH3 portamento rate, note initialization and reverse-phase curve.
#[derive(Clone, Copy)]
pub struct PortamentoCurves {
    pub values: [[u16; 256]; 16],
}
impl PortamentoCurves {
    /// Portamento selects all sixteen SH3 curve pointers. Its upper eight
    /// curves are distinct from the eight ADSR curve pointers.
    pub fn evaluate(&self, curve: u8, phase: u32) -> u32 {
        if phase == 0 {
            return 0;
        }
        let position = phase as u16 as u32;
        if curve & 15 == 3 {
            return position;
        }
        let index = (position >> 8) as usize;
        let lower = self.values[(curve & 15) as usize][index] as u32;
        let upper = if index == 255 {
            65536
        } else {
            self.values[(curve & 15) as usize][index + 1] as u32
        };
        let difference = upper.wrapping_sub(lower) as u16 as u32;
        lower.wrapping_add(difference.wrapping_mul(position & 255) >> 8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortamentoProgram {
    pub time: u8,
    pub curve: u8,
    pub switch_required: bool,
}
impl Default for PortamentoProgram {
    fn default() -> Self {
        Self {
            time: 0,
            curve: 3,
            switch_required: false,
        }
    }
}
#[derive(Clone, Copy)]
pub struct PortamentoRates {
    pub values: [u32; 128],
}
impl PortamentoProgram {
    pub fn rate(
        self,
        tables: &PortamentoRates,
        switch: bool,
        modulation: i16,
        manual_offset: i8,
    ) -> u32 {
        if self.switch_required && !switch {
            return tables.values[0];
        }
        let time =
            ((self.time & 127) as i32 + modulation as i32 + manual_offset as i32).clamp(0, 127);
        tables.values[time as usize]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortamentoState {
    pub phase: u32,
    pub rate: u32,
    pub start_q16: i32,
    pub current_q16: i32,
}
impl PortamentoState {
    /// Original 01fb06. Caller supplies the accepted previous voice/timbre
    /// pitches and the firmware's decision to inherit its timbre pitch.
    pub fn begin(
        &mut self,
        rate: u32,
        note: u8,
        previous_voice: i32,
        previous_timbre: Option<i32>,
    ) {
        self.rate = rate;
        if rate == 0 {
            self.phase = 0;
            self.start_q16 = 0;
            self.current_q16 = 0;
        } else {
            self.phase = 0xffffff;
            self.start_q16 = previous_timbre
                .unwrap_or(previous_voice)
                .wrapping_sub((note as i32) << 16);
            self.current_q16 = self.start_q16;
        }
    }
    /// Original 014cc0/014cf8. The curve and signed note interval are
    /// truncated before their low-word multiply, not after a float blend.
    pub fn advance(&mut self, curves: &PortamentoCurves, curve: u8) {
        if self.rate == 0 {
            return;
        }
        let next = self.phase.wrapping_sub(self.rate);
        if (next as i32) < 0 {
            self.phase = 0;
            self.rate = 0;
            self.current_q16 = 0;
        } else {
            self.phase = next;
            let argument = (self.phase ^ 0xffffff) >> 8;
            // The original zero-input curve return leaves R1 at0xffffff.
            // This caller uses R1; complementing its low word yields zero.
            let shaped = if argument == 0 {
                0xffffff
            } else {
                curves.evaluate(curve, argument)
            };
            let inverse = ((shaped ^ 65535) as u16 as i32) >> 1;
            self.current_q16 = (self.start_q16 >> 8).wrapping_mul(inverse) >> 7;
        }
    }
    pub fn assigned_note_q16(self, note: u8) -> i32 {
        ((note as i32) << 16).wrapping_add(self.current_q16)
    }
    pub fn relative_pitch_word(self, note: u8) -> u16 {
        (self.assigned_note_q16(note).wrapping_sub(60 << 16) >> 8) as u16
    }
}
