//! Original 009888/006eb8/007948 damper flags and release decisions.
use crate::voice_allocation::VoiceClaim;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SustainProgram {
    pub enabled: bool,
}
impl Default for SustainProgram {
    fn default() -> Self {
        Self { enabled: true }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SustainState {
    /// Original timbre+45. Low nibble records input sources, bit7 pending Mono.
    pub flags: u8,
}
impl SustainState {
    pub fn receive(&mut self, channel: u8, event: u32) {
        if channel as u32 != event & 15 {
            return;
        }
        let source = 1u8 << ((event >> 28) & 7);
        if event & 0x00400000 != 0 {
            self.flags |= source;
        } else {
            self.flags &= !source;
        }
    }
    pub fn may_defer(self, program: SustainProgram, category: u8) -> bool {
        category & 12 == 0 && program.enabled && self.flags & 15 != 0
    }
    /// Returns whether the Mono owner's held/deferred actors should release.
    pub fn mono_note_off(&mut self, program: SustainProgram, category: u8) -> bool {
        if self.may_defer(program, category) {
            self.flags |= 128;
            false
        } else {
            self.flags &= 127;
            true
        }
    }
    pub fn poly_note_off(
        self,
        program: SustainProgram,
        category: u8,
        claim: &mut VoiceClaim,
    ) -> bool {
        claim.note_flags &= 127;
        if self.may_defer(program, category) {
            claim.release_flags = (claim.release_flags & !1) | 2;
            false
        } else {
            claim.release_flags |= 1;
            true
        }
    }
    pub fn release_pending_mono(&mut self) -> bool {
        if self.flags & 15 == 0 && self.flags & 128 != 0 {
            self.flags &= 127;
            true
        } else {
            false
        }
    }
    /// Original009a00 scans all physical slots; the caller's owner is not tested.
    pub fn release_pending_poly(claim: &mut VoiceClaim) -> bool {
        if claim.note_flags & 128 == 0 && claim.release_flags & 2 != 0 {
            claim.release_flags = (claim.release_flags & !2) | 1;
            true
        } else {
            false
        }
    }
    /// Original006738 handles held and deferred actors for the Mono owner.
    pub fn release_mono_claim(claim: &mut VoiceClaim) -> bool {
        if claim.note_flags & 128 != 0 || claim.release_flags & 2 != 0 {
            claim.note_flags &= 127;
            claim.release_flags = (claim.release_flags & !2) | 1;
            true
        } else {
            false
        }
    }
}
