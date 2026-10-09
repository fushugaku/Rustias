//! Native host/frame scheduling from the normal DSP control flow.
//! Jobs execute synthesis/data use cases; this state machine decodes no code.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspTask {
    Mailbox,
    PublishOutputBuffer,
    LoadInputBuffer,
    AdvanceOutputRing,
    ResetSynthesisWorkspace,
    SynthesizeFrame(u8),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchEffect {
    pub task: Option<DspTask>,
    pub frame_flags_write: Option<u16>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchStage {
    Main(u8),
    FrameSetup(u8),
    FramePoll { frame: u8, phase: u8 },
    FrameReturn,
    Working(DspTask),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DspDispatch {
    pub stage: DispatchStage,
    pub interrupts_masked: bool,
    host_latch: u16,
    frame_latch: u16,
    return_stage: DispatchStage,
}
impl Default for DspDispatch {
    fn default() -> Self {
        Self {
            stage: DispatchStage::Main(0),
            interrupts_masked: false,
            host_latch: 4,
            frame_latch: 0,
            return_stage: DispatchStage::Main(0),
        }
    }
}
impl DspDispatch {
    /// Skip complete idle polls when inputs stay fixed until the caller's next
    /// event deadline. Keep latched values and the partial final poll exact.
    pub fn skip_idle_clocks(&mut self, hpic: u16, frame_flags: u16, available: u64) -> u64 {
        if self.stage != DispatchStage::Main(0)
            || self.interrupts_masked
            || hpic & 4 == 0
            || frame_flags & 7 == 7
        {
            return 0;
        }
        let clocks = available / 12 * 12;
        if clocks != 0 {
            self.host_latch = hpic & 4;
            self.frame_latch = frame_flags & 7;
        }
        clocks
    }
    fn start(&mut self, task: DspTask, return_stage: DispatchStage) -> DispatchEffect {
        self.stage = DispatchStage::Working(task);
        self.return_stage = return_stage;
        DispatchEffect {
            task: Some(task),
            ..Default::default()
        }
    }
    /// One functional DSP control clock. Port values are sampled only at their
    /// original read point; later arrivals cannot replace an earlier latch.
    pub fn tick(&mut self, hpic: u16, frame_flags: &mut u16) -> DispatchEffect {
        let mut effect = DispatchEffect::default();
        match self.stage {
            DispatchStage::Main(phase) => {
                match phase {
                    1 => self.host_latch = hpic,
                    2 => self.host_latch &= 4,
                    3 if self.host_latch == 0 => {
                        return self.start(DspTask::Mailbox, DispatchStage::Main(4));
                    }
                    4 => self.interrupts_masked = true,
                    5 => self.frame_latch = *frame_flags,
                    6 => self.frame_latch &= 7,
                    8 if self.frame_latch == 7 => {
                        self.stage = DispatchStage::FrameSetup(0);
                        return effect;
                    }
                    9 => self.interrupts_masked = false,
                    _ => {}
                }
                self.stage = DispatchStage::Main(if phase == 11 { 0 } else { phase + 1 });
            }
            DispatchStage::FrameSetup(phase) => {
                match phase {
                    0 => {
                        *frame_flags = 0;
                        effect.frame_flags_write = Some(0);
                    }
                    1 => self.interrupts_masked = false,
                    2 => {
                        return self
                            .start(DspTask::PublishOutputBuffer, DispatchStage::FrameSetup(3));
                    }
                    3 => return self.start(DspTask::LoadInputBuffer, DispatchStage::FrameSetup(4)),
                    4 => {
                        return self
                            .start(DspTask::AdvanceOutputRing, DispatchStage::FrameSetup(5));
                    }
                    5 => {
                        return self.start(
                            DspTask::ResetSynthesisWorkspace,
                            DispatchStage::FramePoll { frame: 0, phase: 0 },
                        );
                    }
                    _ => unreachable!(),
                }
                self.stage = DispatchStage::FrameSetup(phase + 1);
            }
            DispatchStage::FramePoll { frame, phase } => {
                match phase {
                    1 => self.host_latch = hpic,
                    2 => self.host_latch &= 4,
                    3 if self.host_latch == 0 => {
                        return self.start(
                            DspTask::Mailbox,
                            DispatchStage::FramePoll { frame, phase: 4 },
                        );
                    }
                    5 => {
                        return self.start(
                            DspTask::SynthesizeFrame(frame),
                            if frame == 3 {
                                DispatchStage::FrameReturn
                            } else {
                                DispatchStage::FramePoll {
                                    frame: frame + 1,
                                    phase: 0,
                                }
                            },
                        );
                    }
                    _ => {}
                }
                self.stage = DispatchStage::FramePoll {
                    frame,
                    phase: phase + 1,
                };
            }
            DispatchStage::FrameReturn => self.stage = DispatchStage::Main(9),
            DispatchStage::Working(_) => {}
        }
        effect
    }
    pub fn complete_task(&mut self, task: DspTask) {
        assert_eq!(self.stage, DispatchStage::Working(task));
        self.stage = self.return_stage;
    }
}
