//! TMU0 and the voice-service decision from SYS02e464/016118.
//! Clocks and external memory are supplied by adapters.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerServiceTimer {
    pub constant: u32,
    pub counter: u32,
    /// CPU clocks modulo the selected TMU0 prescaler.
    pub prescaler_phase: u8,
}
impl ControllerServiceTimer {
    /// TMU0 prescale0 counts one tick per24 CPU clocks in the current board
    /// reference. Its inclusive countdown makes TCOR3000 a72024-clock period.
    pub fn advance_cpu_clocks(&mut self, clocks: u32) -> u32 {
        let total = u64::from(clocks) + u64::from(self.prescaler_phase);
        let ticks = total / 24;
        self.prescaler_phase = (total % 24) as u8;
        let first = u64::from(self.counter) + 1;
        if ticks < first {
            self.counter -= ticks as u32;
            return 0;
        }
        let rest = ticks - first;
        let period = u64::from(self.constant) + 1;
        self.counter = self.constant - (rest % period) as u32;
        (1 + rest / period) as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceService {
    None,
    Envelopes,
    Release,
}

/// One physical slot of the whole016118 traversal. This is distinct from
/// note-allocation flags. Bit40 skips the first envelope service after binding;
/// bit04 requests release, and bit10 marks a normal envelope update for upload.
pub fn select_voice_service(flags: &mut u8, inhibited: u8) -> VoiceService {
    if inhibited != 0 || *flags & 0x81 == 0 {
        VoiceService::None
    } else if *flags & 0x40 != 0 {
        *flags &= !0x40;
        VoiceService::None
    } else if *flags & 4 != 0 {
        *flags &= !4;
        VoiceService::Release
    } else {
        *flags |= 0x10;
        VoiceService::Envelopes
    }
}
