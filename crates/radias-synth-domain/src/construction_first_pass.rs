//! Ordered visible stores in the first SYS01e838 pass, in functional CPU work.
use crate::actor_control_state::ActorControlState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstPassStore {
    ControllerWord {
        slot: usize,
        offset: usize,
        value: u16,
    },
    AllocationFlag {
        slot: usize,
        value: u8,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedFirstPassStore {
    pub clock: u16,
    pub store: FirstPassStore,
}
impl TimedFirstPassStore {
    pub fn apply(self, controllers: &mut [ActorControlState; 24], flags: &mut [u8; 24]) {
        match self.store {
            FirstPassStore::ControllerWord {
                slot,
                offset,
                value,
            } => controllers[slot].set_word(offset, value as i16),
            FirstPassStore::AllocationFlag { slot, value } => flags[slot] = value,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ConstructionFirstPass {
    stores: [TimedFirstPassStore; 216],
    count: usize,
    pub return_clock: u16,
}
impl ConstructionFirstPass {
    pub fn compile(controllers: &[ActorControlState; 24], selected: u32, owner: u32) -> Self {
        let mut plan = Self {
            stores: [TimedFirstPassStore {
                clock: 0,
                store: FirstPassStore::AllocationFlag { slot: 0, value: 0 },
            }; 216],
            count: 0,
            return_clock: 0,
        };
        let mut clock = 15;
        for (slot, controller) in controllers.iter().enumerate() {
            let last = u16::from(slot == 23);
            if selected & (1 << slot) == 0 {
                clock += 13 - last;
                continue;
            }
            // MOV.L is two ordered word bus writes at the same instruction clock.
            plan.word(clock + 7, slot, 0x30, (owner >> 16) as u16);
            plan.word(clock + 7, slot, 0x32, owner as u16);
            plan.push(
                clock + 10,
                FirstPassStore::AllocationFlag { slot, value: 0 },
            );
            for (family, current_clock, shadow_clock) in [(0, 16, 25), (1, 35, 44), (2, 54, 63)] {
                let offset = 0x48 + 24 * family;
                plan.word(clock + current_clock, slot, offset, 0);
                // Equal shadows omit the store, while retaining the13-clock return.
                if controller.word(offset + 4) != 0 {
                    plan.word(clock + shadow_clock, slot, offset + 4, 0);
                }
            }
            clock += 75 - last;
        }
        plan.return_clock = clock;
        plan
    }
    fn push(&mut self, clock: u16, store: FirstPassStore) {
        self.stores[self.count] = TimedFirstPassStore { clock, store };
        self.count += 1;
    }
    fn word(&mut self, clock: u16, slot: usize, offset: usize, value: u16) {
        self.push(
            clock,
            FirstPassStore::ControllerWord {
                slot,
                offset,
                value,
            },
        );
    }
    pub fn stores(&self) -> &[TimedFirstPassStore] {
        &self.stores[..self.count]
    }
}
