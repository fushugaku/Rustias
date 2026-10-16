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
    inputs: Option<ControlInputs>,
    target: i16,
    parameters: Option<PrimaryParameters>,
}
// The plan owns immutable tables. A new plan/table identity invalidates the
// cache; oscillator history, feedback and four-frame slew never live here.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ControlInputs {
    program: PrimaryProgram,
    increment: PhaseIncrement,
    code: PitchCode,
    lfo: i16,
    modulation: [i16; 2],
    tables: usize,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            selection: u8::MAX,
            current: 0,
            phase_code: None,
            noise: None,
            inputs: None,
            target: 0,
            parameters: None,
        }
    }
}
impl Controller {
    #[allow(clippy::too_many_arguments)]
    pub fn next(
        &mut self,
        oscillator: &mut PrimaryOscillator,
        program: PrimaryProgram,
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
        self.next_with_sync(
            oscillator, program, increment, code, lfo, modulation, modulator, tables, waveform,
            frame, id, false,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn next_with_sync(
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
        sync: bool,
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
            self.inputs = None;
            self.parameters = None;
        }
        let inputs = ControlInputs {
            program,
            increment,
            code,
            lfo,
            modulation,
            tables: tables as *const Tables as usize,
        };
        let changed = self.inputs != Some(inputs);
        let weights = SlewWeights {
            target: 0x1d4,
            memory: 0x7e2d,
        };
        if let Some(noise) = &mut self.noise {
            // Controls use the same compiler and four-frame slewing as native OSC1.
            if changed {
                tables
                    .noise
                    .update_controls(noise, program.control, code, lfo, modulation);
                let coefficient = if program.selection == 4 {
                    tables.noise.noise.curve_scale(code)
                } else {
                    radias_synth_domain::noise_control::formant_frequency(increment)
                };
                noise.receive_pitch(increment, coefficient);
                self.inputs = Some(inputs);
            }
            if fresh {
                noise.initialize_targets();
            }
            if frame & 3 == 3 {
                noise.advance(weights);
            }
            if sync {
                oscillator.phase = radias_synth_domain::Phase(0);
            }
            return oscillator.next_with_modulator_and_bias(
                waveform,
                noise.parameters(),
                Sample(0),
                noise.current()[1],
            );
        }
        if changed {
            self.target = program.waveform_control(lfo, modulation).unwrap_or(0);
        }
        let target = self.target;
        if frame & 3 == 3 {
            self.current = weights.word(self.current, target);
        }
        program.control.control1_modulation = modulation[0];
        program.control.control2_modulation = modulation[1];
        if changed {
            let mut parameters = program
                .compile_waveform(increment, tables.bandwidth.coefficient(increment))
                .unwrap();
            match &mut parameters {
                PrimaryParameters::Ramp(p) | PrimaryParameters::Pulse(p) => {
                    p.offset_target = target
                }
                PrimaryParameters::Sine(p) => p.control[1] = target,
                PrimaryParameters::Unison(p) => {
                    p.detune = target.max(0) as u16;
                    p.retune(increment);
                }
                PrimaryParameters::UnisonCarrier(p) => {
                    p.parameters.detune = target.max(0) as u16;
                    p.parameters.retune(increment);
                }
                PrimaryParameters::Vpm(p) => p.ratio = program.control.vpm_ratio(),
                PrimaryParameters::VpmCarrier(p) => p.modulator.ratio = program.control.vpm_ratio(),
                _ => {}
            }
            self.parameters = Some(parameters);
            self.inputs = Some(inputs);
        }
        let mut parameters = self.parameters.unwrap();
        match &mut parameters {
            PrimaryParameters::Triangle(p) => p.edge_gain = self.current,
            PrimaryParameters::Sine(p) => p.modulation_gain = self.current,
            PrimaryParameters::Cross(p) => p.modulation_gain = self.current,
            PrimaryParameters::CrossTriangle(p) => p.modulation_gain = self.current,
            PrimaryParameters::CrossSine(p) => p.modulation_gain = self.current,
            PrimaryParameters::Vpm(p) => p.modulation_gain = self.current,
            PrimaryParameters::VpmCarrier(p) => p.modulator.modulation_gain = self.current,
            PrimaryParameters::Noise(_) | PrimaryParameters::Formant(_) => unreachable!(),
            _ => {}
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
        if sync {
            // Shift the whole Unison bank together; preserve its phase spread.
            let base = oscillator.unison.phases[0].0;
            for phase in &mut oscillator.unison.phases {
                phase.0 = phase.0.wrapping_sub(base);
            }
            oscillator.phase = radias_synth_domain::Phase(0);
            oscillator.modulated_phase = radias_synth_domain::Phase(if program.selection == 3 {
                0xc000_0000
            } else {
                0
            });
            oscillator.control_feedback = 0;
        }
        oscillator.next_with_modulator(waveform, parameters, modulator)
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn cached_controls_keep_audio_histories_feedback_sync_and_slew() {
        let tables = [Tables::new(), Tables::new()];
        let waveform = WaveformTable {
            correction: [0; 129],
            shapers: crate::standalone_tables::shapers(),
        };
        for selection in [
            0, 1, 2, 3, 4, 5, 16, 17, 18, 19, 32, 33, 34, 35, 48, 49, 50, 51,
        ] {
            let mut cached = Controller::default();
            let mut uncached = Controller::default();
            let mut actual = PrimaryOscillator::default();
            let mut original = PrimaryOscillator::default();
            for frame in 0..4096u64 {
                let mut program = PrimaryProgram {
                    selection,
                    ..Default::default()
                };
                program.control.control1 = ((frame / 127 * 37) % 128) as u8;
                program.control.control2 = ((frame / 257 * 19) % 128) as u8;
                let increment = PhaseIncrement(0x128_9000 + (frame / 256) as u32 * 0x1_0000);
                let code = PitchCode::new(14000 + (frame / 256) as u16 * 21).unwrap();
                let lfo = (frame / 37 * 2131) as i16;
                let modulation = [
                    (frame / 31 * 3559) as i16,
                    ((frame / 43 * 217) % 255) as i16 - 127,
                ];
                let modulator = Sample((frame as i32).wrapping_mul(0x17ab35));
                let sync = frame % 131 == 130;
                let table = &tables[(frame / 512 % 2) as usize];
                // The oracle runs the same shared arithmetic with every cache
                // invalidated, so the input compilers execute on every sample.
                uncached.inputs = None;
                let a = cached.next_with_sync(
                    &mut actual,
                    program,
                    increment,
                    code,
                    lfo,
                    modulation,
                    modulator,
                    table,
                    &waveform,
                    frame,
                    2,
                    sync,
                );
                let b = uncached.next_with_sync(
                    &mut original,
                    program,
                    increment,
                    code,
                    lfo,
                    modulation,
                    modulator,
                    table,
                    &waveform,
                    frame,
                    2,
                    sync,
                );
                assert_eq!(a, b, "selection {selection}, frame {frame}");
                assert_eq!(actual, original, "generator history {selection}/{frame}");
                assert_eq!(cached.current, uncached.current);
                assert_eq!(cached.noise, uncached.noise);
                assert_eq!(cached.phase_code, uncached.phase_code);
            }
        }
    }
}
