use super::{
    State,
    filters::{Model, soft},
    motion::VoiceMotion,
    osc3, primary,
};
use crate::standalone::{Values, default_values};
use radias_synth_application::{
    motion::Program,
    primary::{PrimaryProgram, ThirdProgram},
};
use radias_synth_domain::{
    Sample,
    controller_mixer::{MixerLevel, MixerScales},
    controller_primary::PrimaryControl,
    controller_secondary::{FineTuneTable, SecondaryPitch},
    modulation::AppliedModulationTargets,
    voice::VoiceParameters,
    waveform::WaveformTable,
};
use std::sync::Arc;

pub(super) struct Expanded {
    pub values: Values,
    pub fourth: State,
    pub models: [Model; 2],
    pub motion: VoiceMotion,
    pub previous_third_phase: u32,
    pub tempo: Option<f64>,
    scales: MixerScales,
    dc_input: f64,
    dc_output: f64,
    fold_output: f64,
    envelopes_modified: bool,
    gain_current: Option<f64>,
    fold_depth: Option<f64>,
    fold_bias: Option<f64>,
}
impl Default for Expanded {
    fn default() -> Self {
        static DEFAULTS: std::sync::OnceLock<Values> = std::sync::OnceLock::new();
        Self {
            values: *DEFAULTS.get_or_init(default_values),
            fourth: State {
                primary: Some(Box::default()),
                ..Default::default()
            },
            models: Default::default(),
            motion: Default::default(),
            scales: crate::standalone_tables::mixer(),
            dc_input: 0.0,
            dc_output: 0.0,
            fold_output: 0.0,
            previous_third_phase: 0,
            envelopes_modified: false,
            gain_current: None,
            fold_depth: None,
            fold_bias: None,
            tempo: None,
        }
    }
}
impl Expanded {
    pub fn fresh(&self) -> Self {
        let mut next = Self::default();
        next.values = self.values;
        next.tempo = self.tempo;
        next.motion.configure(self.motion.program.clone());
        next
    }
    pub fn inherit(&mut self, other: &Self) {
        self.values = other.values;
        self.tempo = other.tempo;
        self.motion.configure(other.motion.program.clone());
    }
    pub fn parameters(&mut self, values: &[i32]) {
        if values.len() == self.values.len() {
            self.values.copy_from_slice(values);
        }
    }
    pub fn motion(&mut self, program: Arc<Program>) {
        self.motion.configure(program);
    }
    pub fn envelopes(
        &mut self,
    ) -> Option<[radias_synth_application::voice_envelopes::ModEnvelopeProgram; 3]> {
        self.envelopes_modified |= self
            .motion
            .program
            .lanes
            .iter()
            .any(|l| matches!(l.parameter,3..=6|32..=51));
        if !self.envelopes_modified {
            return None;
        }
        let get = |id: usize| {
            (self.values[id] as f64 + self.motion.offsets[id])
                .round()
                .clamp(0.0, 127.0) as u8
        };
        Some(core::array::from_fn(|i| {
            let (a, b) = match i {
                0 => (32, 40),
                1 => (3, 44),
                _ => (36, 48),
            };
            radias_synth_application::voice_envelopes::ModEnvelopeProgram {
                adsr: core::array::from_fn(|i| get(a + i)),
                curve: get(b).min(7),
                velocity_level_sensitivity: get(b + 1),
                velocity_time_sensitivity: get(b + 2),
                key_tracking: get(b + 3),
            }
        }))
    }
    pub fn enabled(&self) -> bool {
        self.values[187] != 0
            || self.gain_current.is_some_and(|g| g >= 0.5)
            || self.values[189] != 0
            || self.values[190] != 0
            || self
                .motion
                .program
                .lanes
                .iter()
                .any(|lane| lane.parameter >= 179)
    }
    pub fn value(&self, id: usize, targets: AppliedModulationTargets) -> f64 {
        let dest = match id {
            179 => 55,
            180 => 56,
            181 => 53,
            182 => 54,
            183 => 57,
            184 => 58,
            185 => 59,
            186 => 60,
            187 => 61,
            188 => 62,
            189 => 63,
            190 => 64,
            191 => 65,
            192 => 66,
            193 => 67,
            194 => 68,
            _ => 0,
        };
        let patch = if dest == 0 {
            0.0
        } else {
            targets.controls[dest - 2] as f64 / if matches!(id, 181 | 187) { 128.0 } else { 1.0 }
        };
        self.values[id] as f64 + self.motion.offsets[id] + patch
    }
    pub fn gain(&self, targets: AppliedModulationTargets) -> i16 {
        let wave = self.value(179, targets).round().clamp(0.0, 5.0) as u8;
        let mode = self.value(180, targets).round().clamp(0.0, 4.0) as u8;
        MixerLevel {
            level: self.value(187, targets).round().clamp(0.0, 127.0) as u8,
            manual_offset: 0,
            modulation: 0,
            scale: self.scales.primary(if mode == 4 {
                3
            } else {
                wave | ((if wave >= 4 { 0 } else { mode }) << 4)
            }),
        }
        .gain()
    }
    pub fn next_gain(&mut self, targets: AppliedModulationTargets) -> i16 {
        let target = self.gain(targets) as f64;
        let current = self.gain_current.get_or_insert(target);
        *current += (target - *current) * 0.02;
        if (*current - target).abs() < 0.5 {
            *current = target;
        }
        current.round() as i16
    }
    #[allow(clippy::too_many_arguments)]
    pub fn oscillator(
        &mut self,
        targets: AppliedModulationTargets,
        p: VoiceParameters,
        reference: Sample,
        wrap: bool,
        modulator: Sample,
        cv: [f64; 5],
        tables: &primary::Tables,
        fine: &FineTuneTable,
        table: &WaveformTable,
        frame: u64,
    ) -> Sample {
        let get = |id| self.value(id, targets).round().clamp(0.0, 127.0) as u8;
        let wave = get(179).min(5);
        let mode = get(180).min(4);
        let raw_wave = self.values[179] as u8;
        let raw_mode = self.values[180] as u8;
        let program = ThirdProgram {
            primary: PrimaryProgram {
                selection: raw_wave | ((if raw_wave >= 4 { 0 } else { raw_mode.min(3) }) << 4),
                control: PrimaryControl {
                    control1: self.values[181] as u8,
                    control2: self.values[182] as u8,
                    ..Default::default()
                },
            },
            pitch: SecondaryPitch {
                semitone: self.values[183] as u8,
                fine_tune: self.values[184] as u8,
                ..Default::default()
            },
            interaction: self.values[185] as u8,
            reference: self.values[186] as u8,
        };
        // Pitch/controls for the classic generator use the same recovered law as OSC 3.
        if mode != 4 {
            let mut t = targets;
            for (id, dest, scale) in [
                (179, 55, 1.0),
                (180, 56, 1.0),
                (181, 53, 128.0),
                (182, 54, 1.0),
                (183, 57, 1.0),
                (184, 58, 1.0),
                (185, 59, 1.0),
                (186, 60, 1.0),
            ] {
                t.controls[dest - 2] = (t.controls[dest - 2] as f64
                    + self.motion.offsets[id] * scale)
                    .round()
                    .clamp(-32767.0, 32767.0) as i16;
            }
            return osc3::next(
                &mut self.fourth.osc,
                self.fourth.primary.as_mut().unwrap(),
                &mut self.fourth.third_pitch,
                program,
                t,
                p,
                cv,
                modulator,
                reference,
                wrap,
                tables,
                fine,
                table,
                frame,
                62,
                3,
            );
        }
        let mut pitch = program.pitch;
        pitch.semitone = get(183);
        pitch.fine_tune = get(184);
        pitch.virtual_patch_q16 = targets.oscillator_pitch_q16[3];
        let relative = pitch.relative_code(fine) as f64 + cv[3] * 24.0 * 256.0;
        let increment = (p.primary.base_increment().0 as f64 / 4294967296.0
            * 2.0_f64.powf(relative / (12.0 * 256.0)))
        .clamp(0.0, 0.45);
        let interaction = get(185).min(3);
        let shape = get(188).min(4);
        if interaction & 2 != 0 && wrap {
            self.fourth.phase = 0.0;
        }
        let target_depth = (self.value(181, targets) / 127.0 + cv[1]).clamp(0.0, 1.0);
        let target_bias = ((self.value(182, targets) - 64.0) / 64.0 + cv[2]).clamp(-1.0, 1.0);
        let depth = self.fold_depth.get_or_insert(target_depth);
        *depth += (target_depth - *depth) * 0.02;
        let depth = *depth;
        let bias = self.fold_bias.get_or_insert(target_bias);
        *bias += (target_bias - *bias) * 0.02;
        let bias = *bias;
        let mut sum = 0.0;
        for _ in 0..4 {
            self.fourth.phase = (self.fourth.phase + increment * 0.25).fract();
            let phase = self.fourth.phase;
            let x = match wave {
                0 => phase * 2.0 - 1.0,
                1 => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                2 => 1.0 - 4.0 * (phase - 0.5).abs(),
                _ => (phase * std::f64::consts::TAU).sin(),
            };
            let driven = x * (1.0 + depth * 11.0) + bias * 2.0;
            let folded = match shape {
                1 => (driven * std::f64::consts::FRAC_PI_2).sin(),
                2 => {
                    soft((1.0 - 4.0 * ((driven + 1.0) * 0.25).rem_euclid(1.0).sub(0.5).abs()) * 1.5)
                }
                3 => {
                    let v = driven + depth;
                    1.0 - 4.0 * ((v + 1.0) * 0.25).rem_euclid(1.0).sub(0.5).abs()
                }
                4 => driven.abs().rem_euclid(2.0) - 1.0,
                _ => 1.0 - 4.0 * ((driven + 1.0) * 0.25).rem_euclid(1.0).sub(0.5).abs(),
            };
            self.fold_output += (folded - self.fold_output) * 0.6;
            sum += self.fold_output * 0.25;
        }
        let dc = sum - self.dc_input + 0.9995 * self.dc_output;
        self.dc_input = sum;
        self.dc_output = dc;
        let sample = if interaction & 1 != 0 {
            dc * reference.0 as f64 / 2147483648.0
        } else {
            dc
        };
        Sample((sample.clamp(-1.0, 1.0) * 2147483647.0) as i32)
    }
}
use std::ops::Sub;
