//! Original global DSP actor activation mask, SYS01fef4/01ff38.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorLifecycle {
    /// Preserve unrelated upper mask bits, as the source does.
    pub active: u32,
}
impl ActorLifecycle {
    pub fn contains(self, slot: usize) -> bool {
        slot < 24 && self.active & (1 << slot) != 0
    }
    pub fn activate(&mut self, slot: usize) {
        if slot < 24 {
            self.active |= 1 << slot;
        }
    }
    pub fn detach(&mut self, slot: usize) -> bool {
        if !self.contains(slot) {
            return false;
        }
        self.active &= !(1 << slot);
        true
    }
}
