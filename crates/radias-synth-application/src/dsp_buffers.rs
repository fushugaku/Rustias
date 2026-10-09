//! Resumable native buffer service; an adapter owns RAM and device side effects.
use radias_synth_domain::dsp_buffers::{DspBufferBus, DspBufferWork};
pub struct DspBufferExecution {
    pub work: DspBufferWork,
}
impl DspBufferExecution {
    pub fn advance_until(&mut self, completed_clock: u32, bus: &mut impl DspBufferBus) {
        while self.work.elapsed() < completed_clock && !self.work.complete() {
            self.work.step(bus);
        }
    }
}
