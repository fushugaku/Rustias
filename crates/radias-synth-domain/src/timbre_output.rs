//! Complete SYS03BE40/03BEDC/03BDF4 voice and global-input route publications.
use crate::{
    dsp_control::{DspEndpoint, ParameterPacket},
    program::Program,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimbreOutputActor {
    pub timbre_binding: u32,
    pub primary_selection: u8,
    pub flags: u8,
    pub endpoint: DspEndpoint,
    pub parameter_origin: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimbreOutputCommand {
    pub endpoint: DspEndpoint,
    pub packet: ParameterPacket,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimbreOutputPlan {
    commands: [Option<TimbreOutputCommand>; 26],
    count: usize,
}
impl TimbreOutputPlan {
    pub fn commands(&self) -> impl Iterator<Item = &TimbreOutputCommand> {
        self.commands[..self.count].iter().flatten()
    }
    fn word(&mut self, endpoint: DspEndpoint, target: u32, value: u16) {
        self.commands[self.count] = Some(TimbreOutputCommand {
            endpoint,
            packet: ParameterPacket::word(0, target, u32::from(value)),
        });
        self.count += 1;
    }
}
pub struct TimbreOutputTables {
    /// Immutable SYS04C7C4 owner mapping, indexed by stored selector low nibble.
    pub global_input_owners: [u8; 16],
}
impl TimbreOutputTables {
    pub fn prepare(
        &self,
        actors: &[TimbreOutputActor; 24],
        timbre_bindings: [u32; 4],
        program: &Program,
        timbre: u8,
        alternate: bool,
    ) -> Option<TimbreOutputPlan> {
        let selected_binding = timbre_bindings[usize::from(timbre & 3)];
        let mut plan = TimbreOutputPlan {
            commands: [None; 26],
            count: 0,
        };
        for actor in actors {
            if actor.primary_selection & 15 != 8
                || actor.timbre_binding != selected_binding
                || (!alternate && actor.flags & 0x81 == 0)
            {
                continue;
            }
            plan.word(
                actor.endpoint,
                u32::from(actor.parameter_origin) + 0x1b,
                if alternate { 0 } else { 0x7fff },
            );
        }
        let owner = *self
            .global_input_owners
            .get(usize::from(program.bytes()[0x3ed] & 15))?;
        if *timbre_bindings.get(usize::from(owner))? == selected_binding {
            let flags = program.bytes()[0x3c0];
            for (target, mask) in [(0x3803, 3), (0x38bc, 12)] {
                plan.word(
                    DspEndpoint::Master,
                    target,
                    if alternate && flags & mask != 0 {
                        0
                    } else {
                        0x7fff
                    },
                );
            }
        }
        Some(plan)
    }
}
