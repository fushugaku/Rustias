//! Amplifier EG2 controller lifecycle, SH3 01548a, 01477c..0148aa, 015efa.
use crate::envelope_segment::{
    EnvelopeCurves, EnvelopeSegment, EnvelopeTiming, EnvelopeTimingTables,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvelopeStage {
    Attack = 0,
    Decay = 1,
    Sustain = 2,
    Release = 3,
    ReleaseHold = 4,
    Terminating = 5,
    Finished = 6,
    Disabled = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmpEnvelopeParameters {
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub release: u8,
    pub curve: u8,
    pub velocity_sensitivity: u8,
    pub key_tracking: u8,
    pub velocity: u8,
    pub note: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmpEnvelope {
    pub segment: EnvelopeSegment,
    pub stage: EnvelopeStage,
    pub published_level: u16,
    pub target: u16,
    pub increment_flags: u8,
    pub divider: u8,
    pub release_hold: u8,
    pub dirty: bool,
}

impl Default for AmpEnvelope {
    fn default() -> Self {
        Self {
            segment: EnvelopeSegment::default(),
            stage: EnvelopeStage::Disabled,
            published_level: 0,
            target: 0,
            increment_flags: 0,
            divider: 0,
            release_hold: 0,
            dirty: false,
        }
    }
}

impl AmpEnvelope {
    fn timing(parameters: AmpEnvelopeParameters, time: u8) -> EnvelopeTiming {
        EnvelopeTiming {
            curve: parameters.curve,
            time,
            velocity: parameters.velocity,
            velocity_sensitivity: parameters.velocity_sensitivity,
            note: parameters.note,
            key_tracking: parameters.key_tracking,
        }
    }
    fn expanded(value: u8) -> u16 {
        ((value as u16 & 127) << 8) | ((value as u16 & 127) << 1)
    }
    fn evaluate(&mut self, curves: &EnvelopeCurves, curve: u8) {
        let product = curves
            .evaluate(curve, self.segment.phase >> 8)
            .wrapping_mul(self.segment.difference as i32 as u32);
        self.segment.level = self.segment.start.wrapping_add((product >> 16) as u16);
    }
    pub fn note_on(
        &mut self,
        parameters: AmpEnvelopeParameters,
        curves: &EnvelopeCurves,
        tables: &EnvelopeTimingTables,
        initial_phase: u32,
    ) {
        self.stage = EnvelopeStage::Attack;
        self.dirty = true;
        self.increment_flags = 2;
        self.divider = 2;
        self.segment.start = 0;
        self.segment.difference = 32766;
        self.target = 0;
        // Attack uses the original unscaled linear increment table. Decay and
        // release use velocity/key timing and the selected controller curve.
        self.segment.increment = tables.increments[3][(parameters.attack & 127) as usize];
        self.segment.phase = initial_phase
            .wrapping_add(self.segment.increment.wrapping_mul(2))
            .min(0xffffff);
        self.segment.regular_increment = true;
        self.evaluate(curves, 5);
    }
    pub fn release(&mut self, parameters: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        let keep_segment = self.stage == EnvelopeStage::Decay && parameters.sustain & 127 == 0;
        self.stage = EnvelopeStage::Release;
        self.dirty = true;
        let increment = tables.increment(Self::timing(parameters, parameters.release));
        if keep_segment {
            self.segment.increment = increment;
        } else {
            self.target = 0;
            self.segment.begin(0, increment);
        }
    }
    /// Original 014896 publishes the current controller value separately.
    pub fn publish(&mut self) {
        self.published_level = self.segment.level;
    }

    pub fn edit(
        &mut self,
        previous: AmpEnvelopeParameters,
        next: AmpEnvelopeParameters,
        tables: &EnvelopeTimingTables,
    ) {
        if previous.sustain != next.sustain {
            let target = Self::expanded(next.sustain);
            if self.stage == EnvelopeStage::Sustain {
                self.target = target;
                self.segment.level = target;
                self.segment.difference = target as i16;
            } else if self.stage == EnvelopeStage::Decay {
                self.segment.difference = self
                    .segment
                    .difference
                    .wrapping_add(target.wrapping_sub(self.target) as i16);
                self.target = target;
            }
        }
        match self.stage {
            EnvelopeStage::Attack if previous.attack != next.attack => {
                self.segment.increment = tables.increments[3][(next.attack & 127) as usize];
            }
            EnvelopeStage::Decay if previous.decay != next.decay => {
                self.segment.increment = tables.increment(Self::timing(next, next.decay));
            }
            EnvelopeStage::Release if previous.release != next.release => {
                self.segment.increment = tables.increment(Self::timing(next, next.release));
            }
            _ => {}
        }
    }

    fn hold_release(&mut self) {
        self.stage = EnvelopeStage::ReleaseHold;
        self.dirty = true;
        self.segment.level = 0;
        self.release_hold = 2;
    }
    /// One original controller service tick. The application supplies the
    /// voice-release acknowledgement; sample cadence is not inferred here.
    pub fn tick(
        &mut self,
        parameters: AmpEnvelopeParameters,
        curves: &EnvelopeCurves,
        tables: &EnvelopeTimingTables,
        release_acknowledged: bool,
    ) {
        match self.stage {
            EnvelopeStage::Attack | EnvelopeStage::Decay | EnvelopeStage::Release => {
                let stage = self.stage;
                let update = if stage == EnvelopeStage::Attack && self.increment_flags != 0 {
                    self.increment_flags -= 1;
                    self.dirty = true;
                    true
                } else {
                    self.divider = self.divider.wrapping_add(1);
                    self.divider & 3 == 0
                };
                if !update {
                    return;
                }
                self.segment.regular_increment = self.increment_flags != 0;
                let complete = self.segment.advance(
                    curves,
                    if stage == EnvelopeStage::Attack {
                        5
                    } else {
                        parameters.curve
                    },
                );
                self.published_level = self.segment.level;
                if complete {
                    self.increment_flags = 0;
                    match stage {
                        EnvelopeStage::Attack => {
                            self.stage = EnvelopeStage::Decay;
                            self.dirty = true;
                            self.target = Self::expanded(parameters.sustain);
                            self.segment.begin(
                                self.target,
                                tables.increment(Self::timing(parameters, parameters.decay)),
                            );
                        }
                        EnvelopeStage::Decay => {
                            if parameters.sustain & 127 == 0 {
                                self.hold_release();
                            } else {
                                self.stage = EnvelopeStage::Sustain;
                                self.dirty = true;
                            }
                        }
                        EnvelopeStage::Release => self.hold_release(),
                        _ => unreachable!(),
                    }
                }
            }
            EnvelopeStage::ReleaseHold if release_acknowledged => {
                if self.release_hold != 0 {
                    self.release_hold -= 1;
                } else {
                    self.stage = EnvelopeStage::Terminating;
                    self.dirty = true;
                }
                self.segment.level = 0;
            }
            EnvelopeStage::Terminating => {
                self.stage = EnvelopeStage::Finished;
                self.dirty = true;
            }
            _ => {}
        }
    }
}
