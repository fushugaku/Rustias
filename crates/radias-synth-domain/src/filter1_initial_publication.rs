//! Whole SYS01ba64: publish cached frequency with initial attack normalization.
use crate::{
    actor_control_state::ActorControlState, actor_descriptors::DescriptorPlan,
    dsp_control::ParameterPacket,
};
impl ActorControlState {
    pub fn compile_initial_filter1_frequency(&self, body: &[u8; 104]) -> DescriptorPlan {
        // SYS0140a2 omits the separate manual attack offset. SYS013ea6's
        // first table entry is unique; it selects the immediate-attack branch.
        let code = (i32::from(body[52] & 127) + i32::from(self.word(0x140))).clamp(0, 127);
        let mut plan = DescriptorPlan::default();
        plan.send(
            if code == 0 {
                ParameterPacket::INITIAL_FILTER1_IMMEDIATE_SENDER
            } else {
                ParameterPacket::INITIAL_FILTER1_TIMED_SENDER
            },
            0x3c,
            self.long(0xb4) as u32,
            if code == 0 { 79 } else { 78 },
            7,
        );
        plan
    }
}
