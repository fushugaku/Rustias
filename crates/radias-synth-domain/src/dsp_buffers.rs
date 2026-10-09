//! Native four-frame buffer service. Data are sampled at their service clock,
//! including double-word lane order and the slave's serial input overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspRole {
    Master,
    Slave,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspBufferTask {
    PublishOutput,
    LoadInput,
    AdvanceOutputRing,
    MailboxNoop,
    BeginSynthesisBatch,
}
pub trait DspBufferBus {
    fn read_data(&self, address: u16) -> u16;
    fn write_data(&mut self, completed_clock: u32, address: u16, value: u16);
    fn write_io(&mut self, completed_clock: u32, address: u16, value: u16);
    fn host_control(&self) -> u16;
    fn set_host_control(&mut self, completed_clock: u32, value: u16);
}
pub struct DspBufferWork {
    role: DspRole,
    task: DspBufferTask,
    elapsed: u32,
    bank: u16,
    ring_pointer: u16,
    ring_sources: [u16; 2],
}
impl DspBufferWork {
    pub fn new(role: DspRole, task: DspBufferTask) -> Self {
        Self {
            role,
            task,
            elapsed: 0,
            bank: 0,
            ring_pointer: 0,
            ring_sources: [0; 2],
        }
    }
    pub fn elapsed(&self) -> u32 {
        self.elapsed
    }
    pub fn complete(&self) -> bool {
        self.elapsed == self.return_clock()
    }
    fn return_clock(&self) -> u32 {
        match self.task {
            DspBufferTask::PublishOutput => 76 + u32::from(self.bank == 0),
            DspBufferTask::LoadInput => {
                (if self.role == DspRole::Slave { 85 } else { 76 }) + u32::from(self.bank == 0)
            }
            DspBufferTask::AdvanceOutputRing => 17,
            DspBufferTask::MailboxNoop => 20,
            // Each native DSP arithmetic use case starts with fresh locals.
            // This service resets temporary computation, not data RAM.
            DspBufferTask::BeginSynthesisBatch => 19,
        }
    }
    fn copy(bus: &mut impl DspBufferBus, clock: u32, source: u16, target: u16) {
        let high = bus.read_data(source);
        let low = bus.read_data(source ^ 1);
        bus.write_data(clock, target, high);
        bus.write_data(clock, target ^ 1, low);
    }
    pub fn step(&mut self, bus: &mut impl DspBufferBus) -> bool {
        if self.complete() {
            return true;
        }
        let clock = self.elapsed + 1;
        match self.task {
            DspBufferTask::PublishOutput | DspBufferTask::LoadInput => {
                if self.elapsed == 0 {
                    self.bank = bus.read_data(0x442);
                }
                let slave_input =
                    self.task == DspBufferTask::LoadInput && self.role == DspRole::Slave;
                let prefix = (if slave_input { 12 } else { 7 }) + u32::from(self.bank == 0);
                if self.elapsed >= prefix {
                    let block_size = if slave_input { 18 } else { 17 };
                    let frame = (self.elapsed - prefix) / block_size;
                    let phase = (self.elapsed - prefix) % block_size;
                    if frame < 4 {
                        let offset = if self.bank == 0 { 128 } else { 0 };
                        if phase < 16 {
                            let pair = (frame * 16 + phase) as u16 * 2;
                            let buffer = match (self.task, self.role) {
                                (DspBufferTask::PublishOutput, DspRole::Master) => 0x502,
                                (DspBufferTask::PublishOutput, DspRole::Slave) => 0x602,
                                (DspBufferTask::LoadInput, DspRole::Master) => 0x602,
                                (DspBufferTask::LoadInput, DspRole::Slave) => 0x502,
                                _ => unreachable!(),
                            } + offset;
                            let (source, target) = if self.task == DspBufferTask::PublishOutput {
                                (0x463 + pair, buffer + pair)
                            } else {
                                (buffer + pair, 0x463 + pair)
                            };
                            Self::copy(bus, clock, source, target);
                        } else if slave_input {
                            let channel = (phase - 16) as u16;
                            let source = 0x4e2
                                + (if self.bank == 0 { 16 } else { 0 })
                                + frame as u16 * 4
                                + channel * 2;
                            let target = 0x463 + frame as u16 * 32 + channel * 2;
                            Self::copy(bus, clock, source, target);
                        }
                    }
                }
            }
            DspBufferTask::AdvanceOutputRing => match self.elapsed {
                2 => self.ring_pointer = bus.read_data(0x1500),
                3 => self.ring_pointer = self.ring_pointer.wrapping_add(8),
                4 => self.ring_pointer &= 0x1ff8,
                5 => bus.write_io(clock, 0xc86, self.ring_pointer),
                6 => bus.write_data(clock, 0x1500, self.ring_pointer),
                8 => self.ring_sources[0] = bus.read_data(0x1501),
                9 => self.ring_sources[1] = bus.read_data(0x1502),
                10 => bus.write_data(clock, 0x1502, self.ring_sources[0]),
                11 => bus.write_data(clock, 0x1501, self.ring_sources[1]),
                12 => self.ring_sources[1] = self.ring_sources[1].wrapping_shl(1),
                13 => bus.write_io(clock, 0xc84, self.ring_sources[1]),
                15 => bus.write_io(clock, 0xc81, 0xd0c0),
                _ => {}
            },
            DspBufferTask::MailboxNoop => match self.elapsed {
                13 => bus.write_data(clock, 0x101, 0x7fff),
                14 => bus.write_data(clock, 0x100, 0),
                18 => bus.set_host_control(clock, bus.host_control() | 4),
                _ => {}
            },
            DspBufferTask::BeginSynthesisBatch => {}
        }
        self.elapsed += 1;
        self.complete()
    }
}
