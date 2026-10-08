//! SYS0083E8/008448 bind selected actors to logical program identities.
//! Identity values belong to the caller; the domain does not dereference them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorProgramBinding {
    pub selection_bit: u32,
    pub owner: u32,
    pub common: u32,
    pub synthesis: u32,
    pub voice_cost: u16,
    pub uses_program_common: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimbreProgramBinding {
    pub owner: u32,
    pub common: u32,
    pub synthesis: u32,
    pub voice_cost: u16,
}
pub fn bind_selected(
    actors: &mut [ActorProgramBinding; 24],
    selected: u32,
    timbre: &mut TimbreProgramBinding,
    alternate_synthesis: Option<u32>,
) {
    for actor in actors {
        if actor.selection_bit & selected == 0 {
            continue;
        }
        actor.owner = timbre.owner;
        actor.common = timbre.common;
        actor.voice_cost = timbre.voice_cost;
        if let Some(synthesis) = alternate_synthesis {
            timbre.synthesis = synthesis;
            actor.uses_program_common = 1;
        } else {
            actor.uses_program_common = 0;
        }
        actor.synthesis = timbre.synthesis;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramCommon {
    pub level: u8,
    pub pan: u8,
}
impl ProgramCommon {
    pub fn level_for(self, application_flag: u8) -> Option<u8> {
        (application_flag != 0).then_some(self.level)
    }
    pub fn pan_for(self, application_flag: u8) -> Option<u8> {
        (application_flag != 0).then_some(self.pan)
    }
}
