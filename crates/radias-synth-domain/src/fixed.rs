//! Fixed-point products used by original DSP algorithms, without machine state.

#[inline]
pub fn saturate(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[inline]
pub fn high_product(left: i16, right: i16) -> i64 {
    if left == i16::MIN && right == i16::MIN {
        i32::MAX as i64
    } else {
        left as i64 * right as i64 * 2
    }
}

/// Exact equivalent of the original four-part Q31 product and both truncations.
#[inline]
pub fn multiply_q31(left: i32, right: i32) -> i64 {
    // floor((floor(x/2^16) + k)/2^16) == floor((x + k*2^16)/2^32).
    // Combining the partial products preserves both original floors. The
    // saturated high-half MIN16*MIN16 product needs a one-unit correction
    // throughout that entire pair of high halves, including nonzero low bits.
    let correction =
        (left >> 16 == i32::from(i16::MIN) && right >> 16 == i32::from(i16::MIN)) as i64;
    ((i64::from(left) * i64::from(right)) >> 31) - correction
}

#[inline]
pub fn multiply_q15(sample: i32, gain: i16) -> i64 {
    let low = (sample as u32 & 65535) as i64 * gain as i64 * 2;
    high_product((sample >> 16) as i16, gain) + (low >> 16)
}

/// Accumulate low products before the single final shift, as the original mixer.
#[inline]
pub fn weighted_sum<const N: usize>(samples: [i32; N], weights: [i16; N]) -> i64 {
    let mut low = 0i64;
    for (sample, gain) in samples.iter().zip(weights) {
        low += (*sample as u32 & 65535) as i64 * gain as i64 * 2;
    }
    let mut result = low >> 16;
    for (sample, gain) in samples.iter().zip(weights) {
        result += high_product((*sample >> 16) as i16, gain);
    }
    result
}
