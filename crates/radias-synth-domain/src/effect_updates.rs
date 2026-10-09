//! Original nine-slot FX coefficient update state and queue encoding.
//! The commands do not establish ASIC interpolation arithmetic.
pub const UNASSIGNED_TARGET: u32 = 0x2f7;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoefficientSlot {
    pub indices: [u16; 4],
    pub target: u32,
    pub last_value: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectCoefficientAssignments {
    pub slots: [CoefficientSlot; 9],
    pub order: [u8; 9],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoefficientChange {
    pub direct_switch: u16,
    pub standalone: bool,
    pub enabled_argument: u32,
    pub mode: u8,
    pub target: u32,
    pub value: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoefficientQueueWord {
    pub address: u16,
    pub tagged_value: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    Direct,
    Reuse,
    Allocate,
    Evict,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoefficientChangePlan {
    pub entries: [CoefficientQueueWord; 5],
    pub count: u8,
    pub outcome: ChangeOutcome,
}
pub struct PreparedCoefficientChange {
    pub next: EffectCoefficientAssignments,
    pub plan: CoefficientChangePlan,
}
impl EffectCoefficientAssignments {
    pub fn new(indices: [[u16; 4]; 9]) -> Self {
        Self {
            slots: indices.map(|indices| CoefficientSlot {
                indices,
                target: UNASSIGNED_TARGET,
                last_value: 0,
            }),
            order: core::array::from_fn(|i| i as u8),
        }
    }
    pub fn prepare(&self, change: CoefficientChange) -> PreparedCoefficientChange {
        let mut next = *self;
        let mut plan = CoefficientChangePlan {
            entries: [CoefficientQueueWord::default(); 5],
            count: 1,
            outcome: ChangeOutcome::Direct,
        };
        if !change.standalone && (change.direct_switch != 0 || change.enabled_argument == 0) {
            plan.entries[0] = CoefficientQueueWord {
                address: change.target as u16,
                tagged_value: change.value & 0xffffff,
            };
            return PreparedCoefficientChange { next, plan };
        }
        let position = self
            .order
            .iter()
            .position(|&slot| self.slots[slot as usize].target == change.target);
        let selected = position.unwrap_or(8);
        let slot = self.order[selected] as usize;
        let prior = self.slots[slot];
        next.order[1..=selected].copy_from_slice(&self.order[..selected]);
        next.order[0] = slot as u8;
        let (first, second) = if change.mode == 0 {
            (0x7a9765, 0x5689a)
        } else {
            (0x7f8644, 0x79bb)
        };
        for (i, value) in [change.target, change.value, first, second]
            .into_iter()
            .enumerate()
        {
            let tag = if change.direct_switch == 0 {
                u32::from(0x84 - i as u8) << 24
            } else {
                0
            };
            plan.entries[i] = CoefficientQueueWord {
                address: prior.indices[i],
                tagged_value: (value & 0xffffff) | tag,
            };
        }
        plan.count = 4;
        plan.outcome = if position.is_some() {
            ChangeOutcome::Reuse
        } else {
            ChangeOutcome::Allocate
        };
        if position.is_none() && prior.target != UNASSIGNED_TARGET {
            plan.entries[4] = CoefficientQueueWord {
                address: prior.target as u16,
                tagged_value: prior.last_value & 0xffffff,
            };
            plan.count = 5;
            plan.outcome = ChangeOutcome::Evict;
        }
        next.slots[slot].target = change.target;
        next.slots[slot].last_value = change.value;
        PreparedCoefficientChange { next, plan }
    }
}
