//! Resume native DSP tasks between clocked host/frame control checkpoints.
use radias_synth_domain::dsp_dispatch::{DspDispatch, DspTask};

pub trait DspTaskPort {
    fn hpic(&self) -> u16;
    fn frame_flags(&self) -> u16;
    fn set_frame_flags(&mut self, value: u16, completed_clock: u64);
    fn begin_task(&mut self, task: DspTask, clock: u64);
    /// Execute native work at this checkpoint and return true after its last
    /// clock. Completion may depend on inputs sampled during the task.
    /// Hardware interrupt preemption is supplied separately.
    fn task_clock(&mut self, task: DspTask, elapsed: u32, clock: u64) -> bool;
    fn end_task(&mut self, task: DspTask, clock: u64);
}
#[derive(Default)]
pub struct DspDispatchExecution {
    pub control: DspDispatch,
    pub clock: u64,
    task: Option<(DspTask, u32)>,
}
impl DspDispatchExecution {
    /// `until` must be no later than the next external input/IRQ event. The
    /// adapter serializes those events between calls; task-local writes remain
    /// visible at every work clock. Idle polling can then skip whole cycles.
    pub fn advance_until(&mut self, until: u64, port: &mut impl DspTaskPort) {
        while self.clock < until {
            if let Some((task, elapsed)) = self.task {
                let complete = port.task_clock(task, elapsed, self.clock);
                self.clock += 1;
                if complete {
                    port.end_task(task, self.clock);
                    self.control.complete_task(task);
                    self.task = None;
                } else {
                    self.task = Some((task, elapsed + 1));
                }
            } else {
                let idle = self.control.skip_idle_clocks(
                    port.hpic(),
                    port.frame_flags(),
                    until - self.clock,
                );
                if idle != 0 {
                    self.clock += idle;
                    continue;
                }
                let mut flags = port.frame_flags();
                let next = self.control.tick(port.hpic(), &mut flags);
                if let Some(value) = next.frame_flags_write {
                    port.set_frame_flags(value, self.clock + 1);
                }
                self.clock += 1;
                if let Some(task) = next.task {
                    port.begin_task(task, self.clock);
                    self.task = Some((task, 0));
                }
            }
        }
    }
    pub fn task(&self) -> Option<(DspTask, u32)> {
        self.task
    }
}
