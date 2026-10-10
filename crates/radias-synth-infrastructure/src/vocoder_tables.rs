//! Immutable original SYS/DSP data, compiled into the adapter as constants.
//! The development extractor is not part of production or the audio thread.
use crate::vocoder_tables_data as data;
use radias_synth_domain::vocoder_control::VocoderControlTables;
pub fn original() -> VocoderControlTables<'static> {
    VocoderControlTables {
        initial: &data::INITIAL,
        release: &data::RELEASE,
        gate: &data::GATE,
        damping: &data::DAMPING,
        envelope_attack: &data::ENVELOPE_ATTACK,
        envelope_release: &data::ENVELOPE_RELEASE,
        frequencies: &data::FREQUENCIES,
        resonance_gain: &data::RESONANCE_GAIN,
        pans: &data::PANS,
        linear: &data::LINEAR,
        bipolar: &data::BIPOLAR,
        depth: &data::DEPTH,
        pitch_depth: &data::PITCH_DEPTH,
    }
}
pub fn interpolation() -> radias_synth_domain::vocoder::InterpolationTables {
    radias_synth_domain::vocoder::InterpolationTables {
        scalar_offsets: core::array::from_fn(|i| data::INTERPOLATION[i]),
        wide_offset: data::INTERPOLATION[38],
    }
}
pub fn formant_quantizer() -> &'static [u8; 2048] {
    &data::FORMANT_QUANTIZER
}
