//! Functional SH work for live destination compilers. Counts follow original
//! branches and fixed-point inputs; no trace times are consumed by this module.
use crate::{
    actor_control_state::ActorControlState,
    envelope_segment::EnvelopeTiming,
    virtual_patch_live::{LiveCompilerPorts, LiveCompilerTables, LiveDestinationUpdate},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LivePublicationWork {
    pub first: u16,
    pub second: u16,
    pub finish: u16,
}
fn one(first: u16, finish: u16) -> LivePublicationWork {
    LivePublicationWork {
        first,
        second: 0,
        finish,
    }
}
fn two(first: u16, second: u16, finish: u16) -> LivePublicationWork {
    LivePublicationWork {
        first,
        second,
        finish,
    }
}
fn only(finish: u16) -> LivePublicationWork {
    one(0, finish)
}
// SYS01BB6E adds0x2400 before its two limit branches and optional interpolation.
pub(crate) fn frequency(code: i32) -> u16 {
    let position = code.wrapping_add(0x2400);
    if position < 0 {
        13
    } else if position >= 0xa300 {
        16
    } else if position & 255 == 0 {
        28
    } else {
        34
    }
}
// SYS01BFFC/01C87C have different lower-limit and interpolation paths.
pub(crate) fn comb_lookup(code: i32, descending: bool) -> u16 {
    if code < 0 {
        11
    } else if code >= 0x7f00 {
        14
    } else if code & 255 == 0 {
        26
    } else if descending {
        33
    } else {
        32
    }
}
pub(crate) fn envelope_level(sensitivity: u8) -> u16 {
    17 + match sensitivity & 127 {
        64 => 17,
        0..=63 => 42,
        _ => 39,
    }
}
pub(crate) fn filter1_code(
    state: &ActorControlState,
    body: &[u8; 104],
    tables: &LiveCompilerTables<'_>,
) -> i32 {
    let depth = (i32::from(body[37] & 127) - 64
        + i32::from(state.word(0x136))
        + i32::from(state.bytes[0x196] as i8))
    .clamp(-63, 63);
    let level =
        tables
            .amplifier
            .envelope_level(state.word(0x48) as u16, state.bytes[0x37], body[57]) as i16
            as i32;
    (i32::from(body[35] as i8) << 8)
        + i32::from(state.word(0x190))
        + i32::from(state.word(0x10c))
        + (i32::from(state.word(0xe6)) >> 7)
        + ((depth * level) >> 5)
        + i32::from(state.word(0x108))
        + 2 * i32::from(state.word(0x122))
}
pub(crate) fn resonance_work(state: &ActorControlState, body: &[u8; 104]) -> u16 {
    let link = state.bytes[0x1e2] & 128 != 0;
    if state.bytes[0x1e2] & 0x30 != 0x30 {
        let alternate = state.bytes[0x1e2] & 0x83 == 0x81;
        return 57 + u16::from(!link) + u16::from(!alternate);
    }
    let code = i32::from_be_bytes(state.bytes[0xc4..0xc8].try_into().unwrap());
    let resonance = crate::controller_comb::CombResonanceControl {
        link,
        resonance: body[41],
        linked_resonance: body[36],
        modulation: state.word(0x13a),
        manual_offset: state.bytes[0x19c] as i8,
    };
    let correction = (((code.clamp(0, 0x7f00) - 0x7f00) as i64 * 0x78f1) >> 15) as i32;
    let lookup = ((((resonance.level() << 8) + correction) >> 1) + 0x4000).clamp(0, 32767);
    // Two complete SYS016494 multiplications have44 functional clocks each.
    168 - if code < 0 { 2 } else { 0 } + u16::from(!link) + 88 + comb_lookup(lookup, false)
}
pub(crate) fn key_work(
    state: &ActorControlState,
    body: &[u8; 104],
    second: bool,
    tables: &LiveCompilerTables<'_>,
) -> u16 {
    let link = state.bytes[0x1e2] & 128 != 0;
    let (parameter, manual, modulation) = if second {
        (if link { 38 } else { 43 }, 0x1a0, 0x13e)
    } else {
        (38, 0x198, 0x138)
    };
    let depth = (i32::from(body[parameter] & 127) - 64
        + i32::from(state.bytes[manual] as i8)
        + i32::from(state.word(modulation)))
    .clamp(-63, 63);
    let multiply = tables.frequency.key_depth[(depth + 64) as usize] != 0;
    let base = if second { 50 + u16::from(!link) } else { 48 };
    base - if multiply { 0 } else { 5 }
}
pub(crate) fn envelope_work(
    state: &ActorControlState,
    body: &[u8; 104],
    destination: u8,
    tables: &LiveCompilerTables<'_>,
) -> u16 {
    let e = usize::from((destination - 22) / 4);
    let parameter = usize::from((destination - 22) % 4);
    let stage = state.bytes[0x94 + e];
    if parameter == 2 {
        return 30
            + match stage {
                1 => 78,
                2 => 82,
                _ => 14,
            };
    }
    if usize::from(stage) != parameter {
        return 40;
    }
    if parameter == 0 {
        return if e == 2 { 103 } else { 104 };
    }
    let time = (i32::from(body[52 + 8 * e + parameter] & 127)
        + i32::from(state.word(0x140 + 8 * e + 2 * parameter))
        + i32::from(state.bytes[0x1a8 + 8 * e + 2 * parameter] as i8))
    .clamp(0, 127) as u8;
    let timing = EnvelopeTiming {
        curve: body[56 + 8 * e],
        time,
        velocity: state.bytes[0x37],
        velocity_sensitivity: body[58 + 8 * e],
        note: state.bytes[0x36],
        key_tracking: body[59 + 8 * e],
    };
    let table = &tables.timing.increments[usize::from(timing.curve & 7)];
    let velocity = ((i32::from(timing.velocity_sensitivity & 127) - 64)
        * (i32::from(timing.velocity as i8) - 64))
        >> 6;
    let key = (i32::from(tables.timing.key_tracking[usize::from(timing.key_tracking & 127)])
        * (i32::from(timing.note as i8) - 60))
        >> 14;
    let factor = (u32::from(tables.timing.scale[(velocity.clamp(-63, 63) + 64) as usize])
        * u32::from(tables.timing.scale[(key.clamp(-63, 63) + 64) as usize]))
        >> 8;
    let product = (u64::from(table[usize::from(time)]) * u64::from(factor)) >> 8;
    let scaling = if product > u64::from(u32::MAX) || product > u64::from(table[0]) {
        34
    } else if product < u64::from(table[127]) {
        38
    } else {
        37
    };
    // Callback, timing wrapper, curve lookup, velocity/key factors, multiply,
    // limit selection and the destination return. EG1 saves an extra register.
    30 + if e == 0 { 71 } else { 69 } + 40 + 13 + 7 + 32 + 33 + 44 + scaling
}

pub(crate) fn destination_work(
    destination: u8,
    update: LiveDestinationUpdate,
    state: &ActorControlState,
    body: &[u8; 104],
    ports: LiveCompilerPorts,
    tables: &LiveCompilerTables<'_>,
) -> LivePublicationWork {
    if update.compiler.is_none() {
        return only(match destination {
            0 | 1 => 15,
            2 => 45,
            16 => {
                if update.changed {
                    32
                } else {
                    27
                }
            }
            8 | 15 | 17..=39 => 26,
            _ => 24,
        });
    }
    match destination {
        1 => {
            let fine = i32::from(body[29] & 127) * 256 + i32::from(state.word(0x188));
            let limit = if fine > 0x7f00 {
                7
            } else if fine < 0 {
                10
            } else {
                9
            };
            let interpolation = if fine.clamp(0, 0x7f00) & 255 == 0 {
                23
            } else {
                34
            };
            one(104 + limit + interpolation, 14)
        }
        3..=5 => {
            let index = usize::from(destination - 3);
            let raw = 2
                * ((i32::from(body[30 + index] as i8) << 8)
                    + i32::from(state.word(0x18a + 2 * index))
                    + 2 * i32::from(state.word(0x11a + 2 * index)));
            let clipping = if raw < 0 {
                25
            } else if raw > 65535 {
                28
            } else {
                27
            };
            one(
                24 + if index == 2 { 40 } else { 41 } + clipping + if index == 0 { 14 } else { 0 },
                16,
            )
        }
        6 => one(71, 11),
        8 => two(105 + u16::from(state.bytes[0x1e2] & 0x83 != 0x81), 8, 16),
        10 => {
            let mode = state.bytes[0x1e3] & 3;
            if mode == 0 {
                only(62)
            } else {
                let work = if mode == 1 {
                    63
                } else {
                    match state.bytes[0x1e4] & 15 {
                        1 => 225,
                        3 | 4 | 9 | 10 => 86,
                        _ => 66,
                    }
                };
                one(24 + work, 11)
            }
        }
        12 => {
            let raw = ((i32::from(body[49] as i8) << 8)
                + i32::from(state.word(0x1a4))
                + 2 * i32::from(state.word(0x12c)))
            .clamp(0, 32767);
            let own = if raw == 16384 {
                47
            } else if raw < 16384 {
                64
            } else {
                70
            };
            let midi = ports.midi_pan.map_or(0, |pan| match pan & 127 {
                64 => 8,
                0..=63 => 25,
                _ => 31,
            });
            one(59 + own + midi, 14)
        }
        15 => only(
            26 + 4
                + if ports.portamento_switch_required {
                    if ports.portamento_switch { 47 } else { 28 }
                } else {
                    42
                },
        ),
        17 => one(
            131 + envelope_level(body[57]) + frequency(filter1_code(state, body, tables)),
            15,
        ),
        18 => only(30 + key_work(state, body, false, tables)),
        19 => {
            let resonance = resonance_work(state, body);
            if state.bytes[0x1e2] & 0x30 == 0x30 {
                one(71 + resonance, 24)
            } else {
                two(71 + resonance, 8, 21)
            }
        }
        20 | 21 => {
            let key = if destination == 21 {
                4 + key_work(state, body, true, tables)
            } else {
                0
            };
            let extra_return = if destination == 21 { 4 } else { 0 };
            let link = state.bytes[0x1e2] & 128 != 0;
            let code = i32::from_be_bytes(state.bytes[0xc4..0xc8].try_into().unwrap());
            let gain = envelope_level(body[57]);
            if state.bytes[0x1e2] & 0x30 == 0x30 {
                two(
                    168 + 2 * u16::from(!link)
                        + key
                        + gain
                        + comb_lookup(code, true)
                        + resonance_work(state, body),
                    44,
                    32 + extra_return,
                )
            } else {
                one(
                    159 + 2 * u16::from(!link) + key + gain + frequency(code),
                    21 + extra_return,
                )
            }
        }
        22..=33 => only(envelope_work(state, body, destination, tables)),
        _ => unreachable!(),
    }
}
