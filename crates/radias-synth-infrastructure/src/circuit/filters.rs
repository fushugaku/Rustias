//! Nonlinear, oversampled filter families for the expanded instrument.
//! These are Rustias models, not measured replicas of particular hardware.
use std::cell::RefCell;

#[derive(Clone, Copy)]
struct Prewarped {
    coefficient: i32,
    cutoff: f64,
    gain: f64,
}

thread_local! {
    // Pure coefficient results only: no voice, instance or audio history. A
    // fixed-size, allocation-free cache shares identical cutoff calculations
    // across polyphonic voices without locks or cross-thread synchronization.
    static PREWARPED: RefCell<[Option<Prewarped>; 256]> = const { RefCell::new([None; 256]) };
}

fn prewarped(coefficient: i32) -> Prewarped {
    PREWARPED.with_borrow_mut(|cache| {
        let index = (coefficient as u32).wrapping_mul(0x9e37_79b1) >> 24;
        let slot = &mut cache[index as usize];
        if let Some(value) = *slot
            && value.coefficient == coefficient
        {
            return value;
        }
        let cutoff = frequency(coefficient);
        let value = Prewarped {
            coefficient,
            cutoff,
            gain: (std::f64::consts::PI * cutoff / 96_000.0).tan(),
        };
        *slot = Some(value);
        value
    })
}

#[derive(Default)]
pub struct Model {
    poles: [f64; 4],
    svf: [f64; 4],
    previous: f64,
    cutoff: f64,
    resonance: f64,
    drive: f64,
    morph: f64,
    key: Option<(u8, u64, u64, u64, u64)>,
    gain: f64,
    output: f64,
    mode: u8,
    coefficient: i32,
    ladder_gain: f64,
    feedback: f64,
    stage_drive: f64,
    output_gain: f64,
    damping: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    morph_index: usize,
    morph_fraction: f64,
}
pub fn frequency(gain: i32) -> f64 {
    ((gain as f64 / 2147483648.0).clamp(0.0001, 0.99).asin() * 48_000.0 / std::f64::consts::PI)
        .clamp(12.0, 20_000.0)
}
pub(super) fn soft(x: f64) -> f64 {
    // At either clamp endpoint the original rational expression is exactly
    // +/-1. Avoid its division for saturated inputs without approximating it.
    if x >= 3.0 {
        return 1.0;
    }
    if x <= -3.0 {
        return -1.0;
    }
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}
impl Model {
    pub fn sample_coefficient(
        &mut self,
        input: f64,
        mode: u8,
        coefficient: i32,
        resonance: f64,
        drive: f64,
        morph: f64,
    ) -> f64 {
        let (hz, gain) = if self.coefficient == coefficient {
            (self.cutoff, self.key.map(|_| self.gain))
        } else {
            self.coefficient = coefficient;
            let compiled = prewarped(coefficient);
            (compiled.cutoff, Some(compiled.gain))
        };
        self.sample_with_gain(input, mode, hz, resonance, drive, morph, gain)
    }
    pub fn sample(
        &mut self,
        input: f64,
        mode: u8,
        cutoff: f64,
        resonance: f64,
        drive: f64,
        morph: f64,
    ) -> f64 {
        self.sample_with_gain(input, mode, cutoff, resonance, drive, morph, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_with_gain(
        &mut self,
        input: f64,
        mode: u8,
        cutoff: f64,
        resonance: f64,
        drive: f64,
        morph: f64,
        gain: Option<f64>,
    ) -> f64 {
        let key = (
            mode,
            cutoff.to_bits(),
            resonance.to_bits(),
            drive.to_bits(),
            morph.to_bits(),
        );
        if self.key != Some(key) {
            let cutoff = cutoff.clamp(12.0, 20_000.0);
            let resonance = resonance.clamp(0.0, 1.0);
            let cutoff_changed = self.key.is_none() || self.cutoff.to_bits() != cutoff.to_bits();
            let coefficients_changed = cutoff_changed
                || self.resonance.to_bits() != resonance.to_bits()
                || self.mode != mode;
            // Resonance, Drive and Morph modulation must not repeat tan() when
            // cutoff is unchanged. Keep the original f64 expression and rate.
            if cutoff_changed {
                self.gain =
                    gain.unwrap_or_else(|| (std::f64::consts::PI * cutoff / 96_000.0).tan());
            }
            self.key = Some(key);
            self.cutoff = cutoff;
            self.resonance = resonance;
            self.drive = 1.0 + drive.clamp(0.0, 1.0) * 11.0;
            self.morph = morph.clamp(0.0, 1.0);
            self.mode = mode;
            // Cache only parameter arithmetic, never signal history. Preserve
            // multiplication/division order so the rendered PCM remains exact.
            if coefficients_changed {
                if matches!(mode, 1 | 4 | 7 | 8 | 9) {
                    self.ladder_gain = self.gain / (1.0 + self.gain);
                    self.feedback = self.resonance * if mode == 4 { 4.1 } else { 3.95 };
                    self.stage_drive = 1.0 + self.resonance * 0.5;
                    self.output_gain = 1.0 + self.feedback * 0.2;
                } else {
                    self.damping = (2.0 - self.resonance * 1.98).max(0.02);
                    self.a1 = 1.0 / (1.0 + self.gain * (self.gain + self.damping));
                    self.a2 = self.gain * self.a1;
                    self.a3 = self.gain * self.gain * self.a1;
                }
            }
            if mode == 6 {
                let position = self.morph * 3.0;
                self.morph_index = (position as usize).min(2);
                self.morph_fraction = position - self.morph_index as f64;
            }
        }
        // Select the topology once per output sample. Const-specialized kernels
        // remove model tests from both oversampling passes and all four poles.
        let result = match self.mode {
            1 => self.render::<1>(input),
            2 => self.render::<2>(input),
            3 => self.render::<3>(input),
            4 => self.render::<4>(input),
            5 => self.render::<5>(input),
            6 => self.render::<6>(input),
            7 => self.render::<7>(input),
            8 => self.render::<8>(input),
            9 => self.render::<9>(input),
            10 => self.render::<10>(input),
            11 => self.render::<11>(input),
            12 => self.render::<12>(input),
            13 => self.render::<13>(input),
            14 => self.render::<14>(input),
            15 => self.render::<15>(input),
            mode if mode >= 16 && mode % 2 == 0 => self.render::<16>(input),
            mode if mode >= 16 => self.render::<17>(input),
            _ => self.render::<0>(input),
        };
        self.previous = input;
        // A short output smoother also de-clicks a live model change.
        self.output += (result.clamp(-4.0, 4.0) - self.output) * 0.65;
        self.output
    }

    fn render<const MODE: u8>(&mut self, input: f64) -> f64 {
        let mut result = 0.0;
        for x in [(self.previous + input) * 0.5, input] {
            let x = if MODE >= 10 && self.drive <= 1.0 {
                x
            } else {
                soft(x * self.drive)
            };
            let y = if matches!(MODE, 1 | 4 | 7 | 8 | 9) {
                let g = self.ladder_gain;
                let feedback = self.feedback;
                let mut y = soft(x - self.poles[3] * feedback);
                let mut taps = [0.0; 4];
                for (i, z) in self.poles.iter_mut().enumerate() {
                    let v = (y - *z) * g;
                    let p = v + *z;
                    *z = (p + v).clamp(-4.0, 4.0);
                    y = if MODE == 4 {
                        soft(p * 1.15) / 1.15
                    } else if MODE == 9 {
                        soft(p * self.stage_drive) / self.stage_drive
                    } else {
                        p
                    };
                    taps[i] = y;
                }
                match MODE {
                    7 => x - 4.0 * taps[0] + 6.0 * taps[1] - 4.0 * taps[2] + taps[3],
                    8 => 4.0 * (taps[1] - 2.0 * taps[2] + taps[3]),
                    _ => y * self.output_gain,
                }
            } else {
                let k = self.damping;
                let a1 = self.a1;
                if MODE >= 10 {
                    let mut output = x;
                    for stage in 0..if MODE % 2 == 1 { 2 } else { 1 } {
                        let i = stage * 2;
                        let v3 = output - self.svf[i + 1];
                        let band = a1 * self.svf[i] + self.a2 * v3;
                        let low = self.svf[i + 1] + self.a2 * self.svf[i] + self.a3 * v3;
                        self.svf[i] = 2.0 * band - self.svf[i];
                        self.svf[i + 1] = 2.0 * low - self.svf[i + 1];
                        let high = output - k * band - low;
                        output = match (MODE - 10) / 2 {
                            0 => low,
                            1 => high,
                            2 => band * k,
                            _ => low + high,
                        };
                    }
                    result += output * 0.5;
                    continue;
                }
                let v3 = x - self.svf[1];
                let band = a1 * self.svf[0] + self.a2 * v3;
                let low = self.svf[1] + self.a2 * self.svf[0] + self.a3 * v3;
                self.svf[0] = soft(2.0 * band - self.svf[0]);
                self.svf[1] = soft(2.0 * low - self.svf[1]);
                let high = x - k * band - low;
                match MODE {
                    2 => soft((low + band * self.resonance * 0.35) * 1.3) / 1.3,
                    3 => soft(high * 1.3) / 1.3,
                    5 => {
                        if self.morph < 0.5 {
                            low + high * self.morph * 2.0
                        } else {
                            low * (2.0 - self.morph * 2.0) + high
                        }
                    }
                    6 => {
                        let taps = [low, band, high, low + high];
                        let i = self.morph_index;
                        taps[i] + (taps[i + 1] - taps[i]) * self.morph_fraction
                    }
                    _ => low,
                }
            };
            result += y * 0.5;
        }
        result
    }
}
