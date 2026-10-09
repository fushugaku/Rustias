//! Resume visible constructor stores by completed CPU work, excluding IRQ work.
use radias_synth_domain::{
    actor_control_state::ActorControlState, construction_first_pass::ConstructionFirstPass,
};

pub struct FirstPassExecution {
    plan: ConstructionFirstPass,
    next: usize,
}
impl FirstPassExecution {
    pub fn new(plan: ConstructionFirstPass) -> Self {
        Self { plan, next: 0 }
    }
    /// The scheduler pauses this work clock while servicing an interrupt.
    pub fn advance_until(
        &mut self,
        work_clock: u16,
        controllers: &mut [ActorControlState; 24],
        flags: &mut [u8; 24],
    ) {
        while let Some(store) = self.plan.stores().get(self.next) {
            if store.clock > work_clock {
                break;
            }
            store.apply(controllers, flags);
            self.next += 1;
        }
    }
    pub fn finished(&self, work_clock: u16) -> bool {
        self.next == self.plan.stores().len() && work_clock >= self.plan.return_clock
    }
}
