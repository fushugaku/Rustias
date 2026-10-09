//! Composed native voice. Every sample passes through recovered algorithms.
use crate::{
    Phase, Sample,
    envelope::EnvelopeLevel,
    filter::{FilterCoefficients, ResonantFilter},
    fixed::{multiply_q15, saturate},
    mixer::OscillatorMix,
    oscillator::Oscillator,
    pan::{self, StereoFrame},
    primary_oscillator::{PrimaryOscillator, PrimaryParameters},
    secondary_control::SecondaryModulation,
    waveform::WaveformTable,
    waveshaper::{ShaperParameters, ShaperPosition, ShaperSignal, Waveshaper},
};

/// Optional host routing. Native voices keep the recovered fixed graph by default.
#[cfg(feature = "web-modular")]
pub trait SignalProcessor: Send {
    fn process(
        &mut self,
        sources: [Sample; 3],
        parameters: VoiceParameters,
        level: i16,
        table: &WaveformTable,
    ) -> Sample;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceParameters {
    pub primary: PrimaryParameters,
    pub primary_pitch_code: u16,
    pub mix: OscillatorMix,
    pub filter: FilterCoefficients,
    pub routing: Option<crate::filter_routing::DualFilterParameters>,
    pub shaper: Option<ShaperParameters>,
    pub envelope_target: i16,
    pub envelope_rate: i16,
    pub pan_position: i32,
    pub secondary_modulation: SecondaryModulation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voice {
    pub primary: PrimaryOscillator,
    pub secondary: Oscillator,
    pub filter: ResonantFilter,
    pub second_filter: crate::filter_routing::Filter2,
    pub waveshaper: Waveshaper,
    pub envelope: EnvelopeLevel,
    pub previous_secondary: Sample,
    pub previous_primary: Phase,
    pub mixer_noise: crate::noise::MixerNoise,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceFrameInputs {
    pub excitation_bias: i16,
    pub mixer_bias: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceOutput {
    pub mixed: Sample,
    pub filtered: Sample,
    pub level: i16,
    pub amplified: Sample,
    pub stereo: StereoFrame,
}

impl Voice {
    pub fn next_sample(&mut self, table: &WaveformTable, p: VoiceParameters) -> VoiceOutput {
        self.next_sample_on_bus(table, p, StereoFrame::default())
    }

    /// Add in physical voice order, before the DSP's final bus scaling.
    pub fn next_sample_on_bus(
        &mut self,
        table: &WaveformTable,
        p: VoiceParameters,
        existing: StereoFrame,
    ) -> VoiceOutput {
        self.next_sample_on_bus_with_comb(table, p, existing, None)
    }
    pub fn next_sample_on_bus_with_comb(
        &mut self,
        table: &WaveformTable,
        p: VoiceParameters,
        existing: StereoFrame,
        comb: Option<&mut crate::comb::Comb>,
    ) -> VoiceOutput {
        self.next_sample_on_bus_with_inputs(table, p, existing, comb, VoiceFrameInputs::default())
    }
    pub fn next_sample_on_bus_with_inputs(
        &mut self,
        table: &WaveformTable,
        p: VoiceParameters,
        existing: StereoFrame,
        comb: Option<&mut crate::comb::Comb>,
        inputs: VoiceFrameInputs,
    ) -> VoiceOutput {
        self.next_sample_routed(
            table,
            p,
            existing,
            comb,
            inputs,
            #[cfg(feature = "web-modular")]
            None,
        )
    }
    pub fn next_sample_routed(
        &mut self,
        table: &WaveformTable,
        p: VoiceParameters,
        existing: StereoFrame,
        comb: Option<&mut crate::comb::Comb>,
        inputs: VoiceFrameInputs,
        #[cfg(feature = "web-modular")] processor: Option<&mut dyn SignalProcessor>,
    ) -> VoiceOutput {
        let primary = self.primary.next_with_modulator_and_bias(
            table,
            p.primary,
            self.previous_secondary,
            inputs.excitation_bias,
        );
        let current = self.primary.phase;
        if p.secondary_modulation.wraps(current, self.previous_primary) {
            self.secondary.sync_phase(Phase(0));
        }
        self.previous_primary = current;
        self.secondary.set_sync_window(p.secondary_modulation.sync);
        self.secondary.set_gain(if p.secondary_modulation.ring {
            (primary.0 >> 16) as i16
        } else {
            32767
        });
        let secondary = self.secondary.next_with_edge(
            table,
            if p.secondary_modulation.sync {
                Some(current)
            } else {
                None
            },
        );
        self.previous_secondary = secondary;
        let noise = self.mixer_noise.next_word(inputs.excitation_bias);
        #[cfg(feature = "web-modular")]
        if let Some(processor) = processor {
            let level = self.envelope.step(p.envelope_target, p.envelope_rate);
            let amplified = processor.process(
                [primary, secondary, Sample((noise as i32) << 16)],
                p,
                level,
                table,
            );
            return VoiceOutput {
                mixed: primary,
                filtered: amplified,
                level,
                amplified,
                stereo: pan::route(amplified, p.pan_position, existing),
            };
        }
        let mixed = if p
            .routing
            .is_some_and(|r| r.route == crate::filter_routing::FilterRouting::Individual)
        {
            Sample(saturate(multiply_q15(primary.0, p.mix.primary_gain)))
        } else {
            p.mix.sample(primary, secondary, noise, inputs.mixer_bias)
        };
        let mut pre_filter = |input| {
            if let Some(shaper) = p.shaper.filter(|s| s.position == ShaperPosition::PreFilter) {
                self.waveshaper.process(
                    ShaperSignal {
                        input,
                        primary_pitch_code: p.primary_pitch_code,
                        primary_increment: p.primary.base_increment(),
                    },
                    shaper.coefficients,
                    &table.shapers,
                )
            } else {
                input
            }
        };
        let filter_output = if let Some(routing) = p.routing {
            let mut graph = crate::filter_routing::DualFilterGraph {
                first: self.filter,
                second: self.second_filter,
            };
            let output = graph.sample_with_prefilter_and_comb(
                routing.route,
                p.mix,
                routing.first,
                crate::filter_routing::Filter2Frame {
                    coefficients: routing.second,
                    comb,
                },
                (primary, secondary, noise, inputs.mixer_bias),
                pre_filter,
            );
            self.filter = graph.first;
            self.second_filter = graph.second;
            output
        } else {
            self.filter.next_sample(pre_filter(mixed), p.filter)
        };
        let filtered =
            if let Some(shaper) = p.shaper.filter(|s| s.position == ShaperPosition::PreAmp) {
                self.waveshaper.process(
                    ShaperSignal {
                        input: filter_output,
                        primary_pitch_code: p.primary_pitch_code,
                        primary_increment: p.primary.base_increment(),
                    },
                    shaper.coefficients,
                    &table.shapers,
                )
            } else {
                filter_output
            };
        let level = self.envelope.step(p.envelope_target, p.envelope_rate);
        let amplified = Sample(saturate(multiply_q15(filtered.0, level)));
        let stereo = pan::route(amplified, p.pan_position, existing);
        VoiceOutput {
            mixed,
            filtered,
            level,
            amplified,
            stereo,
        }
    }
}
