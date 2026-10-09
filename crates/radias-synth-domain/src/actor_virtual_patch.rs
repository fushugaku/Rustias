//! Whole preparation-only Virtual Patch state transition, SYS021832 (r0=1).
//! Live changed-value compiler/host dispatch remains a separate transition.
use crate::{
    actor_control_state::ActorControlState,
    amplifier_control::AmplifierTables,
    modulation::{
        ControllerSources, ModulationDestination, ModulationSource, ModulationTables,
        ModulationTargets, VirtualPatch,
    },
    virtual_patch_work::VirtualPatchWork,
};

/// Raw timbre/global controller ports read by the original source getters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorVirtualPatchPorts {
    pub bend: i16,
    pub wheel: u8,
    pub auxiliary: i16,
    pub midi_receive_flags: u8,
    pub assignments: [u8; 5],
    pub assignable_values: [i8; 5],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorVirtualPatchError {
    InvalidDestination(u8),
}
impl ActorControlState {
    pub fn virtual_patch_sources(
        &self,
        body: &[u8; 104],
        ports: ActorVirtualPatchPorts,
        amplifier: &AmplifierTables,
    ) -> [i32; 16] {
        let input = ControllerSources {
            envelope_levels: core::array::from_fn(|i| self.word(0x48 + 24 * i) as u16),
            envelope_velocity_sensitivity: core::array::from_fn(|i| body[0x39 + 8 * i] & 127),
            lfo: [self.word(0xe0), self.word(0xe2)],
            velocity: self.bytes[0x37],
            bend: ports.bend,
            wheel: ports.wheel,
            relative_pitch: self.word(0xea),
            auxiliary: ports.auxiliary,
        };
        let mut values = [0; 16];
        values[..10].copy_from_slice(&input.normalized(amplifier));
        for (index, value) in values[10..15].iter_mut().enumerate() {
            let mask = match ports.assignments[index] {
                0 => 0x80,
                1 => 2,
                3 | 4 | 18 | 116 | 82 | 83 => 0x10,
                _ => 0x40,
            };
            if ports.midi_receive_flags & mask != 0 {
                // Original signed multiply by 258, then arithmetic shift by 1.
                *value = i32::from(ports.assignable_values[index]) * 129;
            }
        }
        values[15] = values[14];
        values
    }

    /// Read all six previous feedback depths before replacing any destination.
    /// Invalid routes leave the controller image unchanged.
    pub fn prepare_virtual_patches(
        &mut self,
        body: &[u8; 104],
        ports: ActorVirtualPatchPorts,
        amplifier: &AmplifierTables,
        tables: &ModulationTables,
    ) -> Result<ModulationTargets, ActorVirtualPatchError> {
        self.prepare_virtual_patches_with_work(body, ports, amplifier, tables)
            .map(|(targets, _)| targets)
    }

    pub fn prepare_virtual_patches_with_work(
        &mut self,
        body: &[u8; 104],
        ports: ActorVirtualPatchPorts,
        amplifier: &AmplifierTables,
        tables: &ModulationTables,
    ) -> Result<(ModulationTargets, VirtualPatchWork), ActorVirtualPatchError> {
        let (targets, work) = self.calculate_virtual_patches(body, ports, amplifier, tables)?;
        self.apply_virtual_patch_targets(&targets);
        Ok((targets, work))
    }

    /// Route calculation reads prior feedback before replacing any targets.
    pub fn calculate_virtual_patches(
        &self,
        body: &[u8; 104],
        ports: ActorVirtualPatchPorts,
        amplifier: &AmplifierTables,
        tables: &ModulationTables,
    ) -> Result<(ModulationTargets, VirtualPatchWork), ActorVirtualPatchError> {
        let sources = self.virtual_patch_sources(body, ports, amplifier);
        let mut work = VirtualPatchWork {
            sources: VirtualPatchWork::source_clocks(body, ports),
            routes: [0; 6],
            targets: [0; 40],
        };
        let mut targets = ModulationTargets::default();
        for route in 0..6 {
            let base = 0x56 + 3 * route;
            let selector = body[base] & 15;
            let depth = (((body[base + 2] & 127) as i32 - 64)
                + i32::from(self.bytes[0x1c4 + 2 * route] as i8)
                + i32::from(self.word(0x158 + 2 * route)))
            .clamp(-63, 63) as i8;
            work.routes[route] = VirtualPatchWork::route_clocks(
                depth,
                selector,
                sources[usize::from(selector)],
                work.sources[usize::from(selector)],
                body[base + 1] & 63,
            );
            if depth == 0 || sources[usize::from(selector)] == 0 {
                continue;
            }
            let raw_destination = body[base + 1] & 63;
            let destination = ModulationDestination::new(raw_destination)
                .ok_or(ActorVirtualPatchError::InvalidDestination(raw_destination))?;
            let patch = VirtualPatch {
                source: ModulationSource {
                    selector,
                    value: sources[usize::from(selector)],
                },
                destination,
                intensity: body[base + 2],
                manual_offset: self.bytes[0x1c4 + 2 * route] as i8,
                dynamic_offset: self.word(0x158 + 2 * route),
            };
            let contribution = tables.scale(patch.source, patch.destination, depth);
            let value = &mut targets.values[destination.index()];
            *value = value.wrapping_add(contribution.amount);
            targets.linked_pitch = targets.linked_pitch.wrapping_add(contribution.linked_pitch);
        }
        work.targets = VirtualPatchWork::target_clocks(self, &targets);
        Ok((targets, work))
    }

    fn apply_virtual_patch_targets(&mut self, targets: &ModulationTargets) {
        let applied = targets.applied();
        self.set_long(0xa4, applied.oscillator_pitch_q16[0]);
        self.set_long(0xa8, applied.oscillator_pitch_q16[1]);
        self.set_word(0x116, applied.controls[0]);
        self.set_word(0x118, applied.linked_oscillator_pitch);
        for (index, value) in applied.controls[1..].iter().enumerate() {
            self.set_word(0x11a + 2 * index, *value);
        }
    }
}
