//! Original per-sample amplifier envelope smoothing A1C8..A1D5.
use crate::fixed::{high_product, saturate};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvelopeLevel(pub i16);

impl EnvelopeLevel {
    /// Target/rate are the compiled segment values; no assumed ADSR time curve.
    pub fn step(&mut self, target: i16, coefficient: i16) -> i16 {
        let raised =
            saturate(((self.0 as i64) << 16) + high_product(target, coefficient) as i32 as i64);
        let reduced = saturate(raised as i64 - high_product(self.0, coefficient) as i32 as i64);
        self.0 = (reduced >> 16) as i16;
        self.0
    }
}
