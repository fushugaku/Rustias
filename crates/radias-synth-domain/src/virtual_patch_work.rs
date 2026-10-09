//! Functional SH work derived from complete SYS021832 preparation procedures.
//! This models computation work, not electrical pipeline/interrupt timing.
use crate::{actor_virtual_patch::ActorVirtualPatchPorts, modulation::ModulationTargets};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualPatchWork {
    pub sources: [u16; 16],
    pub routes: [u16; 6],
    pub targets: [u16; 40],
}
impl VirtualPatchWork {
    /// Wrapper clears40 accumulators, invokes six routes, then40 callbacks.
    pub fn total(&self) -> u16 {
        626 + self.routes.iter().sum::<u16>() + self.targets.iter().sum::<u16>()
    }
    pub fn source_clocks(body: &[u8; 104], ports: ActorVirtualPatchPorts) -> [u16; 16] {
        core::array::from_fn(|source| match source {
            0..=2 => match body[0x39 + 8 * source] & 127 {
                64 => 57,
                0..=63 => 82,
                _ => 79,
            },
            3 | 4 | 9 => 21,
            5 => 23,
            6 => 25,
            7 => 36,
            // Unsigned division has32 iterations. Both quotient-bit paths
            // cost the same; both signed wrapper paths cost the same too.
            8 => 374,
            _ => {
                let assignment = ports.assignments[(source - 10).min(4)];
                let group_base = match assignment {
                    3 => Some(72),
                    4 => Some(74),
                    18 => Some(76),
                    116 => Some(78),
                    82 => Some(80),
                    83 => Some(82),
                    _ => None,
                };
                let work = match assignment {
                    0 => 70,
                    1 => 72,
                    _ => group_base.map_or(82, |enabled| {
                        enabled
                            + if ports.midi_receive_flags & 0x10 == 0 {
                                4
                            } else {
                                0
                            }
                    }),
                };
                // The fifth getter falls through into its multiply routine.
                work - if source >= 14 { 3 } else { 0 }
            }
        })
    }
    pub fn scale_clocks(source: u8, destination: u8, depth: i8) -> u16 {
        let key = source & 15 == 8;
        match destination {
            0 | 1 => 414,
            2 if key => 514,
            2 => 496,
            7 | 9 if key => 94,
            7 | 9 => {
                if depth < 0 {
                    90
                } else {
                    91
                }
            }
            13 | 14 => 93,
            _ => {
                if key {
                    90
                } else {
                    72
                }
            }
        }
    }
    pub fn route_clocks(
        depth: i8,
        source: u8,
        source_value: i32,
        getter: u16,
        destination: u8,
    ) -> u16 {
        if depth == 0 {
            return 35;
        }
        let key = source & 15 == 8;
        if source_value == 0 {
            return getter + if key { 49 } else { 50 };
        }
        getter + if key { 66 } else { 67 } + Self::scale_clocks(source, destination, depth)
    }
    pub fn target_clocks(
        controller: &crate::actor_control_state::ActorControlState,
        targets: &ModulationTargets,
    ) -> [u16; 40] {
        let applied = targets.applied();
        core::array::from_fn(|destination| {
            let changed = if destination < 2 {
                let offset = 0xa4 + 4 * destination;
                i32::from_be_bytes(controller.bytes[offset..offset + 4].try_into().unwrap())
                    != applied.oscillator_pitch_q16[destination]
            } else {
                let offset = if destination == 2 {
                    0x116
                } else {
                    0x11a + 2 * (destination - 3)
                };
                controller.word(offset) != applied.controls[destination - 2]
            };
            match destination {
                0 | 1 => 15 + if changed { 2 } else { 0 },
                2 => {
                    45 + if changed { 2 } else { 0 }
                        + if controller.word(0x118) != applied.linked_oscillator_pitch {
                            2
                        } else {
                            0
                        }
                }
                16 => 27 + if changed { 2 } else { 0 },
                8 | 15 | 17..=39 => 26 + if changed { 2 } else { 0 },
                _ => 24 + if changed { 2 } else { 0 },
            }
        })
    }
}
