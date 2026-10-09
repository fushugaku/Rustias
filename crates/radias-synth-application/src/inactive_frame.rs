//! Run or pause a native frame whose physical voices are all inactive.
use radias_synth_domain::{
    dsp_buffers::DspBufferBus,
    inactive_frame::{InactiveFrameError, InactiveFrameWork},
};
pub struct InactiveFrameExecution {
    pub work: InactiveFrameWork,
}
impl InactiveFrameExecution {
    pub fn advance_until(
        &mut self,
        clock: u32,
        bus: &mut impl DspBufferBus,
    ) -> Result<(), InactiveFrameError> {
        while self.work.elapsed() < clock && !self.work.complete() {
            self.work.step(bus)?;
        }
        Ok(())
    }
}
