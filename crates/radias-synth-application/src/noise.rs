//! Native Noise/Formant controls, separated from note allocation and audio I/O.
use radias_synth_domain::{
    control_slew::SlewWeights,
    controller_noise::{
        ColoredNoiseControl, FormantCompilerControl, FormantCounterSeeds, NoiseControl,
        formant_control1, noise_control1,
    },
    controller_primary::PrimaryControl,
    noise_control::{NoisePitchTable, formant_frequency},
    pitch::{PitchCode, PitchTable},
    primary_oscillator::{PrimaryFormantParameters, PrimaryNoiseParameters, PrimaryParameters},
};

/// Immutable firmware coefficients owned by the synthesis application.
/// Devices supply data; no emulator, captured voices or audio enter this type.
pub struct NoiseTables {
    pub pitch: PitchTable,
    pub noise: NoisePitchTable,
    pub counters: FormantCounterSeeds,
}

/// Controller packet values before ordered DSP delivery. Historical internal
/// oscillator names are retained; numeric selections remain firmware inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoiseControlTargets {
    pub shape: Option<u32>,
    pub excitation_gain: i16,
    pub excitation_bias: i16,
}

impl NoiseTables {
    pub fn compile(
        &self,
        program: crate::primary::PrimaryProgram,
        code: PitchCode,
        lfo1: i16,
        modulation: [i16; 2],
    ) -> Option<NoiseVoiceControl> {
        let mut control =
            NoiseVoiceControl::new(program.compile_waveform(self.pitch.increment(code), 0)?)?;
        self.update(&mut control, program.control, code, lfo1, modulation);
        control.initialize_targets();
        Some(control)
    }

    pub fn update(
        &self,
        voice: &mut NoiseVoiceControl,
        primary: PrimaryControl,
        code: PitchCode,
        lfo1: i16,
        modulation: [i16; 2],
    ) {
        self.update_controls(voice, primary, code, lfo1, modulation);
        voice.pitch(code, &self.pitch, &self.noise);
    }
    pub fn update_controls(
        &self,
        voice: &mut NoiseVoiceControl,
        primary: PrimaryControl,
        code: PitchCode,
        lfo1: i16,
        modulation: [i16; 2],
    ) {
        let targets = self.control_targets(voice, primary, code, lfo1, modulation);
        if let Some(shape) = targets.shape {
            voice.update(NoiseTarget::FormantShape {
                input_gain: (shape >> 16) as i16,
                feedback: shape as i16,
            });
        }
        voice.update(NoiseTarget::ExcitationGain(targets.excitation_gain));
        voice.update(NoiseTarget::ExcitationBias(targets.excitation_bias));
    }
    pub fn control_targets(
        &self,
        voice: &NoiseVoiceControl,
        mut primary: PrimaryControl,
        code: PitchCode,
        lfo1: i16,
        modulation: [i16; 2],
    ) -> NoiseControlTargets {
        primary.lfo1 = lfo1;
        primary.control1_modulation = modulation[0];
        primary.control2_modulation = modulation[1];
        let control = NoiseControl {
            control2: primary.control2,
            control2_modulation: primary.control2_modulation,
            control2_manual_offset: primary.control2_manual_offset,
        };
        let base = primary.compose().base;
        match voice {
            NoiseVoiceControl::Colored { .. } => {
                let target = control.colored(noise_control1(base));
                NoiseControlTargets {
                    shape: None,
                    excitation_gain: target.color,
                    excitation_bias: target.frequency,
                }
            }
            NoiseVoiceControl::Formant { .. } => {
                let target = control.formant(formant_control1(base), code.raw() as i16);
                NoiseControlTargets {
                    shape: Some(target.shape),
                    excitation_gain: target.input_gain,
                    excitation_bias: target.frequency,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseVoiceControl {
    Colored {
        parameters: PrimaryNoiseParameters,
        current: [i16; 2],
        target: [i16; 2],
    },
    Formant {
        parameters: PrimaryFormantParameters,
        current: [i16; 2],
        target: [i16; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseTarget {
    ExcitationGain(i16),
    ExcitationBias(i16),
    FormantShape { input_gain: i16, feedback: i16 },
}

impl NoiseVoiceControl {
    pub fn update(&mut self, command: NoiseTarget) {
        match command {
            NoiseTarget::ExcitationGain(value) | NoiseTarget::ExcitationBias(value) => {
                let index = usize::from(matches!(command, NoiseTarget::ExcitationBias(_)));
                match self {
                    Self::Colored { target, .. } | Self::Formant { target, .. } => {
                        target[index] = value
                    }
                }
            }
            NoiseTarget::FormantShape {
                input_gain,
                feedback,
            } => {
                if let Self::Formant { parameters, .. } = self {
                    parameters.generator.input_gain = input_gain;
                    parameters.generator.feedback = feedback;
                }
            }
        }
    }
    pub fn new(primary: PrimaryParameters) -> Option<Self> {
        Some(match primary {
            PrimaryParameters::Noise(parameters) => {
                let current = [
                    parameters.generator.phase_gain,
                    parameters.generator.seed_bias,
                ];
                Self::Colored {
                    parameters,
                    current,
                    target: current,
                }
            }
            PrimaryParameters::Formant(parameters) => {
                let current = [
                    parameters.generator.seed_gain,
                    parameters.generator.seed_bias,
                ];
                Self::Formant {
                    parameters,
                    current,
                    target: current,
                }
            }
            _ => return None,
        })
    }

    pub fn colored_target(&mut self, control: ColoredNoiseControl) {
        if let Self::Colored { target, .. } = self {
            *target = [control.color, control.frequency];
        }
    }

    pub fn formant_target(&mut self, control: FormantCompilerControl) {
        if let Self::Formant {
            parameters, target, ..
        } = self
        {
            *target = [control.input_gain, control.frequency];
            parameters.generator.input_gain = (control.shape >> 16) as i16;
            parameters.generator.feedback = control.shape as i16;
        }
    }

    pub fn pitch(&mut self, code: PitchCode, pitch: &PitchTable, noise: &NoisePitchTable) {
        let increment = pitch.increment(code);
        match self {
            Self::Colored { parameters, .. } => {
                parameters.increment = increment;
                parameters.generator.curve_scale = noise.curve_scale(code);
            }
            Self::Formant { parameters, .. } => {
                parameters.increment = increment;
                parameters.generator.frequency = formant_frequency(increment);
            }
        }
    }
    pub fn receive_pitch(
        &mut self,
        increment: radias_synth_domain::pitch::PhaseIncrement,
        coefficient: i16,
    ) {
        match self {
            Self::Colored { parameters, .. } => {
                parameters.increment = increment;
                parameters.generator.curve_scale = coefficient;
            }
            Self::Formant { parameters, .. } => {
                parameters.increment = increment;
                parameters.generator.frequency = coefficient;
            }
        }
    }

    pub fn current(&self) -> [i16; 2] {
        match self {
            Self::Colored { current, .. } | Self::Formant { current, .. } => *current,
        }
    }

    /// Original E056/E05A note initialization copies targets to currents.
    pub fn initialize_targets(&mut self) {
        match self {
            Self::Colored {
                current, target, ..
            }
            | Self::Formant {
                current, target, ..
            } => *current = *target,
        }
    }

    pub fn parameters(&self) -> PrimaryParameters {
        match *self {
            Self::Colored {
                mut parameters,
                current,
                ..
            } => {
                parameters.generator.phase_gain = current[0];
                parameters.generator.seed_bias = current[1];
                PrimaryParameters::Noise(parameters)
            }
            Self::Formant {
                mut parameters,
                current,
                ..
            } => {
                parameters.generator.seed_gain = current[0];
                parameters.generator.seed_bias = current[1];
                PrimaryParameters::Formant(parameters)
            }
        }
    }

    pub fn advance(&mut self, weights: SlewWeights) {
        let (current, target) = match self {
            Self::Colored {
                current, target, ..
            }
            | Self::Formant {
                current, target, ..
            } => (current, target),
        };
        for (current, target) in current.iter_mut().zip(target) {
            *current = weights.word(*current, *target);
        }
    }
}
