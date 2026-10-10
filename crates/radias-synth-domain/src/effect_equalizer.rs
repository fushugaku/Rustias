//! Original SYS04Dxxx integer EQ coefficient preparation, without CPU emulation.
pub struct EffectEqualizerTables {
    pub frequency: [u16; 59],
    pub pole: [u32; 59],
    pub gain: [u32; 73],
    pub q: [u32; 96],
    pub curve_a: [[u32; 5]; 16],
    pub curve_b: [[u32; 4]; 32],
}
/// SYS04D172's unsigned product; current controller callers use shifts 0..=9.
pub(crate) fn multiply(a: u32, b: u32, shift: u32) -> u32 {
    ((u64::from(a) * u64::from(b)) >> (32 - shift)) as u32
}
fn signed_multiply(a: u32, b: u32, shift: u32) -> u32 {
    let negative = (a ^ b) >> 31 != 0;
    let a = if (a as i32) < 0 { a.wrapping_neg() } else { a };
    let b = if (b as i32) < 0 { b.wrapping_neg() } else { b };
    let result = multiply(a, b, shift);
    if negative {
        result.wrapping_neg()
    } else {
        result
    }
}
/// SYS04D140's exact 32-step restoring division, including carry and wrap.
fn divide(mut high: u32, mut low: u32, divisor: u32) -> u32 {
    for _ in 0..32 {
        let carry = high >> 31;
        high = high.wrapping_mul(2).wrapping_add(low >> 31);
        low = low.wrapping_mul(2);
        if carry != 0 || high >= divisor {
            high = high.wrapping_sub(divisor);
            low = low.wrapping_add(1);
        }
    }
    low
}
fn signed_divide_q28(numerator: u32, denominator: u32) -> u32 {
    let mut high = ((numerator as i32) >> 4) as u32;
    let mut low = numerator << 28;
    let negative = (high ^ denominator) >> 31 != 0;
    if (high as i32) < 0 {
        high = high.wrapping_neg().wrapping_sub(u32::from(low != 0));
        low = low.wrapping_neg();
    }
    let denominator = if (denominator as i32) < 0 {
        denominator.wrapping_neg()
    } else {
        denominator
    };
    let result = divide(high, low, denominator);
    if negative {
        result.wrapping_neg()
    } else {
        result
    }
}
fn sine_polynomial(value: u32) -> u32 {
    let square = multiply(value, value, 0);
    let first = (square >> 16).wrapping_mul(0x038e);
    let value2 = multiply(!first, square, 0);
    let value2 = multiply(value2, 0x06186186, 0);
    let value2 = multiply(!value2, square, 0);
    let value2 = multiply(value2, 0x0ccccccd, 0);
    let value2 = multiply(!value2, square, 0);
    let value2 = multiply(value2, 0x2aaaaaab, 0);
    multiply(value, !value2, 0)
}
fn cosine_polynomial(value: u32) -> u32 {
    let square = multiply(value, value, 0);
    let first = (square >> 16).wrapping_mul(0x0492);
    let value = multiply(!first, square, 0);
    let value = multiply(value, 0x08888888, 0);
    let value = multiply(!value, square, 0);
    let value = multiply(value, 0x15555555, 0);
    !(multiply(!value, square, 0) >> 1)
}
fn cosine(phase: u32) -> u32 {
    if phase >= 0x80000000 {
        return !cosine(!phase);
    }
    if phase > 0x389fd980 {
        sine_polynomial(multiply(0x80000000u32.wrapping_sub(phase), 0xc90fdaa2, 2)) >> 1
    } else {
        cosine_polynomial(multiply(phase, 0xc90fdaa2, 2)) >> 1
    }
}
impl EffectEqualizerTables {
    fn curve_a(&self, value: u32) -> Option<u32> {
        let record = self.curve_a.get((value >> 27) as usize)?;
        let fraction = value & 0x07ffffff;
        let delta = ((record[4] >> 16).wrapping_mul(fraction >> 11)) >> 4;
        let value = multiply(record[3].wrapping_add(delta), fraction, 1).wrapping_add(record[2]);
        let value = multiply(value, fraction, 1).wrapping_add(record[1]);
        Some(multiply(value, fraction, 2).wrapping_add(record[0]))
    }
    fn curve_b(&self, value: u32) -> Option<u32> {
        let index = (value >> 27) as usize;
        let record = self.curve_b.get(index)?;
        let fraction = value & 0x07ffffff;
        let delta = ((record[3] >> 16).wrapping_mul(fraction >> 11)) >> 7;
        let coefficient = if index < 18 {
            record[2].wrapping_add(delta)
        } else {
            record[2].wrapping_sub(delta)
        };
        let value = record[1].wrapping_sub(multiply(coefficient, fraction, 0));
        Some(multiply(value, fraction, 0).wrapping_add(record[0]))
    }
    fn alpha(&self, phase: u32, q: u8) -> Option<u32> {
        let scaled = multiply(phase, *self.q.get(usize::from(q))?, 0);
        let value = if phase < 0x80000000 {
            let first = self.curve_a(phase)?;
            let second = self.curve_a(scaled)?;
            let ratio = divide(first >> 4, first << 28, second);
            let product = multiply(ratio, first, 0);
            if product < 0x10000000 {
                self.curve_b(product << 4)?
            } else {
                self.curve_b(divide(0x10000000, 0, product))?.wrapping_neg()
            }
        } else {
            let value = self.curve_a(!phase)?;
            let value = multiply(value, value, 0);
            let value = if scaled < 0x80000000 {
                multiply(self.curve_a(scaled)?, value, 0)
            } else {
                divide(value, 0, self.curve_a(!scaled)?)
            };
            self.curve_b(value)?.wrapping_neg()
        };
        let value = value.wrapping_sub(scaled);
        Some(if value < 0x80000000 {
            self.curve_a(value)? >> 3
        } else {
            divide(0x20000000, 0, self.curve_a(!value)?)
        })
    }
    /// Whole SYS04D5F0: five coefficient words and three scaling exponents.
    pub fn peaking(&self, frequency: u8, q: u8, gain: i8) -> Option<[u32; 8]> {
        if frequency >= 59 || q >= 96 || !(-36..=36).contains(&gain) {
            return None;
        }
        if gain == 0 {
            return Some([0x10000000, 0, 0, 0, 0, 3, 3, 3]);
        }
        let phase = multiply(
            u32::from(self.frequency[usize::from(frequency)]) << 16,
            0xaec33e1f,
            2,
        );
        let cosine = cosine(phase);
        let alpha = self.alpha(phase, q)?;
        let gain_word = self.gain[(i32::from(gain) + 36) as usize];
        let (first, third, first_scale, third_scale, inverse) = if gain >= 0 {
            let inverse = if alpha < 0xe0000000 {
                divide(0x20000000, 0, alpha.wrapping_add(0x20000000))
            } else {
                divide(0x10000000, 0, (alpha >> 1).wrapping_add(0x10000000))
            };
            let first = signed_multiply(0x10000000u32.wrapping_sub(gain_word), inverse >> 1, 1)
                .wrapping_add(gain_word);
            let third =
                multiply(gain_word.wrapping_add(0x10000000), inverse, 0).wrapping_sub(gain_word);
            (first, third, 3, 3, inverse)
        } else {
            let gain = gain_word << 1;
            let inverse = if alpha < u32::MAX.wrapping_sub(gain) {
                divide(gain, 0, gain.wrapping_add(alpha))
            } else {
                divide(gain >> 1, 0, (gain >> 1).wrapping_add(alpha >> 1))
            };
            let first = if alpha < 0xe0000000 {
                multiply(alpha.wrapping_add(0x20000000), inverse, 0)
            } else {
                multiply((alpha >> 1).wrapping_add(0x10000000), inverse, 0) << 1
            };
            let third = if alpha <= 0x20000000 {
                signed_multiply(0x20000000u32.wrapping_sub(alpha), inverse >> 1, 0)
            } else {
                signed_multiply(0x10000000u32.wrapping_sub(alpha >> 1), inverse, 0)
            };
            (first, third, 2, 3, inverse)
        };
        let second = if (cosine as i32) >= 0 {
            multiply(cosine, inverse, 0).wrapping_neg()
        } else {
            multiply(cosine.wrapping_neg(), inverse, 0)
        };
        Some([
            first,
            second,
            third,
            second.wrapping_neg(),
            0x80000000u32.wrapping_sub(inverse),
            first_scale,
            1,
            third_scale,
        ])
    }
    /// Whole SYS04D7AA.
    pub fn low_shelf(&self, frequency: u8, gain: i8) -> Option<[u32; 3]> {
        if !(-36..=36).contains(&gain) {
            return None;
        }
        let pole = *self.pole.get(usize::from(frequency))?;
        if gain == 0 {
            return Some([0x3fffff, 0, 0]);
        }
        let minus = pole.wrapping_sub(0x10000000);
        let plus = pole.wrapping_add(0x10000000);
        let (first, second, feedback) = if gain > 0 {
            let gain = self.gain[(36 - i32::from(gain)) as usize];
            let product = signed_multiply(gain, minus, 4);
            (
                signed_divide_q28(plus.wrapping_sub(product), gain << 2),
                signed_divide_q28(product.wrapping_add(plus), gain << 2),
                (pole as i32).wrapping_neg().wrapping_div(2) as u32,
            )
        } else {
            let gain = self.gain[(36 + i32::from(gain)) as usize];
            let product = signed_multiply(gain, minus, 4);
            let divisor = plus.wrapping_sub(product);
            (
                signed_divide_q28(gain, divisor),
                signed_divide_q28(signed_multiply(gain, pole, 4), divisor),
                (signed_divide_q28(product.wrapping_add(plus), divisor) as i32)
                    .wrapping_neg()
                    .wrapping_div(2) as u32,
            )
        };
        Some([first, second, feedback].map(|v| (v.wrapping_add(16) as i32 >> 5) as u32))
    }
    /// Whole SYS04D250.
    pub fn high_shelf(&self, frequency: u8, gain: i8) -> Option<[u32; 3]> {
        if !(-36..=36).contains(&gain) {
            return None;
        }
        let pole = *self.pole.get(usize::from(frequency))?;
        if gain == 0 {
            return Some([0xfffff, 0, 0]);
        }
        let plus = pole.wrapping_add(0x10000000);
        let minus = pole.wrapping_sub(0x10000000);
        let difference = 0x10000000u32.wrapping_sub(pole);
        let (first, second, feedback) = if gain > 0 {
            let gain = self.gain[(36 - i32::from(gain)) as usize];
            let product = signed_multiply(gain, plus, 4);
            (
                signed_divide_q28(product.wrapping_add(difference), gain << 2),
                signed_divide_q28(product.wrapping_add(minus), gain << 2),
                (pole as i32).wrapping_neg().wrapping_div(8) as u32,
            )
        } else {
            let gain = self.gain[(36 + i32::from(gain)) as usize];
            let product = signed_multiply(gain, plus, 4);
            let divisor = product.wrapping_add(difference);
            (
                signed_divide_q28(gain, divisor),
                signed_divide_q28(signed_multiply(gain, pole, 4), divisor),
                (signed_divide_q28(product.wrapping_add(minus), divisor) as i32)
                    .wrapping_neg()
                    .wrapping_div(8) as u32,
            )
        };
        Some([
            (first.wrapping_add(64) as i32 >> 7) as u32,
            (second.wrapping_add(64) as i32 >> 7) as u32,
            (feedback.wrapping_add(16) as i32 >> 5) as u32,
        ])
    }
}
