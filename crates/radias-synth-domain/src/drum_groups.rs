//! SYS007E78 retires an active exclusive group in physical actor order.
use crate::{
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VOICE_COUNT, VoiceClaim, VoiceMask, VoiceOrder},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumVoiceGroups {
    pub groups: [u8; VOICE_COUNT],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumGroupRequest {
    pub owner: AllocationOwner,
    pub event: u32,
    pub group: u8,
}
impl Default for DrumVoiceGroups {
    fn default() -> Self {
        Self {
            groups: [0; VOICE_COUNT],
        }
    }
}
impl DrumVoiceGroups {
    pub fn retire(
        &self,
        request: DrumGroupRequest,
        notes: &NoteGroups,
        claims: &mut [VoiceClaim; VOICE_COUNT],
        order: &mut VoiceOrder,
        declared_costs: &mut [u16; VOICE_COUNT],
    ) -> VoiceMask {
        if request.group == 0 {
            return 0;
        }
        let mut selected = 0;
        for slot in 0..VOICE_COUNT {
            let claim = &mut claims[slot];
            if claim.owner != request.owner
                || notes.tags[slot] != (request.event >> 24) as u8
                || self.groups[slot] != request.group
                || (claim.release_flags & 3 == 0 && claim.note_flags & 128 == 0)
            {
                continue;
            }
            selected |= 1 << slot;
            claim.note_flags &= 127;
            claim.release_flags &= !3;
            order.promote_released(slot as u8, claims);
            declared_costs[slot] = 0;
        }
        selected
    }
}
