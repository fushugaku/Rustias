//! Original OSC2 Ring/Sync preparation, Master A158..A198.
use crate::{Phase, Sample, pitch::PhaseIncrement};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SecondaryModulation {
    pub ring: bool,
    pub sync: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondaryControl {
    pub previous_primary: Phase,
    pub phase: Phase,
    pub gain: i16,
}

impl SecondaryModulation {
    pub fn prepare(
        self,
        current: Phase,
        previous: Phase,
        secondary: Phase,
        increment: PhaseIncrement,
        primary: Sample,
    ) -> SecondaryControl {
        let mut phase = secondary;
        phase.retreat(increment);
        if self.sync && (current.0 as i32 as i64 - previous.0 as i32 as i64) > 0 {
            phase = Phase(0);
        }
        SecondaryControl {
            previous_primary: current,
            phase,
            gain: if self.ring {
                (primary.0 >> 16) as i16
            } else {
                32767
            },
        }
    }
    pub fn wraps(self, current: Phase, previous: Phase) -> bool {
        self.sync && (current.0 as i32 as i64 - previous.0 as i32 as i64) > 0
    }
}
