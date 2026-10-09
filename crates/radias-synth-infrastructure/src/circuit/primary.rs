//! Independent OSC1 controllers for the optional browser graph.
//! Generator arithmetic, coefficient transfers and phase initialization stay
//! in the shared synthesis crates; this adapter owns only a module's state.
use radias_synth_application::{
    noise::{NoiseTables, NoiseVoiceControl},
    primary::PrimaryProgram,
};
use radias_synth_domain::{
    Sample,
    bandlimit::BandwidthTable,
    control_slew::SlewWeights,
    noise::{FormantState, NoiseFrameSeeds},
    pitch::{PhaseIncrement, PitchCode},
    primary_oscillator::{PrimaryOscillator, PrimaryParameters},
    waveform::WaveformTable,
};

pub(super) struct Tables {
    noise: NoiseTables,
    bandwidth: BandwidthTable,
}
impl Tables {
    pub fn new() -> Self {
        Self {
            noise: crate::standalone_tables::noise(),
            bandwidth: crate::standalone_tables::bandwidth(),
        }
    }
}
pub(super) struct Controller {
    selection: u8,
    current: i16,
    phase_code: Option<u16>,
    noise: Option<NoiseVoiceControl>,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            selection: u8::MAX,
            current: 0,
            phase_code: None,
            noise: None,
        }
    }
}
impl Controller {
    #[allow(clippy::too_many_arguments)]
    pub fn next(
        &mut self,
        oscillator: &mut PrimaryOscillator,
        mut program: PrimaryProgram,
        increment: PhaseIncrement,
        code: PitchCode,
        lfo: i16,
        modulation: [i16; 2],
        modulator: Sample,
        tables: &Tables,
        waveform: &WaveformTable,
        frame: u64,
        id: usize,
    ) -> Sample {
        let fresh = self.selection != program.selection;
        if fresh {
            self.selection = program.selection;
            self.current = 0;
            self.phase_code = None;
            *oscillator = PrimaryOscillator::default();
            oscillator.phase = NoiseFrameSeeds::from_inputs(id as i16 + 1, 0).primary[0];
            oscillator.initialize_waveform_phase(program.selection);
            oscillator.formant = FormantState {
                counter: tables.noise.counters.for_slot(id as u8),
                ..Default::default()
            };
            self.noise = tables.noise.compile(program, code, lfo, modulation);
        }
        let weights = SlewWeights {
            target: 0x1d4,
            memory: 0x7e2d,
        };
        if let Some(noise) = &mut self.noise {
            // Controls use the same compiler and four-frame slewing as native OSC1.
            tables
                .noise
                .update_controls(noise, program.control, code, lfo, modulation);
            let coefficient = if program.selection == 4 {
                tables.noise.noise.curve_scale(code)
            } else {
                radias_synth_domain::noise_control::formant_frequency(increment)
            };
            noise.receive_pitch(increment, coefficient);
            if fresh {
                noise.initialize_targets();
            }
            if frame & 3 == 3 {
                noise.advance(weights);
            }
            return oscillator.next_with_modulator_and_bias(
                waveform,
                noise.parameters(),
                Sample(0),
                noise.current()[1],
            );
        }
        let target = program.waveform_control(lfo, modulation).unwrap_or(0);
        if frame & 3 == 3 {
            self.current = weights.word(self.current, target);
        }
        program.control.control1_modulation = modulation[0];
        program.control.control2_modulation = modulation[1];
        let mut parameters = program
            .compile_waveform(increment, tables.bandwidth.coefficient(increment))
            .unwrap();
        match &mut parameters {
            PrimaryParameters::Ramp(p) | PrimaryParameters::Pulse(p) => p.offset_target = target,
            PrimaryParameters::Triangle(p) => p.edge_gain = self.current,
            PrimaryParameters::Sine(p) => {
                p.control[1] = target;
                p.modulation_gain = self.current;
            }
            PrimaryParameters::Cross(p) => p.modulation_gain = self.current,
            PrimaryParameters::CrossTriangle(p) => p.modulation_gain = self.current,
            PrimaryParameters::CrossSine(p) => p.modulation_gain = self.current,
            PrimaryParameters::Unison(p) => {
                p.detune = target.max(0) as u16;
                p.retune(increment);
            }
            PrimaryParameters::UnisonCarrier(p) => {
                p.parameters.detune = target.max(0) as u16;
                p.parameters.retune(increment);
            }
            PrimaryParameters::Vpm(p) => {
                p.modulation_gain = self.current;
                p.ratio = program.control.vpm_ratio();
            }
            PrimaryParameters::VpmCarrier(p) => {
                p.modulator.modulation_gain = self.current;
                p.modulator.ratio = program.control.vpm_ratio();
            }
            PrimaryParameters::Noise(_) | PrimaryParameters::Formant(_) => unreachable!(),
        }
        if program.selection & 48 == 32 {
            let code = program.control.phase_code();
            if self.phase_code != Some(code) {
                if let Some(phases) = radias_synth_domain::unison_pitch::unison_phases(
                    code,
                    program.selection & 3 == 2,
                ) {
                    oscillator.unison.phases = phases;
                    oscillator.phase = phases[0];
                }
                self.phase_code = Some(code);
            }
        }
        oscillator.next_with_modulator(waveform, parameters, modulator)
    }
}
