//! Nonlinear, oversampled filter families for the expanded instrument.
//! These are Rustias models, not measured replicas of particular hardware.
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
}
pub fn frequency(gain: i32) -> f64 {
    ((gain as f64 / 2147483648.0).clamp(0.0001, 0.99).asin() * 48_000.0 / std::f64::consts::PI)
        .clamp(12.0, 20_000.0)
}
pub(super) fn soft(x: f64) -> f64 {
    let x = x.clamp(-3.0, 3.0);
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
        let hz = if self.coefficient == coefficient {
            self.cutoff
        } else {
            self.coefficient = coefficient;
            frequency(coefficient)
        };
        self.sample(input, mode, hz, resonance, drive, morph)
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
        let key = (
            mode,
            cutoff.to_bits(),
            resonance.to_bits(),
            drive.to_bits(),
            morph.to_bits(),
        );
        if self.key != Some(key) {
            self.key = Some(key);
            self.cutoff = cutoff.clamp(12.0, 20_000.0);
            self.resonance = resonance.clamp(0.0, 1.0);
            self.drive = 1.0 + drive.clamp(0.0, 1.0) * 11.0;
            self.morph = morph.clamp(0.0, 1.0);
            self.gain = (std::f64::consts::PI * self.cutoff / 96_000.0).tan();
            self.mode = mode;
        }
        let mut result = 0.0;
        for x in [(self.previous + input) * 0.5, input] {
            let x = if self.mode >= 10 && self.drive <= 1.0 {
                x
            } else {
                soft(x * self.drive)
            };
            let y = if matches!(self.mode, 1 | 4 | 7 | 8 | 9) {
                let g = self.gain / (1.0 + self.gain);
                let feedback = self.resonance * if self.mode == 4 { 4.1 } else { 3.95 };
                let mut y = soft(x - self.poles[3] * feedback);
                let mut taps = [0.0; 4];
                for (i, z) in self.poles.iter_mut().enumerate() {
                    let v = (y - *z) * g;
                    let p = v + *z;
                    *z = (p + v).clamp(-4.0, 4.0);
                    y = if self.mode == 4 {
                        soft(p * 1.15) / 1.15
                    } else if self.mode == 9 {
                        soft(p * (1.0 + self.resonance * 0.5)) / (1.0 + self.resonance * 0.5)
                    } else {
                        p
                    };
                    taps[i] = y;
                }
                match self.mode {
                    7 => x - 4.0 * taps[0] + 6.0 * taps[1] - 4.0 * taps[2] + taps[3],
                    8 => 4.0 * (taps[1] - 2.0 * taps[2] + taps[3]),
                    _ => y * (1.0 + feedback * 0.2),
                }
            } else {
                let g = self.gain;
                let k = (2.0 - self.resonance * 1.98).max(0.02);
                let a1 = 1.0 / (1.0 + g * (g + k));
                if self.mode >= 10 {
                    let mut output = x;
                    for stage in 0..if self.mode % 2 == 1 { 2 } else { 1 } {
                        let i = stage * 2;
                        let v3 = output - self.svf[i + 1];
                        let band = a1 * self.svf[i] + g * a1 * v3;
                        let low = self.svf[i + 1] + g * a1 * self.svf[i] + g * g * a1 * v3;
                        self.svf[i] = 2.0 * band - self.svf[i];
                        self.svf[i + 1] = 2.0 * low - self.svf[i + 1];
                        let high = output - k * band - low;
                        output = match (self.mode - 10) / 2 {
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
                let band = a1 * self.svf[0] + g * a1 * v3;
                let low = self.svf[1] + g * a1 * self.svf[0] + g * g * a1 * v3;
                self.svf[0] = soft(2.0 * band - self.svf[0]);
                self.svf[1] = soft(2.0 * low - self.svf[1]);
                let high = x - k * band - low;
                match self.mode {
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
                        let p = self.morph * 3.0;
                        let taps = [low, band, high, low + high];
                        let i = (p as usize).min(2);
                        taps[i] + (taps[i + 1] - taps[i]) * (p - i as f64)
                    }
                    _ => low,
                }
            };
            result += y * 0.5;
        }
        self.previous = input;
        // A short output smoother also de-clicks a live model change.
        self.output += (result.clamp(-4.0, 4.0) - self.output) * 0.65;
        self.output
    }
}
