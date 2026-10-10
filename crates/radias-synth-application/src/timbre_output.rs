use radias_synth_domain::{
    program::Program,
    timbre_output::{TimbreOutputActor, TimbreOutputPlan, TimbreOutputTables},
};
pub trait TimbreOutputPort {
    type Error;
    /// Accept the whole ordered plan atomically, including both DSP endpoints.
    fn accept_timbre_output(&mut self, plan: &TimbreOutputPlan) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum TimbreOutputError<E> {
    InvalidInput,
    Port(E),
}
pub fn restore_timbre_output<P: TimbreOutputPort>(
    port: &mut P,
    tables: &TimbreOutputTables,
    actors: &[TimbreOutputActor; 24],
    bindings: [u32; 4],
    program: &Program,
    timbre: u8,
    alternate: bool,
) -> Result<(), TimbreOutputError<P::Error>> {
    let plan = tables
        .prepare(actors, bindings, program, timbre, alternate)
        .ok_or(TimbreOutputError::InvalidInput)?;
    port.accept_timbre_output(&plan)
        .map_err(TimbreOutputError::Port)
}
