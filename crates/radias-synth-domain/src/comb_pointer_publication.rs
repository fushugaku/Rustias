//! Whole SYS01c8d8 Comb pointer swap, including the two-value SYS00f21c packet.
use crate::{
    actor_control_state::ActorControlState, actor_descriptors::DescriptorPlan,
    dsp_control::ParameterPacket,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCombControllerSlot;
pub struct CompiledCombPointerPublication {
    pub controller: ActorControlState,
    pub publication: DescriptorPlan,
}
impl ActorControlState {
    pub fn compile_comb_pointer_publication(
        &self,
    ) -> Result<CompiledCombPointerPublication, InvalidCombControllerSlot> {
        let mut controller = *self;
        let mut publication = DescriptorPlan::default();
        let flags = self.bytes[0x1e2];
        if flags & 3 == 0 {
            publication.work(19);
        } else if flags & 0x30 != 0x30 {
            publication.work(23);
        } else {
            let slot = self.bytes[0x34];
            if slot >= 24 {
                return Err(InvalidCombControllerSlot);
            }
            controller.bytes[0x1e6] ^= 1;
            publication.send(
                ParameterPacket::COMB_POINTERS_SENDER,
                0x75,
                (u32::from(slot) << 8) | u32::from(controller.bytes[0x1e6] & 1),
                if slot < 12 { 84 } else { 83 },
                7,
            );
        }
        Ok(CompiledCombPointerPublication {
            controller,
            publication,
        })
    }
}
