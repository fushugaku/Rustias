//! Original007860 oldest queue match and0084a8 physical group identities.
use crate::voice_allocation::{
    AllocationOwner, VOICE_COUNT, VOICE_MASK, VoiceClaim, VoiceMask, VoiceOrder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteGroups {
    pub counter: u16,
    pub ages: [u16; VOICE_COUNT],
    pub tags: [u8; VOICE_COUNT],
}
impl Default for NoteGroups {
    fn default() -> Self {
        Self {
            counter: 0,
            ages: [0; VOICE_COUNT],
            tags: [0; VOICE_COUNT],
        }
    }
}
impl NoteGroups {
    /// Original01EB48 partitions a selected mask by the complete age/tag/note
    /// identity, including the held bit, without an owner or active-flag test.
    pub fn take_selected_group(
        &self,
        remaining: &mut VoiceMask,
        claims: &[VoiceClaim; VOICE_COUNT],
    ) -> VoiceMask {
        *remaining &= VOICE_MASK;
        if *remaining == 0 {
            return 0;
        }
        let first = remaining.trailing_zeros() as usize;
        let mut selected = 0;
        for (slot, claim) in claims.iter().enumerate() {
            if *remaining & (1 << slot) != 0
                && self.ages[slot] == self.ages[first]
                && self.tags[slot] == self.tags[first]
                && claim.note_flags == claims[first].note_flags
            {
                selected |= 1 << slot;
            }
        }
        *remaining &= !selected;
        selected
    }
    /// Original01ECC4 consumes the next retired identity and finds its live
    /// survivors outside this iteration's remaining retired mask. Earlier
    /// consumed identities can participate in a later iteration. The flag branch
    /// intentionally uses that byte in the low identity field, as SH3 does.
    pub fn surviving_group(
        &self,
        remaining: &mut VoiceMask,
        claims: &[VoiceClaim; VOICE_COUNT],
    ) -> VoiceMask {
        *remaining &= VOICE_MASK;
        if *remaining == 0 {
            return 0;
        }
        let excluded = *remaining;
        let first = remaining.trailing_zeros() as usize;
        *remaining &= !(1 << first);
        let identity = ((self.ages[first] as u32) << 16)
            | ((self.tags[first] as u32) << 8)
            | (claims[first].note_flags as u32 & 127);
        let mut survivors = 0;
        for (slot, claim) in claims.iter().enumerate() {
            let low = if claim.release_flags & 3 != 0 {
                claim.release_flags
            } else if claim.note_flags & 128 != 0 {
                claim.note_flags
            } else {
                continue;
            };
            let candidate = ((self.ages[slot] as u32) << 16)
                | ((self.tags[slot] as u32) << 8)
                | (low as u32 & 127);
            if candidate == identity {
                *remaining &= !(1 << slot);
                if excluded & (1 << slot) == 0 {
                    survivors |= 1 << slot;
                }
            }
        }
        survivors
    }
    /// Original006dd0 increments even for a routed note-off or rejected note.
    pub fn dispatch(&mut self) {
        self.counter = self.counter.wrapping_add(1);
    }
    pub fn assign(&mut self, mask: VoiceMask, event: u32) {
        for slot in 0..VOICE_COUNT {
            if mask & (1 << slot) != 0 {
                self.ages[slot] = self.counter;
                self.tags[slot] = (event >> 24) as u8;
            }
        }
    }
    /// Find the first held note/tag in allocation order, then select every
    /// physical member of that same age group with matching owner/note/tag.
    pub fn release_mask(
        &self,
        order: &VoiceOrder,
        claims: &[VoiceClaim; VOICE_COUNT],
        owner: AllocationOwner,
        event: u32,
    ) -> VoiceMask {
        let matches = |slot: usize| {
            claims[slot].owner == owner
                && claims[slot].note_flags & 128 != 0
                && claims[slot].note_flags & 127 == event as u8 & 127
                && self.tags[slot] == (event >> 24) as u8
        };
        let Some(first) = order.0.iter().copied().find(|&slot| matches(slot as usize)) else {
            return 0;
        };
        let age = self.ages[first as usize];
        (0..VOICE_COUNT)
            .filter(|&slot| matches(slot) && self.ages[slot] == age)
            .fold(0, |mask, slot| mask | 1 << slot)
    }
}
