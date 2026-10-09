//! Primary pitch command composition before the shared ordered sender.
use crate::dsp_transport::ParameterSendRequest;
use radias_synth_domain::{
    dsp_control::DspEndpoint, primary_pitch_dispatch::PrimaryPitchSendTable,
};

/// SYS01fca8/01fcf0 use the assigned DSP base and oscillator descriptor.
/// PCM descriptors6/7 require their separate paired pitch/sample compiler.
pub fn primary_pitch_request(
    table: PrimaryPitchSendTable,
    clock: u64,
    endpoint: DspEndpoint,
    voice_address: u16,
    selection: u8,
    code: u16,
) -> Option<ParameterSendRequest> {
    Some(ParameterSendRequest {
        endpoint,
        sender: table.sender(selection)?,
        address: u32::from(voice_address.wrapping_add(2)),
        value: u32::from(code),
        available_clock: clock,
    })
}
