//! Original six-note timbre queue, SH3 0080a0/00819c/008200/0082a0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotePriority {
    #[default]
    Last,
    Lowest,
    Highest,
}
impl NotePriority {
    pub fn from_raw(raw: u8) -> Self {
        match raw & 3 {
            1 => Self::Lowest,
            2 => Self::Highest,
            _ => Self::Last,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceMode {
    pub polyphonic: bool,
    pub multi_trigger: bool,
    pub priority: NotePriority,
}
impl Default for VoiceMode {
    fn default() -> Self {
        Self {
            polyphonic: true,
            multi_trigger: false,
            priority: NotePriority::Last,
        }
    }
}
impl VoiceMode {
    pub fn from_raw(raw: u8) -> Self {
        Self {
            polyphonic: raw & 128 != 0,
            multi_trigger: raw & 64 != 0,
            priority: NotePriority::from_raw(raw),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonoAction {
    Ignore,
    Allocate,
    Legato,
    Retrigger,
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonoDecision {
    pub action: MonoAction,
    pub event: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonoNotes {
    /// Original big-endian note-flags/tag word; first entry is selected.
    pub entries: [u16; 6],
    /// Original queue keeps one selected velocity, not per-note velocities.
    pub velocity: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueOutcome {
    pub event: u32,
    pub previous: u32,
}
fn pack_word(word: u16) -> u32 {
    ((word as u32 & 255) << 24) | (word as u32 >> 8)
}
impl MonoNotes {
    /// Original007040/006e56 action choice. Allocation and controller effects
    /// are separate from this queue/priority decision.
    pub fn note_on(&mut self, mode: VoiceMode, event: u32) -> MonoDecision {
        let result = self.insert(mode.priority, event);
        let action = if (result.event as i8) >= 0 {
            MonoAction::Ignore
        } else if (result.previous as i8) < 0 {
            MonoAction::Allocate
        } else if mode.multi_trigger {
            MonoAction::Retrigger
        } else {
            MonoAction::Legato
        };
        MonoDecision {
            action,
            event: result.event,
        }
    }
    pub fn note_off(&mut self, mode: VoiceMode, event: u32) -> MonoDecision {
        let result = self.remove(event);
        if (result.previous as i8) < 0 {
            MonoDecision {
                action: if mode.multi_trigger {
                    MonoAction::Retrigger
                } else {
                    MonoAction::Legato
                },
                event: result.previous | ((self.velocity as u32 & 127) << 8),
            }
        } else {
            MonoDecision {
                action: if (result.event as i8) >= 0 {
                    MonoAction::Release
                } else {
                    MonoAction::Ignore
                },
                event: result.event,
            }
        }
    }
    pub fn selected(self) -> Option<(u8, u8)> {
        let flags = (self.entries[0] >> 8) as u8;
        (flags & 128 != 0).then_some((flags & 127, self.entries[0] as u8))
    }
    pub fn insert(&mut self, priority: NotePriority, mut event: u32) -> QueueOutcome {
        let mut previous = 0x8000u16;
        let note = event as u8;
        let tag = (event >> 24) as u8;
        if priority == NotePriority::Last {
            previous = self.entries[0] ^ 0x8000;
            self.entries.copy_within(0..5, 1);
            self.entries[0] = u16::from_be_bytes([note, tag]);
            self.velocity = (event >> 8) as u8;
        } else {
            let precedes = |stored: u16| {
                let stored = (stored >> 8) as u8;
                stored & 128 != 0
                    && match priority {
                        NotePriority::Lowest => stored <= note,
                        NotePriority::Highest => note <= stored,
                        NotePriority::Last => unreachable!(),
                    }
            };
            if precedes(self.entries[5]) {
                event &= 0xff00ff7f;
            } else {
                let mut index = 5;
                loop {
                    if precedes(self.entries[index - 1]) {
                        self.entries[index] = u16::from_be_bytes([note, tag]);
                        event &= 0xff00ff7f;
                        break;
                    }
                    let moved = self.entries[index - 1];
                    self.entries[index] = moved;
                    index -= 1;
                    if index == 0 {
                        previous = moved ^ 0x8000;
                        self.entries[0] = u16::from_be_bytes([note, tag]);
                        self.velocity = (event >> 8) as u8;
                        break;
                    }
                }
            }
        }
        QueueOutcome {
            event,
            previous: pack_word(previous),
        }
    }
    pub fn remove(&mut self, mut event: u32) -> QueueOutcome {
        let target = u16::from_be_bytes([event as u8 | 128, (event >> 24) as u8]);
        let mut previous = 0;
        if self.entries[0] == target {
            previous = self.entries[1];
            self.entries.copy_within(1..6, 0);
            self.entries[5] &= 0x7fff;
        } else {
            event |= 128;
            if let Some(index) = self.entries[1..].iter().position(|word| *word == target) {
                let index = index + 1;
                self.entries.copy_within(index + 1..6, index);
                self.entries[5] &= 0x7fff;
            }
        }
        QueueOutcome {
            event,
            previous: pack_word(previous),
        }
    }
}
