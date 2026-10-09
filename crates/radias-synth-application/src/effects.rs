//! Effect program/coefficients are fully prepared before touching an FX backend.
use radias_synth_domain::effect_control::{EffectMix, PreparedEffect};
use radias_synth_domain::effect_updates::{
    CoefficientChange, CoefficientChangePlan, EffectCoefficientAssignments,
};
pub trait EffectProgramPort {
    type Error;
    fn upload_program(
        &mut self,
        destination: u16,
        words: &[u64],
        control: u16,
    ) -> Result<(), Self::Error>;
    fn write_coefficient(
        &mut self,
        address: u16,
        value: u32,
        control: u16,
    ) -> Result<(), Self::Error>;
    fn write_coefficient_packet(
        &mut self,
        address: u16,
        values: &[u32],
        control: u16,
    ) -> Result<(), Self::Error> {
        for (i, value) in values.iter().enumerate() {
            self.write_coefficient(address.wrapping_add(i as u16), *value, control)?;
        }
        Ok(())
    }
}
pub trait EffectUpdateQueue {
    type Error;
    /// Accept every entry atomically; rejection leaves the queue unchanged.
    fn enqueue(&mut self, plan: &CoefficientChangePlan) -> Result<(), Self::Error>;
}
pub struct EffectUpdateController {
    pub assignments: EffectCoefficientAssignments,
}
impl EffectUpdateController {
    pub fn change<Q: EffectUpdateQueue>(
        &mut self,
        queue: &mut Q,
        change: CoefficientChange,
    ) -> Result<(), Q::Error> {
        let prepared = self.assignments.prepare(change);
        queue.enqueue(&prepared.plan)?;
        self.assignments = prepared.next;
        Ok(())
    }
}
pub fn dispatch_coefficient_plan<P: EffectProgramPort>(
    port: &mut P,
    plan: &CoefficientChangePlan,
) -> Result<(), P::Error> {
    crate::decimator_effect::dispatch_words(port, &plan.entries[..usize::from(plan.count)])
}
pub fn load_effect<P: EffectProgramPort>(
    port: &mut P,
    program: &PreparedEffect,
) -> Result<(), P::Error> {
    for block in &program.blocks[..usize::from(program.block_count)] {
        let start = usize::from(block.word_start);
        for (i, chunk) in program.words[start..start + usize::from(block.count)]
            .chunks(16)
            .enumerate()
        {
            port.upload_program(block.destination.wrapping_add((16 * i) as u16), chunk, 1)?;
        }
    }
    Ok(())
}
pub fn set_effect_mix<P: EffectProgramPort>(
    port: &mut P,
    origin: u16,
    mix: EffectMix,
) -> Result<(), P::Error> {
    for (offset, value) in mix.host_words().into_iter().enumerate() {
        port.write_coefficient(origin.wrapping_add(offset as u16), value, 1)?;
    }
    Ok(())
}
pub fn set_effect_selector<P: EffectProgramPort>(
    port: &mut P,
    writes: &radias_synth_domain::effect_setters::EffectSelectorWrites,
) -> Result<(), P::Error> {
    for word in &writes.words[..usize::from(writes.count)] {
        port.write_coefficient(word.address, word.tagged_value, 1)?;
    }
    Ok(())
}
