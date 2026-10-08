// Generated from reference SHA256 ff228f02739431f86987b5913b867a649db584fb964a977005244b2a2290b502
use super::{C55, MASK40, signed40};
impl C55 {
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn condition(&self, mut c: u8) -> bool {
        {
            let _arg1 = (c & (127i32 as u8));
            c = _arg1;
            _arg1
        };
        let mut comparison: u32 = ((c as i32).wrapping_shr(4i32 as u32) as u32);
        let mut n: u32 = (((c as i32) & 15i32) as u32);
        let mut v: i64 = (if (n < (4i32 as u32)) {
            (if (((self.st[(1i32 as u64) as usize] as i32) & 1056i32) != 0) {
                signed40({
                    let _arg2 = n;
                    self.reg(_arg2)
                })
            } else {
                (({
                    let _arg3 = n;
                    self.reg(_arg3)
                } as i32) as i64)
            })
        } else {
            (({
                let _arg4 = n;
                self.reg(_arg4)
            } as i16) as i64)
        });
        'switch1: {
            match comparison {
                0 => {
                    return (v == (0i32 as i64));
                }
                1 => {
                    return (v != (0i32 as i64));
                }
                2 => {
                    return (v < (0i32 as i64));
                }
                3 => {
                    return (v <= (0i32 as i64));
                }
                4 => {
                    return (v > (0i32 as i64));
                }
                5 => {
                    return (v >= (0i32 as i64));
                }
                _ => {}
            }
        }
        self.unsupported("condition code");
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn arithmetic(
        &mut self,
        mut destination: u32,
        mut left: u64,
        mut right: u64,
        mut subtract: bool,
        mut incoming: u32,
    ) -> u64 {
        if (destination >= (4i32 as u32)) {
            {
                let mut value: i64 = ((left as i16) as i64).wrapping_add(
                    (if subtract {
                        (((right as i16) as i64).wrapping_neg()).wrapping_sub((incoming as i64))
                    } else {
                        ((right as i16) as i64).wrapping_add((incoming as i64))
                    }),
                );
                let mut carry: bool = (if subtract {
                    (((left as u16) as u32) >= ((right as u16) as u32).wrapping_add(incoming))
                } else {
                    ((((left as u16) as u32).wrapping_add(((right as u16) as u32)))
                        .wrapping_add(incoming)
                        > (65535i32 as u32))
                });
                {
                    let _arg1 = ((((self.st[(0i32 as u64) as usize] as u32) & (!2048u32))
                        | ((if carry { 2048i32 } else { 0i32 }) as u32))
                        as u16);
                    self.st[(0i32 as u64) as usize] = _arg1;
                    _arg1
                };
                if (((self.st[(3i32 as u64) as usize] as i32) & 32i32) != 0) {
                    {
                        let _arg2 =
                            (value).clamp(((32768i32).wrapping_neg() as i64), (32767i32 as i64));
                        value = _arg2;
                        _arg2
                    };
                }
                return ((value as u16) as u64);
            }
        }
        let mut width: u32 = ((if (((self.st[(1i32 as u64) as usize] as i32) & 1024i32) != 0) {
            40i32
        } else {
            32i32
        }) as u32);
        let mut mask: u64 = ((1i32 as u64).wrapping_shl(width as u32)).wrapping_sub((1i32 as u64));
        let mut sign: u64 = (1i32 as u64).wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32);
        let mut value: u64 = ((if subtract {
            ((left).wrapping_sub(right)).wrapping_sub((incoming as u64))
        } else {
            ((left).wrapping_add(right)).wrapping_add((incoming as u64))
        }) & MASK40);
        let mut carry: bool = (if subtract {
            ((left & mask) >= (right & mask).wrapping_add((incoming as u64)))
        } else {
            (((left & mask).wrapping_add((right & mask))).wrapping_add((incoming as u64)) > mask)
        });
        {
            let _arg3 = ((((self.st[(0i32 as u64) as usize] as u32) & (!2048u32))
                | ((if carry { 2048i32 } else { 0i32 }) as u32)) as u16);
            self.st[(0i32 as u64) as usize] = _arg3;
            _arg3
        };
        let mut overflow: bool = (if subtract {
            ((((left ^ right) & (left ^ value)) & sign) != (0i32 as u64))
        } else {
            ((((!(left ^ right)) & (left ^ value)) & sign) != (0i32 as u64))
        });
        if overflow {
            {
                let mut flags: [u32; 4] = [
                    (10i32 as u32),
                    (9i32 as u32),
                    (15i32 as u32),
                    (14i32 as u32),
                ];
                {
                    let _arg4 = (self.st[(0i32 as u64) as usize]
                        | ((1u32).wrapping_shl(flags[destination as usize] as u32) as u16));
                    self.st[(0i32 as u64) as usize] = _arg4;
                    _arg4
                };
                if (((self.st[(1i32 as u64) as usize] as i32) & 512i32) != 0) {
                    {
                        let _arg5 = (if ((left & sign) != 0) {
                            (MASK40 ^ (sign).wrapping_sub((1i32 as u64)))
                        } else {
                            (sign).wrapping_sub((1i32 as u64))
                        });
                        value = _arg5;
                        _arg5
                    };
                }
            }
        }
        return value;
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn signed_shift(
        &mut self,
        mut destination: u32,
        mut source: u64,
        mut count: i32,
        mut update_carry: bool,
    ) -> u64 {
        if (destination >= (4i32 as u32)) {
            {
                let mut value: i64 = ((source as i16) as i64);
                {
                    let _arg1 = (if (count >= 0i32) {
                        (value).wrapping_mul((1i32 as i64).wrapping_shl(count as u32))
                    } else {
                        (value).wrapping_shr((count).wrapping_neg() as u32)
                    });
                    value = _arg1;
                    _arg1
                };
                if (((self.st[(3i32 as u64) as usize] as i32) & 32i32) != 0) {
                    {
                        let _arg2 =
                            (value).clamp(((32768i32).wrapping_neg() as i64), (32767i32 as i64));
                        value = _arg2;
                        _arg2
                    };
                }
                return ((value as u16) as u64);
            }
        }
        let mut compatibility: bool = (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0);
        let mut sign_extend: bool = (((self.st[(1i32 as u64) as usize] as i32) & 256i32) != 0);
        let mut width: u32 =
            ((if (compatibility || (((self.st[(1i32 as u64) as usize] as i32) & 1024i32) != 0)) {
                40i32
            } else {
                32i32
            }) as u32);
        let mut input: u64 = (source & MASK40);
        if (width == (32i32 as u32)) {
            {
                let _arg3 = (if sign_extend {
                    ((((source as i32) as i64) as u64) & MASK40)
                } else {
                    ((source as u32) as u64)
                });
                input = _arg3;
                _arg3
            };
        }
        if update_carry {
            {
                let mut carry: bool = false;
                if (count != 0) {
                    {
                        let mut bit: u32 = (if (count > 0i32) {
                            (width).wrapping_sub((count as u32))
                        } else {
                            (((count).wrapping_neg()).wrapping_sub(1i32) as u32)
                        });
                        {
                            let _arg4 = ((input & (1i32 as u64).wrapping_shl(bit as u32)) != 0);
                            carry = _arg4;
                            _arg4
                        };
                    }
                }
                {
                    let _arg5 = ((((self.st[(0i32 as u64) as usize] as u32) & (!2048u32))
                        | ((if carry { 2048i32 } else { 0i32 }) as u32))
                        as u16);
                    self.st[(0i32 as u64) as usize] = _arg5;
                    _arg5
                };
            }
        }
        if (count <= 0i32) {
            return ((if sign_extend {
                ((signed40(input)).wrapping_shr((count).wrapping_neg() as u32) as u64)
            } else {
                (input).wrapping_shr((count).wrapping_neg() as u32)
            }) & MASK40);
        }
        let mut value: i64 = signed40(input);
        let mut maximum: i64 = ((1i32 as i64)
            .wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32))
        .wrapping_sub((1i32 as i64));
        let mut minimum: i64 =
            ((1i32 as i64).wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32)).wrapping_neg();
        let mut overflow: bool = ((value > (maximum).wrapping_shr(count as u32))
            || (value < (minimum).wrapping_shr(count as u32)));
        let mut result: u64 = ((input).wrapping_shl(count as u32) & MASK40);
        if (overflow && (!compatibility)) {
            {
                let mut flags: [u32; 4] = [
                    (10i32 as u32),
                    (9i32 as u32),
                    (15i32 as u32),
                    (14i32 as u32),
                ];
                {
                    let _arg6 = (self.st[(0i32 as u64) as usize]
                        | ((1u32).wrapping_shl(flags[destination as usize] as u32) as u16));
                    self.st[(0i32 as u64) as usize] = _arg6;
                    _arg6
                };
                if (((self.st[(1i32 as u64) as usize] as i32) & 512i32) != 0) {
                    {
                        let _arg7 = (((if (value < (0i32 as i64)) {
                            minimum
                        } else {
                            maximum
                        }) as u64)
                            & MASK40);
                        result = _arg7;
                        _arg7
                    };
                }
            }
        }
        return result;
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn pointer_offset(
        &self,
        mut index: u32,
        mut pointer: u32,
        mut offset: i32,
        mut circular: bool,
        mut linear: bool,
    ) -> u32 {
        if (linear
            || ((!circular)
                && (!(((self.st[(2i32 as u64) as usize] as u32)
                    & (1u32).wrapping_shl(index as u32))
                    != 0))))
        {
            return ((pointer & (8323072i32 as u32))
                | (((pointer).wrapping_add((offset as u32)) as u16) as u32));
        }
        let mut compatibility: bool = (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0);
        let mut size: u32 = (self.memory[((if (index == (8i32 as u32)) {
            49i32
        } else {
            (if (compatibility || (index < (4i32 as u32))) {
                25i32
            } else {
                48i32
            })
        }) as u64) as usize] as u32);
        let mut position: u32 = ((pointer as u16) as u32);
        let mut start: u32 = (0i32 as u32);
        if compatibility {
            {
                let mut mask: u32 = {
                    let _arg1 = size;
                    self.circular_mask(_arg1)
                };
                {
                    let _arg2 = (position & (!mask));
                    start = _arg2;
                    _arg2
                };
                {
                    let _arg3 = (position & (mask as u32));
                    position = _arg3;
                    _arg3
                };
            }
        }
        if ((((!(size != 0)) || (position >= size)) || (offset <= (size as i32).wrapping_neg()))
            || (offset >= (size as i32)))
        {
            self.unsupported("circular pointer/index outside documented range");
        }
        let mut next: i32 = (position as i32).wrapping_add(offset);
        if (next < 0i32) {
            {
                let _arg4 = (next).wrapping_add((size as i32));
                next = _arg4;
                _arg4
            };
        } else {
            if (next >= (size as i32)) {
                {
                    let _arg5 = (next).wrapping_sub((size as i32));
                    next = _arg5;
                    _arg5
                };
            }
        }
        return ((pointer & (8323072i32 as u32))
            | (((start).wrapping_add((next as u32)) as u16) as u32));
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn pointer_address(
        &self,
        mut index: u32,
        mut pointer: u32,
        mut circular: bool,
        mut linear: bool,
    ) -> u32 {
        if (linear
            || ((!circular)
                && (!(((self.st[(2i32 as u64) as usize] as u32)
                    & (1u32).wrapping_shl(index as u32))
                    != 0))))
        {
            return pointer;
        }
        {
            let _arg1 = index;
            let _arg2 = pointer;
            let _arg3 = 0i32;
            let _arg4 = circular;
            let _arg5 = false;
            self.pointer_offset(_arg1, _arg2, _arg3, _arg4, _arg5)
        };
        let mut compatibility: bool = (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0);
        let mut base: u32 = (self.memory[((if (index == (8i32 as u32)) {
            (54i32 as u32)
        } else {
            (50i32 as u32).wrapping_add((index / (2i32 as u32)))
        }) as u64) as usize] as u32);
        let mut size: u32 = (self.memory[((if (index == (8i32 as u32)) {
            49i32
        } else {
            (if (compatibility || (index < (4i32 as u32))) {
                25i32
            } else {
                48i32
            })
        }) as u64) as usize] as u32);
        let mut start: u32 = (if compatibility {
            (((pointer as u16) as u32)
                & (!{
                    let _arg6 = size;
                    self.circular_mask(_arg6)
                }))
        } else {
            (0i32 as u32)
        });
        if (((base).wrapping_add(start)).wrapping_add(size) > (65536i32 as u32)) {
            self.unsupported("circular buffer crosses a main data page");
        }
        return ((pointer & (8323072i32 as u32)) | (base).wrapping_add(((pointer as u16) as u32)));
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn circular_mask(&self, mut size: u32) -> u32 {
        let mut mask: u32 = (1i32 as u32);
        while (mask < size) {
            {
                let _arg1 = ((mask).wrapping_shl(1i32 as u32) | (1i32 as u32));
                mask = _arg1;
                _arg1
            };
        }
        return mask;
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn pointer_bitreverse(&self, mut pointer: u32, mut subtract: bool) -> u32 {
        let mut increment: u32 = ((if (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0) {
            (self.xar[(0i32 as u64) as usize] as u16)
        } else {
            self.t[(0i32 as u64) as usize]
        }) as u32);
        let mut carry: u32 = (0i32 as u32);
        let mut result: u32 = (0i32 as u32);
        let mut bit: i32 = 16i32;
        while (bit != 0) {
            {
                {
                    let _arg1 = bit;
                    bit = _arg1.wrapping_sub(1);
                    bit
                };
                let mut left: u32 = ((pointer).wrapping_shr(bit as u32) & (1i32 as u32));
                let mut right: u32 = ((increment).wrapping_shr(bit as u32) & (1i32 as u32));
                let mut digit: i32 = (if subtract {
                    ((left as i32).wrapping_sub((right as i32))).wrapping_sub((carry as i32))
                } else {
                    (((left).wrapping_add(right)).wrapping_add(carry) as i32)
                });
                {
                    let _arg2 = (result
                        | (((digit as u32) & (1i32 as u32)).wrapping_shl(bit as u32) as u32));
                    result = _arg2;
                    _arg2
                };
                {
                    let _arg3 = ((if subtract {
                        (digit < 0i32)
                    } else {
                        (digit >= 2i32)
                    }) as u32);
                    carry = _arg3;
                    _arg3
                };
            }
        }
        return ((pointer & (8323072i32 as u32)) | result);
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn multiply(
        &mut self,
        mut destination: u32,
        mut source: u32,
        mut x: i64,
        mut y: i64,
        mut kind: u32,
        mut modifiers: u8,
        mut narrow_product: bool,
    ) -> u64 {
        let mut product: i64 = (x).wrapping_mul(y);
        if (((self.st[(1i32 as u64) as usize] as i32) & 64i32) != 0) {
            {
                let _arg1 = (product).wrapping_mul(((2i32 as i64) as i64));
                product = _arg1;
                _arg1
            };
        }
        let mut width: u32 = ((if ((((modifiers as i32) & 16i32) != 0)
            || (((self.st[(1i32 as u64) as usize] as i32) & 1024i32) != 0))
        {
            40i32
        } else {
            32i32
        }) as u32);
        let mut maximum: i64 = ((1i32 as i64)
            .wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32))
        .wrapping_sub((1i32 as i64));
        let mut minimum: i64 =
            ((1i32 as i64).wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32)).wrapping_neg();
        if ((((((self.st[(3i32 as u64) as usize] as i32) & 2i32) != 0)
            && (((self.st[(1i32 as u64) as usize] as i32) & 576i32) == 576i32))
            && (x == ((32768i32).wrapping_neg() as i64)))
            && (y == ((32768i32).wrapping_neg() as i64)))
        {
            {
                let _arg2 = (2147483647i32 as i64);
                product = _arg2;
                _arg2
            };
        }
        if narrow_product {
            {
                let _arg3 = (if (((modifiers as i32) & 32i32) != 0) {
                    ((product as u32) as i64)
                } else {
                    (((product as u32) as i32) as i64)
                });
                product = _arg3;
                _arg3
            };
        }
        let mut value: i64 = product;
        if (kind != 0) {
            {
                let mut accumulator: i64 = signed40(self.ac[(source as u64) as usize]);
                if (kind == (2i32 as u32)) {
                    {
                        let _arg4 = (accumulator).wrapping_shr((16i32 as u32));
                        accumulator = _arg4;
                        _arg4
                    };
                }
                {
                    let _arg5 = (accumulator).wrapping_add(
                        (if (kind == (3i32 as u32)) {
                            (product).wrapping_neg()
                        } else {
                            product
                        }),
                    );
                    value = _arg5;
                    _arg5
                };
            }
        }
        if (((modifiers as i32) & 1i32) != 0) {
            {
                let mut bits: u64 = (value as u64);
                let mut increment: bool =
                    (((!(((self.st[(2i32 as u64) as usize] as i32) & 1024i32) != 0))
                        || ((bits & (65535i32 as u64)) > (32768i32 as u64)))
                        || (((bits & (65535i32 as u64)) == (32768i32 as u64))
                            && ((bits & (65536i32 as u64)) != 0)));
                if increment {
                    {
                        let _arg6 = (value).wrapping_add(((32768i32 as i64) as i64));
                        value = _arg6;
                        _arg6
                    };
                }
            }
        }
        let mut overflow: bool = ((value > maximum) || (value < minimum));
        if overflow {
            {
                let mut flags: [u32; 4] = [
                    (10i32 as u32),
                    (9i32 as u32),
                    (15i32 as u32),
                    (14i32 as u32),
                ];
                {
                    let _arg7 = (self.st[(0i32 as u64) as usize]
                        | ((1u32).wrapping_shl(flags[destination as usize] as u32) as u16));
                    self.st[(0i32 as u64) as usize] = _arg7;
                    _arg7
                };
            }
        }
        if (((self.st[(1i32 as u64) as usize] as i32) & 512i32) != 0) {
            {
                let _arg8 = (value).clamp(minimum, maximum);
                value = _arg8;
                _arg8
            };
        }
        if (((modifiers as i32) & 1i32) != 0) {
            {
                let _arg9 = (value & ((!(65535i32 as i64)) as i64));
                value = _arg9;
                _arg9
            };
        }
        return ((value as u64) & MASK40);
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn fir_sum(
        &mut self,
        mut destination: u32,
        mut x: i64,
        mut y: i64,
        mut subtract: bool,
    ) -> u64 {
        let mut width: u32 = ((if (((self.st[(1i32 as u64) as usize] as i32) & 1024i32) != 0) {
            40i32
        } else {
            32i32
        }) as u32);
        let mut mask: u64 = ((1i32 as u64).wrapping_shl(width as u32)).wrapping_sub((1i32 as u64));
        let mut maximum: i64 = ((1i32 as i64)
            .wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32))
        .wrapping_sub((1i32 as i64));
        let mut minimum: i64 =
            ((1i32 as i64).wrapping_shl((width).wrapping_sub((1i32 as u32)) as u32)).wrapping_neg();
        let mut left: i64 = (x).wrapping_mul((65536i32 as i64));
        let mut right: i64 = (y).wrapping_mul((65536i32 as i64));
        let mut flags: [u32; 4] = [
            (10i32 as u32),
            (9i32 as u32),
            (15i32 as u32),
            (14i32 as u32),
        ];
        if (!(((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0)) {
            {
                if ((((left > maximum) || (left < minimum)) || (right > maximum))
                    || (right < minimum))
                {
                    {
                        let _arg1 = (self.st[(0i32 as u64) as usize]
                            | ((1u32).wrapping_shl(flags[destination as usize] as u32) as u16));
                        self.st[(0i32 as u64) as usize] = _arg1;
                        _arg1
                    };
                }
                if (((self.st[(1i32 as u64) as usize] as i32) & 512i32) != 0) {
                    {
                        {
                            let _arg2 = (left).clamp(minimum, maximum);
                            left = _arg2;
                            _arg2
                        };
                        {
                            let _arg3 = (right).clamp(minimum, maximum);
                            right = _arg3;
                            _arg3
                        };
                    }
                }
            }
        }
        let mut value: i64 = (if subtract {
            (left).wrapping_sub(right)
        } else {
            (left).wrapping_add(right)
        });
        let mut carry: bool = (if subtract {
            (((left as u64) & mask) >= ((right as u64) & mask))
        } else {
            (((left as u64) & mask).wrapping_add(((right as u64) & mask)) > mask)
        });
        {
            let _arg4 = ((((self.st[(0i32 as u64) as usize] as u32) & (!2048u32))
                | ((if carry { 2048i32 } else { 0i32 }) as u32)) as u16);
            self.st[(0i32 as u64) as usize] = _arg4;
            _arg4
        };
        if ((value > maximum) || (value < minimum)) {
            {
                let _arg5 = (self.st[(0i32 as u64) as usize]
                    | ((1u32).wrapping_shl(flags[destination as usize] as u32) as u16));
                self.st[(0i32 as u64) as usize] = _arg5;
                _arg5
            };
        }
        if (((self.st[(1i32 as u64) as usize] as i32) & 512i32) != 0) {
            {
                let _arg6 = (value).clamp(minimum, maximum);
                value = _arg6;
                _arg6
            };
        }
        return ((value as u64) & MASK40);
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn parallel_opcode(&self, mut op: u8) -> bool {
        return ((((op as i32) & 1i32) != 0)
            && (((op as i32) <= 9i32) || (((op as i32) >= 12i32) && ((op as i32) <= 95i32))));
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn loop_address(&self, mut level: u32, mut end: bool) -> u32 {
        let mut base: u32 = ((60i32 as u32).wrapping_add((level).wrapping_mul((4i32 as u32))))
            .wrapping_add(((if end { 2i32 } else { 0i32 }) as u32));
        return (((self.memory[(base as u64) as usize] as u32) & (255i32 as u32))
            .wrapping_shl(16i32 as u32)
            | (self.memory[((base).wrapping_add((1i32 as u32)) as u64) as usize] as u32));
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn sync_loop_view(&mut self) -> () {
        {
            let _arg1 = ((self.loop_type[(0i32 as u64) as usize] as i32) != 0i32);
            self.block_active = _arg1;
            _arg1
        };
        {
            let _arg4 = {
                let _arg2 = (0i32 as u32);
                let _arg3 = false;
                self.loop_address(_arg2, _arg3)
            };
            self.block_start = _arg4;
            _arg4
        };
        {
            let _arg7 = {
                let _arg5 = (0i32 as u32);
                let _arg6 = true;
                self.loop_address(_arg5, _arg6)
            };
            self.block_end = _arg7;
            _arg7
        };
        if (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0) {
            {
                if self.block_active {
                    {
                        let _arg8 = (self.st[(1i32 as u64) as usize] | (32768i32 as u16));
                        self.st[(1i32 as u64) as usize] = _arg8;
                        _arg8
                    };
                } else {
                    {
                        let _arg9 = (self.st[(1i32 as u64) as usize] & ((!32768i32) as u16));
                        self.st[(1i32 as u64) as usize] = _arg9;
                        _arg9
                    };
                }
            }
        }
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn braf(&mut self, mut enabled: bool) -> () {
        if (!(((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0)) {
            return;
        }
        if enabled {
            {
                if (!(self.loop_type[(0i32 as u64) as usize] != 0)) {
                    {
                        let _arg1 = (1i32 as u8);
                        self.loop_type[(0i32 as u64) as usize] = _arg1;
                        _arg1
                    };
                }
            }
        } else {
            {
                let _arg2 = (0i32 as u8);
                self.loop_type[(0i32 as u64) as usize] = _arg2;
                _arg2
            };
        }
        {
            self.sync_loop_view()
        };
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn status_bit(&mut self, mut bit: u32, mut index: u32, mut enabled: bool) -> () {
        if ((index == (1i32 as u32)) && (bit < (5i32 as u32))) {
            return;
        }
        if (((index == (1i32 as u32)) && (bit == (15i32 as u32)))
            && (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0))
        {
            {
                {
                    let _arg1 = enabled;
                    self.braf(_arg1)
                };
                return;
            }
        }
        if enabled {
            {
                let _arg2 =
                    (self.st[(index as u64) as usize] | ((1u32).wrapping_shl(bit as u32) as u16));
                self.st[(index as u64) as usize] = _arg2;
                _arg2
            };
        } else {
            {
                let _arg3 = (self.st[(index as u64) as usize]
                    & ((!(1u32).wrapping_shl(bit as u32)) as u16));
                self.st[(index as u64) as usize] = _arg3;
                _arg3
            };
        }
    }
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub(super) fn write_st1(&mut self, mut value: u16, mut protected_alias: bool) -> () {
        if (((self.st[(1i32 as u64) as usize] as i32) & 32i32) != 0) {
            {
                if protected_alias {
                    {
                        let _arg1 = ((((value as i32) & 31i32) ^ 16i32).wrapping_sub(16i32) as u16);
                        self.t[(2i32 as u64) as usize] = _arg1;
                        _arg1
                    };
                }
                {
                    let _arg2 = (((value as i32) & 32768i32) != 0);
                    self.braf(_arg2)
                };
            }
        }
        {
            let _arg3 = value;
            self.st[(1i32 as u64) as usize] = _arg3;
            _arg3
        };
    }
}
