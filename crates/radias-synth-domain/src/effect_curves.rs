//! Original SH parameter-to-coefficient curves. These are controller integer
//! operations, separate from the FXD03 audio accumulator and multiplier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectParameterRange {
    pub minimum: i16,
    pub maximum: i16,
    pub encoded_zero: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectCurve {
    Linear,
    Quadratic,
    EaseOut,
    InverseScale,
    Select,
    OffsetScale,
    OffsetQuadratic,
}
impl EffectParameterRange {
    pub fn compile(self, curve: EffectCurve, value: i32, first: i32, second: i32) -> Option<i32> {
        let width = if matches!(
            curve,
            EffectCurve::OffsetScale | EffectCurve::OffsetQuadratic
        ) {
            self.maximum as u8
        } else {
            i32::from(self.maximum).wrapping_sub(i32::from(self.minimum)) as u8
        };
        if width == 0 && curve != EffectCurve::Select {
            return None;
        }
        let span = i32::from(width);
        let offset = value.wrapping_sub(i32::from(self.minimum));
        let product_divide = |a: i32, b: i32| a.wrapping_mul(b).wrapping_div(span);
        let (start, distance, position) = if first >= second {
            (second, first.wrapping_sub(second), offset)
        } else {
            (first, second.wrapping_sub(first), span.wrapping_sub(offset))
        };
        Some(match curve {
            EffectCurve::Linear => start.wrapping_add(product_divide(distance, position)),
            EffectCurve::Quadratic => {
                let weight = if value < 0 {
                    position.wrapping_neg()
                } else {
                    position
                } as u8;
                let initial = product_divide(distance, position);
                start.wrapping_add(product_divide(initial, i32::from(weight)))
            }
            EffectCurve::EaseOut => {
                let signed = if value < 0 {
                    position.wrapping_neg()
                } else {
                    position
                };
                let weight = span.wrapping_mul(2).wrapping_sub(signed);
                let initial = product_divide(distance, position);
                start.wrapping_add(product_divide(initial, weight))
            }
            EffectCurve::InverseScale => product_divide(first, span.wrapping_sub(offset)),
            EffectCurve::Select => {
                if offset == 0 {
                    second
                } else {
                    first
                }
            }
            EffectCurve::OffsetScale => {
                let position = value.wrapping_sub(i32::from(self.encoded_zero));
                product_divide(first, position)
            }
            EffectCurve::OffsetQuadratic => {
                let position = value.wrapping_sub(i32::from(self.encoded_zero));
                let weight = if position < 0 {
                    position.wrapping_neg()
                } else {
                    position
                } as u8;
                let initial = product_divide(first, position);
                product_divide(initial, i32::from(weight))
            }
        })
    }
}
