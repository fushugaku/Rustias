//! Original EG1/EG3 lifecycle. Their callbacks do not reclaim amplifier voices.
use crate::{
    amp_envelope::{AmpEnvelope, AmpEnvelopeParameters, EnvelopeStage},
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
};

pub type ModEnvelopeParameters = AmpEnvelopeParameters;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModEnvelope {
    pub envelope: AmpEnvelope,
}
impl ModEnvelope {
    pub fn note_on(
        &mut self,
        p: ModEnvelopeParameters,
        curves: &EnvelopeCurves,
        timing: &EnvelopeTimingTables,
        phase: u32,
    ) {
        self.envelope.note_on(p, curves, timing, phase);
    }
    pub fn publish(&mut self) {
        self.envelope.publish();
    }
    pub fn release(&mut self, p: ModEnvelopeParameters, timing: &EnvelopeTimingTables) {
        self.envelope.increment_flags = 0;
        self.envelope.release(p, timing);
    }
    pub fn edit(
        &mut self,
        previous: ModEnvelopeParameters,
        next: ModEnvelopeParameters,
        timing: &EnvelopeTimingTables,
    ) {
        self.envelope.edit(previous, next, timing);
    }
    pub fn tick(
        &mut self,
        p: ModEnvelopeParameters,
        curves: &EnvelopeCurves,
        timing: &EnvelopeTimingTables,
    ) {
        let previous = self.envelope.stage;
        self.envelope.tick(p, curves, timing, false);
        if self.envelope.stage == EnvelopeStage::ReleaseHold {
            // Unlike EG2, zero sustain remains Sustain and release disables
            // the modulation envelope without waiting for DSP acknowledgement.
            self.envelope.stage = if previous == EnvelopeStage::Decay {
                EnvelopeStage::Sustain
            } else {
                EnvelopeStage::Disabled
            };
            self.envelope.release_hold = 0;
        }
    }
}
