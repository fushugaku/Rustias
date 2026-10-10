//! Synthesis use cases. The domain owns arithmetic; adapters own external data.
#![no_std]
pub mod effect_audio;
pub mod dsp_audio_exchange;
extern crate alloc;
#[cfg(feature = "web-modular")]
use alloc::boxed::Box;
#[cfg(feature = "web-modular")]
pub trait VoiceCircuit: radias_synth_domain::voice::SignalProcessor {
    fn fresh(&self) -> Box<dyn VoiceCircuit>;
    fn as_any(&self) -> &dyn core::any::Any;
    fn reconfigure(&mut self, prototype: &dyn VoiceCircuit);
    fn controls(&mut self, values: [f64; 8]);
    fn tail_active(&self) -> bool;
}
pub mod actor_construction;
pub mod actor_copy;
pub mod actor_lifecycle;
pub mod actor_note_initialization;
pub mod actor_note_preparation;
pub mod actor_preparation;
pub mod actor_startup;
pub mod amplifier;
pub mod amplifier_transport;
pub mod auto_pan_delay;
pub mod cabinet_effect;
pub mod chorus_effect;
pub mod clock;
pub mod comb;
pub mod comb_pointer_publication;
pub mod complete_actor_startup;
pub mod construction_first_pass;
pub mod decimator_effect;
pub mod delay_effect;
pub mod drum_program;
pub mod dsp_buffers;
pub mod dsp_dispatch;
pub mod dsp_receiver;
pub mod dsp_transport;
pub mod dynamics_effect;
pub mod early_reflect_effect;
pub mod early_reflect_time;
pub mod effect_buffer_allocation;
pub mod effect_buffers;
pub mod effect_modulation;
pub mod mixed_effect_midi;
pub mod mixed_effect_parameter;
pub mod effect_pair_transition;
pub mod effect_parameters;
pub mod effect_routing;
pub mod effect_rack_initialization;
pub mod effect_rack_rebuild;
pub mod timbre_output;
pub mod master_pressure_type_change;
pub mod master_pressure_type_value;
pub mod master_assignment_release;
pub mod master_parameter_caller;
pub mod insert_parameter_caller;
pub mod effect_header_event;
pub mod effect_property;
pub mod effect_value_change;
pub mod master_type_value_change;
pub mod master_type_value_streaming;
pub mod effect_transition_queue;
pub mod effects;
pub mod ensemble_effect;
pub mod equalizer_effect;
pub mod filter1_initial_publication;
pub mod filter2;
pub mod filter_effect;
pub mod flanger_phaser_effect;
pub mod inactive_frame;
pub mod insert_effect_initialization;
pub mod insert_paired_initialization;
pub mod insert_effect_control;
pub mod insert_program_initialization;
pub mod lfo;
pub mod live_modulation;
pub mod manual_parameters;
pub mod mixer;
pub mod mod_delay;
pub mod modulation;
pub mod motion_initialization;
pub mod noise;
pub mod note_modulators;
pub mod note_pitch;
pub mod note_refresh;
pub mod parameter_transport;
pub mod pitch_delivery;
pub mod pitch_grain_shifter;
pub mod polyphony;
pub mod portamento;
pub mod primary;
pub mod program;
pub mod reverb_effect;
pub mod reverb_time;
pub mod secondary;
pub mod shaper;
pub mod shared_lfo;
pub mod stored_program;
pub mod synthesis_transport;
pub mod tremolo_ring_mod_effect;
pub mod tube_effect;
pub mod voice_envelopes;
pub mod voice_groups;
pub mod vocoder;
pub mod wah_effect;

use radias_synth_domain::control_slew::SlewWeights;
use radias_synth_domain::filter::{FilterCoefficients, ResonantFilter};
use radias_synth_domain::oscillator::Oscillator;
use radias_synth_domain::pitch::{PhaseIncrement, PitchCode, PitchTable};
use radias_synth_domain::{
    Sample,
    waveform::{WaveformFrame, WaveformTable},
};
use radias_synth_domain::{
    fixed::saturate,
    pan::StereoFrame,
    voice::{Voice, VoiceParameters},
};

/// Compile a DSP pitch parameter without constructing a firmware machine.
pub fn compile_pitch(tables: &PitchTable, code: u16) -> Option<PhaseIncrement> {
    PitchCode::new(code).map(|pitch| tables.increment(pitch))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockLengthMismatch;

/// Render a prepared phase/control block directly. No allocations or callbacks
/// occur here. Event/patch compilation is separate from sample arithmetic.
pub fn render_waveform_block(
    table: &WaveformTable,
    input: &[WaveformFrame],
    output: &mut [Sample],
) -> Result<(), BlockLengthMismatch> {
    if input.len() != output.len() {
        return Err(BlockLengthMismatch);
    }
    for (frame, sample) in input.iter().zip(output) {
        *sample = table.sample(
            frame.transfer,
            frame.phase,
            frame.edge_phase,
            frame.parameters,
        );
    }
    Ok(())
}

/// Live-ready use case: state belongs to the oscillator, callback size has no
/// effect on sound, and neither firmware nor prepared per-sample input is used.
pub fn render_oscillator_block(
    table: &WaveformTable,
    oscillator: &mut Oscillator,
    output: &mut [Sample],
) {
    for sample in output {
        *sample = oscillator.next_sample(table);
    }
}

/// Filter a stream in place; state is continuous across device callbacks.
pub fn render_filter_block(
    filter: &mut ResonantFilter,
    coefficients: FilterCoefficients,
    samples: &mut [Sample],
) {
    for sample in samples {
        *sample = filter.next_sample(*sample, coefficients);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceControlEvent {
    pub frame: u64,
    pub parameters: VoiceParameters,
}

/// Sample clock and event scheduling belong to the application, not the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePitchState {
    primary: radias_synth_domain::primary_oscillator::PrimaryParameters,
    code: u16,
    pitch_override: Option<(PhaseIncrement, i16)>,
    code_override: Option<u16>,
    primary_override: Option<radias_synth_domain::primary_oscillator::PrimaryParameters>,
    control: Option<i16>,
    current: Option<i16>,
    ratio: Option<i16>,
    noise: Option<noise::NoiseVoiceControl>,
    secondary: Oscillator,
    secondary_modulation: Option<radias_synth_domain::secondary_control::SecondaryModulation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceScalarState {
    mix: radias_synth_domain::mixer::OscillatorMix,
    pan_position: i32,
    mixer_override: Option<radias_synth_domain::mixer::OscillatorMix>,
    mixer_target: Option<radias_synth_domain::mixer::OscillatorMix>,
    pan_override: Option<(radias_synth_domain::pan::PanSmoother, SlewWeights)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceFilter2State {
    state: radias_synth_domain::filter_routing::Filter2State,
    parameters: Option<radias_synth_domain::filter_routing::DualFilterParameters>,
    current: Option<radias_synth_domain::filter_routing::Filter2Coefficients>,
    target: Option<radias_synth_domain::filter_routing::Filter2Coefficients>,
    routing: Option<Option<radias_synth_domain::filter_routing::FilterRouting>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceShaperState {
    state: radias_synth_domain::waveshaper::ShaperState,
    parameters: Option<radias_synth_domain::waveshaper::ShaperParameters>,
    current: Option<Option<radias_synth_domain::waveshaper::ShaperParameters>>,
    target: Option<radias_synth_domain::waveshaper::ShaperParameters>,
}
pub struct VoiceRenderer {
    #[cfg(feature = "web-modular")]
    pub circuit: Option<Box<dyn VoiceCircuit>>,
    pub voice: Voice,
    pub comb: Option<radias_synth_domain::comb::Comb>,
    parameters: VoiceParameters,
    next_event: usize,
    frame: u64,
    pitch_override: Option<(PhaseIncrement, i16)>,
    pitch_code_override: Option<u16>,
    primary_override: Option<radias_synth_domain::primary_oscillator::PrimaryParameters>,
    primary_control_override: Option<i16>,
    primary_control_current: Option<i16>,
    primary_ratio_override: Option<i16>,
    primary_control_slew_phase: u8,
    noise_control: Option<noise::NoiseVoiceControl>,
    unison_phase_key: Option<(u8, i16, i8, bool)>,
    secondary_modulation_override:
        Option<radias_synth_domain::secondary_control::SecondaryModulation>,
    filter_override: Option<FilterCoefficients>,
    filter_target: Option<FilterCoefficients>,
    routing_override: Option<Option<radias_synth_domain::filter_routing::FilterRouting>>,
    filter2_override: Option<radias_synth_domain::filter_routing::Filter2Coefficients>,
    filter2_target: Option<radias_synth_domain::filter_routing::Filter2Coefficients>,
    shaper_override: Option<Option<radias_synth_domain::waveshaper::ShaperParameters>>,
    shaper_target: Option<radias_synth_domain::waveshaper::ShaperParameters>,
    mixer_override: Option<radias_synth_domain::mixer::OscillatorMix>,
    mixer_target: Option<radias_synth_domain::mixer::OscillatorMix>,
    mixer_slew_phase: Option<u8>,
    control_slew: Option<(SlewWeights, u8)>,
    envelope_override: Option<i16>,
    envelope_rate_override: Option<i16>,
    pan_override: Option<(radias_synth_domain::pan::PanSmoother, SlewWeights)>,
    gate: bool,
    last_amplified: Sample,
    last_pan_current: i16,
}

impl VoiceRenderer {
    pub fn shaper_state(&self) -> VoiceShaperState {
        VoiceShaperState {
            state: self.voice.waveshaper.state,
            parameters: self.parameters.shaper,
            current: self.shaper_override,
            target: self.shaper_target,
        }
    }
    pub fn restore_shaper_state(&mut self, state: VoiceShaperState) {
        self.voice.waveshaper.state = state.state;
        self.parameters.shaper = state.parameters;
        self.shaper_override = state.current;
        self.shaper_target = state.target;
    }
    pub fn filter2_state(&self) -> VoiceFilter2State {
        VoiceFilter2State {
            state: self.voice.second_filter.state,
            parameters: self.parameters.routing,
            current: self.filter2_override,
            target: self.filter2_target,
            routing: self.routing_override,
        }
    }
    pub fn restore_filter2_state(&mut self, state: VoiceFilter2State) {
        self.voice.second_filter.state = state.state;
        self.parameters.routing = state.parameters;
        self.filter2_override = state.current;
        self.filter2_target = state.target;
        self.routing_override = state.routing;
    }
    pub fn scalar_state(&self) -> VoiceScalarState {
        VoiceScalarState {
            mix: self.parameters.mix,
            pan_position: self.parameters.pan_position,
            mixer_override: self.mixer_override,
            mixer_target: self.mixer_target,
            pan_override: self.pan_override,
        }
    }
    pub fn restore_scalar_state(&mut self, state: VoiceScalarState) {
        self.parameters.mix = state.mix;
        self.parameters.pan_position = state.pan_position;
        self.mixer_override = state.mixer_override;
        self.mixer_target = state.mixer_target;
        self.pan_override = state.pan_override;
    }
    pub fn pitch_state(&self) -> VoicePitchState {
        VoicePitchState {
            primary: self.parameters.primary,
            code: self.parameters.primary_pitch_code,
            pitch_override: self.pitch_override,
            code_override: self.pitch_code_override,
            primary_override: self.primary_override,
            control: self.primary_control_override,
            current: self.primary_control_current,
            ratio: self.primary_ratio_override,
            noise: self.noise_control,
            secondary: self.voice.secondary,
            secondary_modulation: self.secondary_modulation_override,
        }
    }
    pub fn restore_pitch_state(&mut self, state: VoicePitchState) {
        self.parameters.primary = state.primary;
        self.parameters.primary_pitch_code = state.code;
        self.pitch_override = state.pitch_override;
        self.pitch_code_override = state.code_override;
        self.primary_override = state.primary_override;
        self.primary_control_override = state.control;
        self.primary_control_current = state.current;
        self.primary_ratio_override = state.ratio;
        self.noise_control = state.noise;
        self.voice.secondary = state.secondary;
        self.secondary_modulation_override = state.secondary_modulation;
    }
    /// Construct a new note from a prepared template without copying an
    /// active Comb delay or retaining another actor's private controls.
    pub fn fresh_note(&self) -> Self {
        let mut note = Self::new(self.voice, self.parameters);
        note.pitch_override = self.pitch_override;
        note.pitch_code_override = self.pitch_code_override;
        note.primary_override = self.primary_override;
        note.primary_control_override = self.primary_control_override;
        note.primary_control_current = self.primary_control_current;
        note.primary_ratio_override = self.primary_ratio_override;
        note.primary_control_slew_phase = self.primary_control_slew_phase;
        #[cfg(feature = "web-modular")]
        {
            note.circuit = self.circuit.as_ref().map(|p| p.fresh());
        }
        note.noise_control = self.noise_control;
        note.unison_phase_key = self.unison_phase_key;
        note.secondary_modulation_override = self.secondary_modulation_override;
        note.filter_override = self.filter_override;
        note.filter_target = self.filter_target;
        note.routing_override = self.routing_override;
        note.filter2_override = self.filter2_override;
        note.filter2_target = self.filter2_target;
        note.shaper_override = self.shaper_override;
        note.shaper_target = self.shaper_target;
        note.mixer_override = self.mixer_override;
        note.mixer_target = self.mixer_target;
        note.mixer_slew_phase = self.mixer_slew_phase;
        note.control_slew = self.control_slew;
        note.envelope_override = self.envelope_override;
        note.envelope_rate_override = self.envelope_rate_override;
        note.pan_override = self.pan_override;
        if self.comb.is_some() && note.comb.is_none() {
            note.comb = Some(radias_synth_domain::comb::Comb {
                feedback: Default::default(),
                delay: Default::default(),
            });
        }
        note
    }
    pub fn rendered_frames(&self) -> u64 {
        self.frame
    }
    pub fn new(voice: Voice, parameters: VoiceParameters) -> Self {
        Self {
            voice,
            #[cfg(feature = "web-modular")]
            circuit: None,
            comb: parameters
                .routing
                .filter(|r| {
                    r.second.output == radias_synth_domain::filter_routing::Filter2Output::Comb
                })
                .map(|_| radias_synth_domain::comb::Comb {
                    feedback: Default::default(),
                    delay: Default::default(),
                }),
            parameters,
            next_event: 0,
            frame: 0,
            pitch_override: None,
            pitch_code_override: None,
            primary_override: None,
            primary_control_override: None,
            primary_control_current: None,
            primary_ratio_override: None,
            primary_control_slew_phase: 3,
            noise_control: None,
            unison_phase_key: None,
            secondary_modulation_override: None,
            filter_override: None,
            filter_target: None,
            routing_override: None,
            filter2_override: None,
            filter2_target: None,
            shaper_override: None,
            shaper_target: None,
            mixer_override: None,
            mixer_target: None,
            mixer_slew_phase: None,
            control_slew: None,
            envelope_override: None,
            envelope_rate_override: None,
            pan_override: None,
            gate: true,
            last_amplified: Sample(0),
            last_pan_current: 0,
        }
    }

    pub fn set_pitch(&mut self, increment: PhaseIncrement, bandwidth: i16) {
        self.pitch_override = Some((increment, bandwidth));
    }
    pub fn current_pitch(&self) -> (u16, PhaseIncrement, i16) {
        let primary = self.primary_override.unwrap_or(self.parameters.primary);
        let (increment, bandwidth) = self.pitch_override.unwrap_or((primary.base_increment(), 0));
        (self.primary_pitch_code(), increment, bandwidth)
    }
    pub fn set_secondary_coefficients(
        &mut self,
        coefficients: radias_synth_domain::pitch_receiver::SecondaryPitchCoefficients,
    ) {
        self.voice.secondary.retune(
            coefficients.increment,
            coefficients.edge,
            coefficients.bandwidth,
        );
    }
    pub fn last_amplified(&self) -> Sample {
        self.last_amplified
    }
    pub fn last_pan_current(&self) -> i16 {
        self.last_pan_current
    }
    pub fn set_primary_pitch_code(&mut self, code: PitchCode) {
        self.pitch_code_override = Some(code.raw());
    }
    pub fn primary_pitch_code(&self) -> u16 {
        self.pitch_code_override
            .unwrap_or(self.parameters.primary_pitch_code)
    }
    pub fn envelope_rate(&self) -> i16 {
        self.envelope_rate_override
            .unwrap_or(self.parameters.envelope_rate)
    }
    pub fn set_envelope_rate(&mut self, rate: i16) {
        self.envelope_rate_override = Some(rate);
    }
    /// Current DSP coefficients for read-only conformance diagnostics.
    pub fn current_filter(&self) -> FilterCoefficients {
        self.filter_override.unwrap_or(self.parameters.filter)
    }
    pub fn current_mixer(&self) -> radias_synth_domain::mixer::OscillatorMix {
        self.mixer_override.unwrap_or(self.parameters.mix)
    }
    pub fn set_primary_waveform_control(&mut self, control: i16) {
        self.primary_control_override = Some(control);
    }
    pub fn set_primary_ratio(&mut self, ratio: i16) {
        self.primary_ratio_override = Some(ratio);
    }
    pub fn initialize_primary_control(&mut self, current: i16, phase: u8) {
        self.primary_control_current = Some(current);
        self.primary_control_slew_phase = phase & 3;
    }
    pub fn configure_noise_control(&mut self, control: noise::NoiseVoiceControl) {
        self.noise_control = Some(control);
    }
    pub fn noise_control_mut(&mut self) -> Option<&mut noise::NoiseVoiceControl> {
        self.noise_control.as_mut()
    }
    pub fn set_noise_target(&mut self, target: noise::NoiseTarget, initialize: bool) {
        if let Some(control) = &mut self.noise_control {
            control.update(target);
            if initialize {
                control.initialize_targets();
            }
        }
    }
    pub fn initialize_unison_phases(&mut self, control2_code: u16, triangle: bool) {
        if let Some(phases) =
            radias_synth_domain::unison_pitch::unison_phases(control2_code, triangle)
        {
            self.voice.primary.unison.phases = phases;
            self.voice.primary.phase = phases[0];
        }
    }
    pub fn update_unison_phases(
        &mut self,
        control: radias_synth_domain::controller_primary::PrimaryControl,
        triangle: bool,
    ) {
        let key = (
            control.control2,
            control.control2_modulation,
            control.control2_manual_offset,
            triangle,
        );
        if self.unison_phase_key != Some(key) {
            self.initialize_unison_phases(control.phase_code(), triangle);
            self.unison_phase_key = Some(key);
        }
    }
    pub fn select_primary(
        &mut self,
        parameters: radias_synth_domain::primary_oscillator::PrimaryParameters,
    ) {
        self.primary_override = Some(parameters);
        // The newly selected generator receives its own controller format.
        // A ramp width word must not become a sine modulation depth.
        self.primary_control_override = None;
        self.primary_control_current = Some(0);
        self.primary_ratio_override = None;
        self.unison_phase_key = None;
        self.noise_control = None;
    }
    /// A descriptor construction word updates coefficients while preserving
    /// oscillator phases. Its controller and physical-state commands are
    /// separate publications owned by the caller.
    pub fn receive_primary_parameters(
        &mut self,
        parameters: radias_synth_domain::primary_oscillator::PrimaryParameters,
    ) {
        self.primary_override = Some(parameters);
    }
    pub fn set_secondary_pitch(
        &mut self,
        code: PitchCode,
        pitch: &PitchTable,
        bandwidth: &radias_synth_domain::bandlimit::BandwidthTable,
    ) {
        self.set_secondary_pitch_mode(code, pitch, bandwidth, false);
    }
    pub fn select_secondary(&mut self, program: secondary::SecondaryProgram) {
        self.voice.secondary.select_transfer(program.transfer(), 0);
        self.secondary_modulation_override = Some(program.modulation());
    }
    pub fn clear_secondary_tuning(&mut self) {
        self.voice.secondary.retune(PhaseIncrement(0), 0, 0);
    }
    pub fn secondary_sync(&self) -> bool {
        self.secondary_modulation_override
            .unwrap_or(self.parameters.secondary_modulation)
            .sync
    }
    pub fn set_secondary_sync(&mut self, sync: bool) {
        let mut modulation = self
            .secondary_modulation_override
            .unwrap_or(self.parameters.secondary_modulation);
        modulation.sync = sync;
        self.secondary_modulation_override = Some(modulation);
    }
    pub fn set_secondary_pitch_mode(
        &mut self,
        code: PitchCode,
        pitch: &PitchTable,
        bandwidth: &radias_synth_domain::bandlimit::BandwidthTable,
        sync: bool,
    ) {
        let increment = pitch.increment(code);
        self.voice.secondary.retune(
            increment,
            radias_synth_domain::bandlimit::edge_coefficient(code, sync),
            bandwidth.coefficient(increment),
        );
    }
    pub fn set_filter(&mut self, coefficients: FilterCoefficients) {
        if self.control_slew.is_some() {
            self.filter_override.get_or_insert(self.parameters.filter);
            self.filter_target = Some(coefficients);
        } else {
            self.set_filter_immediate(coefficients);
        }
    }
    /// Original fresh-note DSP reset clears private Filter1 history. Physical
    /// oscillator phases and the separately owned Filter2/Comb state are untouched.
    pub fn reset_filter_memory(&mut self) {
        self.voice.filter.state = radias_synth_domain::filter::FilterState::default();
    }
    pub fn set_filter_immediate(&mut self, coefficients: FilterCoefficients) {
        self.filter_override = Some(coefficients);
        self.filter_target = Some(coefficients);
    }
    pub fn set_filter_routing(
        &mut self,
        routing: Option<radias_synth_domain::filter_routing::FilterRouting>,
        second: radias_synth_domain::filter_routing::Filter2Coefficients,
    ) {
        if second.output == radias_synth_domain::filter_routing::Filter2Output::Comb
            && self.comb.is_none()
        {
            self.comb = Some(radias_synth_domain::comb::Comb {
                feedback: Default::default(),
                delay: Default::default(),
            });
        }
        self.routing_override = Some(routing);
        if self.control_slew.is_some() {
            self.filter2_override.get_or_insert(second).output = second.output;
            self.filter2_target = Some(second);
        } else {
            self.set_filter2_immediate(second);
        }
    }
    pub fn set_filter2_immediate(
        &mut self,
        second: radias_synth_domain::filter_routing::Filter2Coefficients,
    ) {
        self.filter2_override = Some(second);
        self.filter2_target = Some(second);
    }
    pub fn set_filter2_target(
        &mut self,
        second: radias_synth_domain::filter_routing::Filter2Coefficients,
    ) {
        if self.control_slew.is_some() {
            self.filter2_override.get_or_insert(second).output = second.output;
            self.filter2_target = Some(second);
        } else {
            self.set_filter2_immediate(second);
        }
    }
    pub fn filter2_target(
        &self,
    ) -> Option<radias_synth_domain::filter_routing::Filter2Coefficients> {
        self.filter2_target.or_else(|| self.current_filter2())
    }
    pub fn filter_routing(&self) -> Option<radias_synth_domain::filter_routing::FilterRouting> {
        self.routing_override
            .unwrap_or(self.parameters.routing.map(|r| r.route))
    }
    pub fn current_filter2(
        &self,
    ) -> Option<radias_synth_domain::filter_routing::Filter2Coefficients> {
        self.filter2_override
            .or(self.parameters.routing.map(|routing| routing.second))
    }
    pub fn shaper_target(&self) -> Option<radias_synth_domain::waveshaper::ShaperParameters> {
        self.shaper_target.or_else(|| self.current_shaper())
    }
    pub fn set_shaper_depth(&mut self, value: i16) {
        if let Some(mut target) = self.shaper_target() {
            target.coefficients.set_depth(value);
            if let radias_synth_domain::waveshaper::ShaperCoefficients::SubOscillator(c) =
                &mut target.coefficients
            {
                c.target_depth = value;
            }
            self.set_shaper(Some(target));
        }
    }
    pub fn current_shaper(&self) -> Option<radias_synth_domain::waveshaper::ShaperParameters> {
        self.shaper_override.unwrap_or(self.parameters.shaper)
    }
    pub fn set_shaper_immediate(
        &mut self,
        shaper: Option<radias_synth_domain::waveshaper::ShaperParameters>,
    ) {
        self.shaper_override = Some(shaper);
        self.shaper_target = shaper;
    }
    pub fn set_shaper(
        &mut self,
        target: Option<radias_synth_domain::waveshaper::ShaperParameters>,
    ) {
        if self.control_slew.is_none() {
            self.set_shaper_immediate(target);
            return;
        }
        let mut current = target;
        if let (Some(old), Some(new)) = (self.current_shaper(), &mut current)
            && core::mem::discriminant(&old.coefficients)
                == core::mem::discriminant(&new.coefficients)
        {
            new.coefficients.set_depth(old.coefficients.depth());
            if let Some(gain) = old.coefficients.gain_current() {
                new.coefficients.set_gain_current(gain);
            }
        }
        self.shaper_override = Some(current);
        self.shaper_target = target;
    }
    pub fn initialize_mixer(&mut self, current: radias_synth_domain::mixer::OscillatorMix) {
        self.mixer_override = Some(current);
        self.mixer_target = Some(current);
    }
    pub fn set_mixer(&mut self, target: radias_synth_domain::mixer::OscillatorMix) {
        if self.control_slew.is_some() {
            self.mixer_override.get_or_insert(self.parameters.mix);
            self.mixer_target = Some(target);
        } else {
            self.initialize_mixer(target);
        }
    }
    pub fn set_mixer_band(&mut self, band: u8, value: i16) {
        let mut target = self.mixer_target.unwrap_or(self.current_mixer());
        match band {
            0 => target.primary_gain = value,
            1 => target.secondary_gain = value,
            2 => target.noise_gain = value,
            _ => return,
        }
        self.set_mixer(target);
    }
    pub fn mixer_slew_phase(&mut self, phase: u8) {
        self.mixer_slew_phase = Some(phase & 3);
    }
    pub fn control_slew(&mut self, weights: SlewWeights, phase: u8) {
        self.control_slew = Some((weights, phase & 3));
    }
    pub fn release(&mut self) {
        self.gate = false;
    }
    pub fn set_envelope_target(&mut self, target: i16) {
        self.envelope_override = Some(target);
    }
    pub fn envelope_target(&self) -> i16 {
        self.envelope_override
            .unwrap_or(self.parameters.envelope_target)
    }
    pub fn initialize_pan(
        &mut self,
        state: radias_synth_domain::pan::PanSmoother,
        weights: SlewWeights,
    ) {
        self.pan_override = Some((state, weights));
    }
    pub fn set_pan_target(&mut self, target: i16) {
        if let Some((pan, _)) = &mut self.pan_override {
            pan.target = target;
        }
    }
    pub fn render(
        &mut self,
        table: &WaveformTable,
        events: &[VoiceControlEvent],
        output: &mut [StereoFrame],
    ) {
        for sample in output {
            *sample = scale_bus(self.next_on_bus(table, events, StereoFrame::default()));
        }
    }

    /// The instrument accumulates voices here and scales each completed bus once.
    pub fn next_on_bus(
        &mut self,
        table: &WaveformTable,
        events: &[VoiceControlEvent],
        existing: StereoFrame,
    ) -> StereoFrame {
        self.next_on_bus_with_inputs(
            table,
            events,
            existing,
            radias_synth_domain::voice::VoiceFrameInputs::default(),
        )
    }
    pub fn next_on_bus_with_inputs(
        &mut self,
        table: &WaveformTable,
        events: &[VoiceControlEvent],
        existing: StereoFrame,
        inputs: radias_synth_domain::voice::VoiceFrameInputs,
    ) -> StereoFrame {
        while let Some(event) = events.get(self.next_event) {
            if event.frame > self.frame {
                break;
            }
            self.parameters = event.parameters;
            self.next_event += 1;
        }
        let mut parameters = self.parameters;
        if let Some(rate) = self.envelope_rate_override {
            parameters.envelope_rate = rate;
        }
        if let Some(primary) = self.primary_override {
            parameters.primary = primary;
        }
        if let Some(control) = self.primary_control_override {
            use radias_synth_domain::primary_oscillator::PrimaryParameters;
            match &mut parameters.primary {
                PrimaryParameters::Ramp(p) | PrimaryParameters::Pulse(p) => {
                    p.offset_target = control
                }
                PrimaryParameters::Triangle(p) => {
                    p.edge_gain = *self.primary_control_current.get_or_insert(p.edge_gain);
                }
                PrimaryParameters::Sine(p) => {
                    p.control[1] = control;
                    p.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulation_gain);
                }
                PrimaryParameters::Cross(p) => {
                    p.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulation_gain);
                }
                PrimaryParameters::CrossTriangle(p) => {
                    p.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulation_gain);
                }
                PrimaryParameters::CrossSine(p) => {
                    p.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulation_gain);
                }
                PrimaryParameters::Unison(p) => {
                    p.detune = control.max(0) as u16;
                    if self.pitch_override.is_none() {
                        p.retune(p.increments[0]);
                    }
                }
                PrimaryParameters::UnisonCarrier(p) => {
                    p.parameters.detune = control.max(0) as u16;
                    if self.pitch_override.is_none() {
                        p.parameters.retune(p.parameters.increments[0]);
                        if let radias_synth_domain::unison::UnisonWaveform::Pulse { bandwidth } =
                            &mut p.waveform
                        {
                            *bandwidth = radias_synth_domain::unison_pitch::unison_bandwidth(
                                p.parameters.increments[0],
                            )
                            .1;
                        }
                    }
                }
                PrimaryParameters::Vpm(p) => {
                    p.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulation_gain);
                }
                PrimaryParameters::VpmCarrier(p) => {
                    p.modulator.modulation_gain = *self
                        .primary_control_current
                        .get_or_insert(p.modulator.modulation_gain);
                }
                // Their dedicated coefficient blocks do not use the VA
                // waveform/Cross/Unison/VPM scalar control register.
                PrimaryParameters::Noise(_) | PrimaryParameters::Formant(_) => {}
            }
        }
        if let Some((increment, bandwidth)) = self.pitch_override {
            parameters.primary.retune(increment, bandwidth);
        }
        if let Some(code) = self.pitch_code_override {
            parameters.primary_pitch_code = code;
        }
        if let Some(ratio) = self.primary_ratio_override {
            use radias_synth_domain::primary_oscillator::PrimaryParameters;
            match &mut parameters.primary {
                PrimaryParameters::Vpm(p) => p.ratio = ratio,
                PrimaryParameters::VpmCarrier(p) => p.modulator.ratio = ratio,
                _ => {}
            }
        }
        if let Some(control) = self.noise_control {
            parameters.primary = control.parameters();
        }
        if let Some(modulation) = self.secondary_modulation_override {
            parameters.secondary_modulation = modulation;
        }
        if let Some(filter) = self.filter_override {
            parameters.filter = filter;
        }
        if let Some(routing) = self.routing_override {
            parameters.routing =
                routing.map(
                    |route| radias_synth_domain::filter_routing::DualFilterParameters {
                        route,
                        first: parameters.filter,
                        second: self
                            .filter2_override
                            .expect("Live Filter2 coefficients configured"),
                    },
                );
        } else if let Some(routing) = &mut parameters.routing {
            if self.filter_override.is_some() {
                routing.first = parameters.filter;
            }
            if let Some(second) = self.filter2_override {
                routing.second = second;
            }
        }
        if let Some(mixer) = self.mixer_override {
            parameters.mix = mixer;
        }
        if let Some(shaper) = self.shaper_override {
            parameters.shaper = shaper;
        }
        if let Some(target) = self.envelope_override {
            parameters.envelope_target = target;
        }
        if let Some((pan, weights)) = &mut self.pan_override {
            parameters.pan_position = pan.next(*weights);
        }
        if !self.gate {
            parameters.envelope_target = 0;
        }
        let output = self.voice.next_sample_routed(
            table,
            parameters,
            existing,
            self.comb.as_mut(),
            inputs,
            #[cfg(feature = "web-modular")]
            self.circuit
                .as_mut()
                .map(|p| &mut **p as &mut dyn radias_synth_domain::voice::SignalProcessor),
        );
        self.last_amplified = output.amplified;
        self.last_pan_current = (parameters.pan_position >> 16) as i16;
        let native = output.stereo;
        if let Some((weights, _)) = self.control_slew
            && (self.frame + self.primary_control_slew_phase as u64) & 3 == 3
            && let Some(control) = &mut self.noise_control
        {
            control.advance(weights);
        }
        if let Some((weights, _)) = self.control_slew
            && (self.frame + self.primary_control_slew_phase as u64) & 3 == 3
            && let Some(current) = &mut self.primary_control_current
        {
            let target = match parameters.primary {
                radias_synth_domain::primary_oscillator::PrimaryParameters::Sine(_) => {
                    self.voice.primary.control_feedback
                }
                _ => self.primary_control_override.unwrap_or(*current),
            };
            *current = weights.word(*current, target);
        }
        if let Some((weights, phase)) = self.control_slew
            && (self.frame + phase as u64) & 3 == 3
            && let (Some(current), Some(target)) = (&mut self.filter_override, self.filter_target)
        {
            weights.filter(current, target);
        }
        if let Some((weights, filter_phase)) = self.control_slew
            && (self.frame + filter_phase as u64) & 3 == 3
            && let (Some(current), Some(target)) = (&mut self.filter2_override, self.filter2_target)
        {
            current.input_gain = weights.word(current.input_gain, target.input_gain);
            current.feedback = weights.wide(current.feedback, target.feedback);
            current.integrator_gain = weights.wide(current.integrator_gain, target.integrator_gain);
            current.output = target.output;
        }
        if let Some((weights, filter_phase)) = self.control_slew
            && (self.frame + self.mixer_slew_phase.unwrap_or(filter_phase) as u64) & 3 == 3
            && let (Some(current), Some(target)) = (&mut self.mixer_override, self.mixer_target)
        {
            weights.mixer(current, target);
        }
        if let Some((weights, phase)) = self.control_slew
            && (self.frame + phase as u64) & 3 == 3
            && let (Some(Some(current)), Some(target)) =
                (&mut self.shaper_override, self.shaper_target)
        {
            let gain = current
                .coefficients
                .gain_target(parameters.primary_pitch_code);
            current
                .coefficients
                .set_depth(weights.word(current.coefficients.depth(), target.coefficients.depth()));
            if let (Some(old), Some(target)) = (current.coefficients.gain_current(), gain) {
                current
                    .coefficients
                    .set_gain_current(weights.word(old, target));
            }
        }
        self.frame += 1;
        native
    }
}

pub fn scale_bus(native: StereoFrame) -> StereoFrame {
    StereoFrame {
        left: Sample(saturate((native.left.0 as i64) << 5)),
        right: Sample(saturate((native.right.0 as i64) << 5)),
    }
}

pub mod vibrato_effect;

pub mod rotary_effect;

pub mod talking_effect;

pub mod master_effect_control;

pub mod master_effect_buffers;

pub mod master_effect_construction;

pub mod master_effect_initialization;
pub mod master_rack_coefficients;
pub mod master_initial_mask;

pub mod master_effect_type_change;

pub mod insert_effect_construction;
pub mod insert_type_construction;
