//! Six Virtual Patch routes: original SH3 022300 and 022390..022610.
pub const MODULATION_DESTINATIONS: usize =
    if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
        43
    } else {
        40
    };
pub const VIRTUAL_PATCHES: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerSources {
    pub envelope_levels: [u16; 3],
    pub envelope_velocity_sensitivity: [u8; 3],
    pub lfo: [i16; 2],
    pub velocity: u8,
    pub bend: i16,
    pub wheel: u8,
    pub relative_pitch: i16,
    pub auxiliary: i16,
}
impl ControllerSources {
    pub fn normalized(&self, tables: &crate::amplifier_control::AmplifierTables) -> [i32; 10] {
        let mut result = [0; 10];
        for (i, value) in result[..3].iter_mut().enumerate() {
            *value = tables.envelope_level(
                self.envelope_levels[i],
                self.velocity,
                self.envelope_velocity_sensitivity[i],
            ) as u16 as i32;
        }
        result[3] = (self.lfo[0] as i32) >> 1;
        result[4] = (self.lfo[1] as i32) >> 1;
        result[5] = ((self.velocity & 127) as i32) << 7;
        result[6] = (self.bend as i32) << 1;
        result[7] = (self.wheel & 127) as i32 * 129;
        result[8] = key_source(self.relative_pitch).value;
        result[9] = (self.auxiliary as i32) >> 1;
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModulationDestination(u8);
impl ModulationDestination {
    pub fn new(raw: u8) -> Option<Self> {
        ((raw as usize) < MODULATION_DESTINATIONS).then_some(Self(raw))
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModulationSource {
    pub selector: u8,
    /// Original controller-normalized signal; not an audio sample.
    pub value: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualPatch {
    pub source: ModulationSource,
    pub destination: ModulationDestination,
    pub intensity: u8,
    pub manual_offset: i8,
    pub dynamic_offset: i16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModulationContribution {
    pub amount: i32,
    pub linked_pitch: i32,
}
pub struct ModulationTables {
    pub pitch_depth: [i16; 128],
    pub lfo_rate_depth: [i16; 128],
    pub key_linear_depth: [i16; 128],
    pub key_cutoff_depth: [i16; 128],
    pub key_lfo_rate_depth: [i16; 128],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModulationTargets {
    pub values: [i32; MODULATION_DESTINATIONS],
    pub linked_pitch: i32,
}

/// Controller storage formats after 0219a4..0222fc. Compiler/host dispatch
/// side effects are separate from this fixed-point application boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedModulationTargets {
    pub oscillator_pitch_q16: [i32; 2],
    pub controls: [i16; MODULATION_DESTINATIONS - 2],
    pub linked_oscillator_pitch: i16,
}
impl Default for AppliedModulationTargets {
    fn default() -> Self {
        Self {
            oscillator_pitch_q16: [0; 2],
            controls: [0; MODULATION_DESTINATIONS - 2],
            linked_oscillator_pitch: 0,
        }
    }
}
impl ModulationTargets {
    pub fn applied(&self) -> AppliedModulationTargets {
        AppliedModulationTargets {
            oscillator_pitch_q16: [
                self.values[0].wrapping_shl(8),
                self.values[1].wrapping_shl(8),
            ],
            controls: core::array::from_fn(|n| {
                let destination = n + 2;
                let shift = match destination {
                    8 | 15 | 16 | 19 | 22..=33 => 7,
                    17 | 18 | 20 | 21 | 34..=41 => 8,
                    _ => 0,
                };
                (self.values[destination].clamp(-32767, 32767) >> shift) as i16
            }),
            linked_oscillator_pitch: self.linked_pitch.clamp(-32767, 32767) as i16,
        }
    }
}
impl Default for ModulationTargets {
    fn default() -> Self {
        Self {
            values: [0; MODULATION_DESTINATIONS],
            linked_pitch: 0,
        }
    }
}
impl VirtualPatch {
    pub fn depth(self) -> i8 {
        (((self.intensity & 127) as i32 - 64)
            + self.manual_offset as i32
            + self.dynamic_offset as i32)
            .clamp(-63, 63) as i8
    }
}
impl ModulationTables {
    pub fn pitch(&self, source: i32, depth: i8) -> i32 {
        let coefficient = self.pitch_depth[(depth as i16 + 64) as usize] as i64;
        // 022390 consumes MACL before its signed quotient wrapper.
        ((coefficient * source as i64) as i32) / 16383
    }
    pub fn scale(
        &self,
        source: ModulationSource,
        destination: ModulationDestination,
        depth: i8,
    ) -> ModulationContribution {
        debug_assert!((-63..=63).contains(&depth));
        let d = destination.index();
        let key = source.selector & 15 == 8;
        let index = (depth as i16 + 64) as usize;
        let (amount, linked_pitch) = match d {
            0 | 1 => (self.pitch(source.value, depth), 0),
            7 | 9 if key => (
                ((self.key_cutoff_depth[index] as i64 * source.value as i64) >> 16) as i32,
                0,
            ),
            7 | 9 => (
                (((depth as i32 * depth.abs() as i32) as i64 * source.value as i64) >> 12) as i32,
                0,
            ),
            13 | 14 | 42 if key => (
                ((self.key_lfo_rate_depth[index] as i64 * source.value as i64) >> 15) as i32,
                0,
            ),
            13 | 14 | 42 => (
                ((self.lfo_rate_depth[index] as i64 * source.value as i64) >> 15) as i32,
                0,
            ),
            _ => {
                let amount = if key {
                    ((self.key_linear_depth[index] as i64 * source.value as i64) >> 14) as i32
                } else {
                    ((depth as i64 * source.value as i64) >> 6) as i32
                };
                (
                    amount,
                    if d == 2 {
                        self.pitch(source.value, depth)
                    } else {
                        0
                    },
                )
            }
        };
        ModulationContribution {
            amount,
            linked_pitch,
        }
    }
    pub fn route(&self, patches: &[VirtualPatch; VIRTUAL_PATCHES]) -> ModulationTargets {
        self.route_all(patches)
    }
    /// Browser extensions use the same scaling and accumulator arithmetic.
    pub fn route_all(&self, patches: &[VirtualPatch]) -> ModulationTargets {
        let mut targets = ModulationTargets::default();
        for &patch in patches {
            let depth = patch.depth();
            if depth == 0 || patch.source.value == 0 {
                continue;
            }
            let routed = self.scale(patch.source, patch.destination, depth);
            let value = &mut targets.values[patch.destination.index()];
            *value = value.wrapping_add(routed.amount);
            targets.linked_pitch = targets.linked_pitch.wrapping_add(routed.linked_pitch);
        }
        targets
    }
}

pub fn lfo_source(selector: u8, value: i16) -> ModulationSource {
    ModulationSource {
        selector,
        value: (value as i32) >> 1,
    }
}
pub fn velocity_source(velocity: u8) -> ModulationSource {
    ModulationSource {
        selector: 5,
        value: ((velocity & 127) as i32) << 7,
    }
}
pub fn key_source(relative_pitch: i16) -> ModulationSource {
    ModulationSource {
        selector: 8,
        value: ((relative_pitch as i32 * 32767) / 3072) >> 1,
    }
}
