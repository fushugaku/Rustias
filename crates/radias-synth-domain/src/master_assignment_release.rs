//! Whole SYS075316 assignment release, including state at suspended producers.
use crate::{
    effect_transition_queue::{
        EffectQueueHostContext, EffectQueuePublicationCursor, EffectTransitionQueue,
    },
    effect_updates::{CoefficientQueueWord, UNASSIGNED_TARGET},
    master_effect_control::MasterControlState,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterAssignmentRelease {
    slot: usize,
    phase: usize,
    direct_switch: u32,
    publication: EffectQueuePublicationCursor,
}
impl MasterAssignmentRelease {
    pub fn new(direct_switch: u32) -> Self {
        Self {
            slot: 0,
            phase: 0,
            direct_switch,
            publication: EffectQueuePublicationCursor::default(),
        }
    }
    pub fn published_words(&self) -> usize {
        self.publication.published_words()
    }
    pub fn host_context(&mut self) -> &mut EffectQueueHostContext {
        self.publication.host_context()
    }
    /// The source clears each occupied record after publishing its restored
    /// target, before the four pool-index writes. Service observes that state.
    pub fn publish_available(
        &mut self,
        state: &mut MasterControlState,
        queue: &mut EffectTransitionQueue,
    ) -> bool {
        while self.slot < state.assignments.slots.len() {
            let record = &mut state.assignments.slots[self.slot];
            if self.phase == 0 && record.target == UNASSIGNED_TARGET {
                self.slot += 1;
                continue;
            }
            let word = if self.phase == 0 {
                CoefficientQueueWord {
                    address: record.target as u16,
                    tagged_value: record.last_value & 0xffffff,
                }
            } else {
                let index = self.phase - 1;
                CoefficientQueueWord {
                    address: record.indices[index],
                    tagged_value: [UNASSIGNED_TARGET, 0, 0x7a9765, 0x5689a][index]
                        | if self.direct_switch == 0 {
                            (0x84 - index as u32) << 24
                        } else {
                            0
                        },
                }
            };
            if !self.publication.publish_word(queue, word) {
                return false;
            }
            if self.phase == 0 {
                record.target = UNASSIGNED_TARGET;
                record.last_value = 0;
            }
            self.phase += 1;
            if self.phase == 5 {
                self.phase = 0;
                self.slot += 1;
            }
        }
        state.update_marker = 0;
        true
    }
}
