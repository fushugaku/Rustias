// Generated from SH3 reference SHA256 27b411d6e8f6f4ba422ddac061f1251d6d6fd9a26c5172526d4a8fe14306439b
use super::{Bus, Sh3};
impl Sh3 {
    #[allow(
        unused_mut,
        unused_variables,
        unused_assignments,
        unused_parens,
        unused_labels
    )]
    pub fn step(&mut self, bus: &mut impl Bus) {
        if !self.delayed && self.sr & 0x10000000 == 0 {
            let irq = bus.interrupt();
            if irq.level > (self.sr >> 4 & 15) {
                self.exception(bus, irq.event, self.pc, true);
                if irq.event2 != 0 {
                    bus.event2(irq.event2);
                }
            }
        }
        if self.sleeping {
            bus.tick(64);
            self.cycles += 64;
            return;
        }
        if self.pc & 1 != 0 {
            self.failure("Unaligned instruction fetch");
        }
        let here = self.pc;
        let in_delay = self.delayed;
        let target = self.delayed_pc;
        self.delayed = false;
        self.restore_sr = false;
        let op = bus.read16(here);
        self.last_pc = here;
        self.last_op = op;
        self.pc = here.wrapping_add(2);
        let n = (op as u32 >> 8) & 15;
        let m = (op as u32 >> 4) & 15;
        let d = op as u32 & 15;
        let literal_pc = if in_delay {
            target.wrapping_add(2)
        } else {
            here.wrapping_add(4)
        };
        let a = self.r[n as usize];
        let b = self.r[m as usize];
        let imm = op as i8 as i32;
        let mut cost = 1u32;
        let mut decoded = true;
        'switch1: {
            match (op as i32).wrapping_shr(12i32 as u32) {
                0 => {
                    'switch2: {
                        match ((op as i32) & 15i32) {
                            4 => {
                                {
                                    let _arg1 = (self.r[(0i32 as u64) as usize]).wrapping_add(a);
                                    let _arg2 = b;
                                    let _arg3 = (1i32 as u32);
                                    self.wr(bus, _arg1, _arg2, _arg3)
                                };
                                break 'switch2;
                            }
                            5 => {
                                {
                                    let _arg4 = (self.r[(0i32 as u64) as usize]).wrapping_add(a);
                                    let _arg5 = b;
                                    let _arg6 = (2i32 as u32);
                                    self.wr(bus, _arg4, _arg5, _arg6)
                                };
                                break 'switch2;
                            }
                            6 => {
                                {
                                    let _arg7 = (self.r[(0i32 as u64) as usize]).wrapping_add(a);
                                    let _arg8 = b;
                                    let _arg9 = (4i32 as u32);
                                    self.wr(bus, _arg7, _arg8, _arg9)
                                };
                                break 'switch2;
                            }
                            7 => {
                                {
                                    let _arg10 = ((a as u64).wrapping_mul((b as u64)) as u32);
                                    self.macl = _arg10;
                                    _arg10
                                };
                                {
                                    let _arg11 = (2i32 as u32);
                                    cost = _arg11;
                                    _arg11
                                };
                                break 'switch2;
                            }
                            12 => {
                                {
                                    let _arg14 = {
                                        let _arg12 =
                                            (self.r[(0i32 as u64) as usize]).wrapping_add(b);
                                        let _arg13 = (1i32 as u32);
                                        self.rd(bus, _arg12, _arg13)
                                    };
                                    self.r[(n as u64) as usize] = _arg14;
                                    _arg14
                                };
                                break 'switch2;
                            }
                            13 => {
                                {
                                    let _arg17 = {
                                        let _arg15 =
                                            (self.r[(0i32 as u64) as usize]).wrapping_add(b);
                                        let _arg16 = (2i32 as u32);
                                        self.rd(bus, _arg15, _arg16)
                                    };
                                    self.r[(n as u64) as usize] = _arg17;
                                    _arg17
                                };
                                break 'switch2;
                            }
                            14 => {
                                {
                                    let _arg20 = {
                                        let _arg18 =
                                            (self.r[(0i32 as u64) as usize]).wrapping_add(b);
                                        let _arg19 = (4i32 as u32);
                                        self.rd(bus, _arg18, _arg19)
                                    };
                                    self.r[(n as u64) as usize] = _arg20;
                                    _arg20
                                };
                                break 'switch2;
                            }
                            15 => {
                                let mut left: i32 = ({
                                    let _arg21 = self.post(m, (4i32 as u32));
                                    let _arg22 = (4i32 as u32);
                                    self.rd(bus, _arg21, _arg22)
                                } as i32);
                                let mut right: i32 = ({
                                    let _arg23 = self.post(n, (4i32 as u32));
                                    let _arg24 = (4i32 as u32);
                                    self.rd(bus, _arg23, _arg24)
                                } as i32);
                                let mut product: i64 = (left as i64).wrapping_mul((right as i64));
                                let mut acc: u64 = ((self.mach as u64).wrapping_shl(32i32 as u32)
                                    | (self.macl as u64));
                                if ((self.sr & (2i32 as u32)) != 0) {
                                    {
                                        let mut value: i64 = (((acc).wrapping_shl(16i32 as u32)
                                            as i64)
                                            .wrapping_shr(16i32 as u32))
                                        .wrapping_add(product);
                                        {
                                            let _arg25 = (value).clamp(
                                                ((1i32 as i64).wrapping_shl(47i32 as u32))
                                                    .wrapping_neg(),
                                                ((1i32 as i64).wrapping_shl(47i32 as u32))
                                                    .wrapping_sub((1i32 as i64)),
                                            );
                                            value = _arg25;
                                            _arg25
                                        };
                                        {
                                            let _arg26 = (value as u64);
                                            acc = _arg26;
                                            _arg26
                                        };
                                    }
                                } else {
                                    {
                                        let _arg27 = (acc).wrapping_add(((product as u64) as u64));
                                        acc = _arg27;
                                        _arg27
                                    };
                                }
                                {
                                    let _arg28 = ((acc).wrapping_shr(32i32 as u32) as u32);
                                    self.mach = _arg28;
                                    _arg28
                                };
                                {
                                    let _arg29 = (acc as u32);
                                    self.macl = _arg29;
                                    _arg29
                                };
                                {
                                    let _arg30 = (3i32 as u32);
                                    cost = _arg30;
                                    _arg30
                                };
                                break 'switch2;
                            }
                            _ => {
                                if (((op as i32) & 61455i32) == 2i32) {
                                    {
                                        {
                                            let _arg32 = {
                                                let _arg31 = m;
                                                self.control(_arg31)
                                            };
                                            self.r[(n as u64) as usize] = _arg32;
                                            _arg32
                                        };
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 10i32) {
                                    {
                                        {
                                            let _arg33 = self.mach;
                                            self.r[(n as u64) as usize] = _arg33;
                                            _arg33
                                        };
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 26i32) {
                                    {
                                        {
                                            let _arg34 = self.macl;
                                            self.r[(n as u64) as usize] = _arg34;
                                            _arg34
                                        };
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 42i32) {
                                    {
                                        {
                                            let _arg35 = self.pr;
                                            self.r[(n as u64) as usize] = _arg35;
                                            _arg35
                                        };
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 41i32) {
                                    {
                                        {
                                            let _arg36 = ({ self.t() } as u32);
                                            self.r[(n as u64) as usize] = _arg36;
                                            _arg36
                                        };
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 3i32) {
                                    {
                                        {
                                            let _arg37 = (here).wrapping_add((4i32 as u32));
                                            self.pr = _arg37;
                                            _arg37
                                        };
                                        self.jump(
                                            ((here).wrapping_add((4i32 as u32))).wrapping_add(a),
                                            &mut cost,
                                            in_delay,
                                        );
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 35i32) {
                                    {
                                        self.jump(
                                            ((here).wrapping_add((4i32 as u32))).wrapping_add(a),
                                            &mut cost,
                                            in_delay,
                                        );
                                        break 'switch2;
                                    }
                                }
                                if (((op as i32) & 61695i32) == 131i32) {
                                    {
                                        break 'switch2;
                                    }
                                }
                                'switch3: {
                                    match (op as i32) {
                                        8 => {
                                            {
                                                let _arg38 = false;
                                                self.set_t(_arg38)
                                            };
                                            break 'switch3;
                                        }
                                        24 => {
                                            {
                                                let _arg39 = true;
                                                self.set_t(_arg39)
                                            };
                                            break 'switch3;
                                        }
                                        9 => {
                                            break 'switch3;
                                        }
                                        25 => {
                                            {
                                                let _arg40 = (self.sr & ((!769u32) as u32));
                                                self.sr = _arg40;
                                                _arg40
                                            };
                                            break 'switch3;
                                        }
                                        40 => {
                                            {
                                                let _arg42 = {
                                                    let _arg41 = (0i32 as u32);
                                                    self.macl = _arg41;
                                                    _arg41
                                                };
                                                self.mach = _arg42;
                                                _arg42
                                            };
                                            break 'switch3;
                                        }
                                        11 => {
                                            self.jump(self.pr, &mut cost, in_delay);
                                            break 'switch3;
                                        }
                                        43 => {
                                            if (!((self.sr & (1073741824i32 as u32)) != 0)) {
                                                self.failure("RTE in user mode");
                                            }
                                            self.jump(self.spc, &mut cost, in_delay);
                                            {
                                                let _arg43 = self.ssr;
                                                self.set_sr(_arg43)
                                            };
                                            {
                                                let _arg44 = (4i32 as u32);
                                                cost = _arg44;
                                                _arg44
                                            };
                                            break 'switch3;
                                        }
                                        27 => {
                                            {
                                                let _arg45 = true;
                                                self.sleeping = _arg45;
                                                _arg45
                                            };
                                            break 'switch3;
                                        }
                                        56 => {
                                            self.failure("LDTLB requires an MMU model");
                                        }
                                        _ => {
                                            {
                                                let _arg46 = false;
                                                decoded = _arg46;
                                                _arg46
                                            };
                                            break 'switch3;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    break 'switch1;
                }
                1 => {
                    {
                        let _arg47 = (a).wrapping_add((4i32 as u32).wrapping_mul(d));
                        let _arg48 = b;
                        let _arg49 = (4i32 as u32);
                        self.wr(bus, _arg47, _arg48, _arg49)
                    };
                    break 'switch1;
                }
                2 => {
                    'switch4: {
                        match d {
                            0 => {
                                {
                                    let _arg50 = a;
                                    let _arg51 = b;
                                    let _arg52 = (1i32 as u32);
                                    self.wr(bus, _arg50, _arg51, _arg52)
                                };
                                break 'switch4;
                            }
                            1 => {
                                {
                                    let _arg53 = a;
                                    let _arg54 = b;
                                    let _arg55 = (2i32 as u32);
                                    self.wr(bus, _arg53, _arg54, _arg55)
                                };
                                break 'switch4;
                            }
                            2 => {
                                {
                                    let _arg56 = a;
                                    let _arg57 = b;
                                    let _arg58 = (4i32 as u32);
                                    self.wr(bus, _arg56, _arg57, _arg58)
                                };
                                break 'switch4;
                            }
                            4 => {
                                {
                                    let _arg59 = (self.r[(n as u64) as usize])
                                        .wrapping_sub(((1i32 as u32) as u32));
                                    self.r[(n as u64) as usize] = _arg59;
                                    _arg59
                                };
                                {
                                    let _arg60 = self.r[(n as u64) as usize];
                                    let _arg61 = b;
                                    let _arg62 = (1i32 as u32);
                                    self.wr(bus, _arg60, _arg61, _arg62)
                                };
                                break 'switch4;
                            }
                            5 => {
                                {
                                    let _arg63 = (self.r[(n as u64) as usize])
                                        .wrapping_sub(((2i32 as u32) as u32));
                                    self.r[(n as u64) as usize] = _arg63;
                                    _arg63
                                };
                                {
                                    let _arg64 = self.r[(n as u64) as usize];
                                    let _arg65 = b;
                                    let _arg66 = (2i32 as u32);
                                    self.wr(bus, _arg64, _arg65, _arg66)
                                };
                                break 'switch4;
                            }
                            6 => {
                                {
                                    let _arg67 = (self.r[(n as u64) as usize])
                                        .wrapping_sub(((4i32 as u32) as u32));
                                    self.r[(n as u64) as usize] = _arg67;
                                    _arg67
                                };
                                {
                                    let _arg68 = self.r[(n as u64) as usize];
                                    let _arg69 = b;
                                    let _arg70 = (4i32 as u32);
                                    self.wr(bus, _arg68, _arg69, _arg70)
                                };
                                break 'switch4;
                            }
                            7 => {
                                {
                                    let _arg71 = ((((self.sr & (!769u32))
                                        | ((a).wrapping_shr(31i32 as u32))
                                            .wrapping_shl(8i32 as u32))
                                        | ((b).wrapping_shr(31i32 as u32))
                                            .wrapping_shl(9i32 as u32))
                                        | (a ^ b).wrapping_shr(31i32 as u32));
                                    self.sr = _arg71;
                                    _arg71
                                };
                                break 'switch4;
                            }
                            8 => {
                                {
                                    let _arg72 = ((a & b) == (0i32 as u32));
                                    self.set_t(_arg72)
                                };
                                break 'switch4;
                            }
                            9 => {
                                {
                                    let _arg73 = (a & b);
                                    self.r[(n as u64) as usize] = _arg73;
                                    _arg73
                                };
                                break 'switch4;
                            }
                            10 => {
                                {
                                    let _arg74 = (a ^ b);
                                    self.r[(n as u64) as usize] = _arg74;
                                    _arg74
                                };
                                break 'switch4;
                            }
                            11 => {
                                {
                                    let _arg75 = (a | b);
                                    self.r[(n as u64) as usize] = _arg75;
                                    _arg75
                                };
                                break 'switch4;
                            }
                            12 => {
                                let mut x: u32 = (a ^ b);
                                {
                                    let _arg76 = (((((x & (255i32 as u32)) == (0i32 as u32))
                                        || ((x & (65280i32 as u32)) == (0i32 as u32)))
                                        || ((x & (16711680i32 as u32)) == (0i32 as u32)))
                                        || ((x & 4278190080u32) == (0i32 as u32)));
                                    self.set_t(_arg76)
                                };
                                break 'switch4;
                            }
                            13 => {
                                {
                                    let _arg77 = ((b).wrapping_shl(16i32 as u32)
                                        | (a).wrapping_shr(16i32 as u32));
                                    self.r[(n as u64) as usize] = _arg77;
                                    _arg77
                                };
                                break 'switch4;
                            }
                            14 => {
                                {
                                    let _arg78 = (((a as u16) as i32)
                                        .wrapping_mul(((b as u16) as i32))
                                        as u32);
                                    self.macl = _arg78;
                                    _arg78
                                };
                                {
                                    let _arg79 = (2i32 as u32);
                                    cost = _arg79;
                                    _arg79
                                };
                                break 'switch4;
                            }
                            15 => {
                                {
                                    let _arg80 = (((a as i16) as i32)
                                        .wrapping_mul(((b as i16) as i32))
                                        as u32);
                                    self.macl = _arg80;
                                    _arg80
                                };
                                {
                                    let _arg81 = (2i32 as u32);
                                    cost = _arg81;
                                    _arg81
                                };
                                break 'switch4;
                            }
                            _ => {
                                {
                                    let _arg82 = false;
                                    decoded = _arg82;
                                    _arg82
                                };
                                break 'switch4;
                            }
                        }
                    }
                    break 'switch1;
                }
                3 => {
                    'switch5: {
                        match d {
                            0 => {
                                {
                                    let _arg83 = (a == b);
                                    self.set_t(_arg83)
                                };
                                break 'switch5;
                            }
                            2 => {
                                {
                                    let _arg84 = (a >= b);
                                    self.set_t(_arg84)
                                };
                                break 'switch5;
                            }
                            3 => {
                                {
                                    let _arg85 = ((a as i32) >= (b as i32));
                                    self.set_t(_arg85)
                                };
                                break 'switch5;
                            }
                            4 => {
                                let mut oldq: bool = ((self.sr & (256i32 as u32)) != 0);
                                let mut q: bool = ((a).wrapping_shr(31i32 as u32) != 0);
                                let mut mm: bool = ((self.sr & (512i32 as u32)) != 0);
                                let mut x: u32 =
                                    ((a).wrapping_shl(1i32 as u32) | ({ self.t() } as u32));
                                let mut previous: u32 = x;
                                if (!oldq) {
                                    {
                                        if (!mm) {
                                            {
                                                {
                                                    let _arg86 = (x).wrapping_sub((b as u32));
                                                    x = _arg86;
                                                    _arg86
                                                };
                                                let mut c: bool = (x > previous);
                                                {
                                                    let _arg87 = (if q { (!c) } else { c });
                                                    q = _arg87;
                                                    _arg87
                                                };
                                            }
                                        } else {
                                            {
                                                {
                                                    let _arg88 = (x).wrapping_add((b as u32));
                                                    x = _arg88;
                                                    _arg88
                                                };
                                                let mut c: bool = (x < previous);
                                                {
                                                    let _arg89 = (if q { c } else { (!c) });
                                                    q = _arg89;
                                                    _arg89
                                                };
                                            }
                                        }
                                    }
                                } else {
                                    {
                                        if (!mm) {
                                            {
                                                {
                                                    let _arg90 = (x).wrapping_add((b as u32));
                                                    x = _arg90;
                                                    _arg90
                                                };
                                                let mut c: bool = (x < previous);
                                                {
                                                    let _arg91 = (if q { (!c) } else { c });
                                                    q = _arg91;
                                                    _arg91
                                                };
                                            }
                                        } else {
                                            {
                                                {
                                                    let _arg92 = (x).wrapping_sub((b as u32));
                                                    x = _arg92;
                                                    _arg92
                                                };
                                                let mut c: bool = (x > previous);
                                                {
                                                    let _arg93 = (if q { c } else { (!c) });
                                                    q = _arg93;
                                                    _arg93
                                                };
                                            }
                                        }
                                    }
                                }
                                {
                                    let _arg94 = x;
                                    self.r[(n as u64) as usize] = _arg94;
                                    _arg94
                                };
                                {
                                    let _arg95 = ((self.sr & (!256u32))
                                        | (q as u32).wrapping_shl(8i32 as u32));
                                    self.sr = _arg95;
                                    _arg95
                                };
                                {
                                    let _arg96 = ((q as i32) == (mm as i32));
                                    self.set_t(_arg96)
                                };
                                break 'switch5;
                            }
                            5 => {
                                let mut x: u64 = (a as u64).wrapping_mul((b as u64));
                                {
                                    let _arg97 = ((x).wrapping_shr(32i32 as u32) as u32);
                                    self.mach = _arg97;
                                    _arg97
                                };
                                {
                                    let _arg98 = (x as u32);
                                    self.macl = _arg98;
                                    _arg98
                                };
                                {
                                    let _arg99 = (2i32 as u32);
                                    cost = _arg99;
                                    _arg99
                                };
                                break 'switch5;
                            }
                            6 => {
                                {
                                    let _arg100 = (a > b);
                                    self.set_t(_arg100)
                                };
                                break 'switch5;
                            }
                            7 => {
                                {
                                    let _arg101 = ((a as i32) > (b as i32));
                                    self.set_t(_arg101)
                                };
                                break 'switch5;
                            }
                            8 => {
                                {
                                    let _arg102 = (a).wrapping_sub(b);
                                    self.r[(n as u64) as usize] = _arg102;
                                    _arg102
                                };
                                break 'switch5;
                            }
                            10 => {
                                let mut rhs: u64 = (b as u64).wrapping_add(({ self.t() } as u64));
                                {
                                    let _arg103 = ((a as u64).wrapping_sub(rhs) as u32);
                                    self.r[(n as u64) as usize] = _arg103;
                                    _arg103
                                };
                                {
                                    let _arg104 = ((a as u64) < rhs);
                                    self.set_t(_arg104)
                                };
                                break 'switch5;
                            }
                            11 => {
                                let mut x: u32 = (a).wrapping_sub(b);
                                {
                                    let _arg105 = x;
                                    self.r[(n as u64) as usize] = _arg105;
                                    _arg105
                                };
                                {
                                    let _arg106 =
                                        ((((a ^ b) & (a ^ x)) & 2147483648u32) != (0i32 as u32));
                                    self.set_t(_arg106)
                                };
                                break 'switch5;
                            }
                            12 => {
                                {
                                    let _arg107 = (a).wrapping_add(b);
                                    self.r[(n as u64) as usize] = _arg107;
                                    _arg107
                                };
                                break 'switch5;
                            }
                            13 => {
                                let mut x: u64 =
                                    (((a as i32) as i64).wrapping_mul(((b as i32) as i64)) as u64);
                                {
                                    let _arg108 = ((x).wrapping_shr(32i32 as u32) as u32);
                                    self.mach = _arg108;
                                    _arg108
                                };
                                {
                                    let _arg109 = (x as u32);
                                    self.macl = _arg109;
                                    _arg109
                                };
                                {
                                    let _arg110 = (2i32 as u32);
                                    cost = _arg110;
                                    _arg110
                                };
                                break 'switch5;
                            }
                            14 => {
                                let mut x: u64 = ((a as u64).wrapping_add((b as u64)))
                                    .wrapping_add(({ self.t() } as u64));
                                {
                                    let _arg111 = (x as u32);
                                    self.r[(n as u64) as usize] = _arg111;
                                    _arg111
                                };
                                {
                                    let _arg112 = ((x).wrapping_shr(32i32 as u32) != 0);
                                    self.set_t(_arg112)
                                };
                                break 'switch5;
                            }
                            15 => {
                                let mut x: u32 = (a).wrapping_add(b);
                                {
                                    let _arg113 = x;
                                    self.r[(n as u64) as usize] = _arg113;
                                    _arg113
                                };
                                {
                                    let _arg114 =
                                        ((((!(a ^ b)) & (a ^ x)) & 2147483648u32) != (0i32 as u32));
                                    self.set_t(_arg114)
                                };
                                break 'switch5;
                            }
                            _ => {
                                {
                                    let _arg115 = false;
                                    decoded = _arg115;
                                    _arg115
                                };
                                break 'switch5;
                            }
                        }
                    }
                    break 'switch1;
                }
                4 => {
                    if ((d == (12i32 as u32)) || (d == (13i32 as u32))) {
                        {
                            if ((b as i32) >= 0i32) {
                                {
                                    let _arg116 = (a).wrapping_shl((b & (31i32 as u32)) as u32);
                                    self.r[(n as u64) as usize] = _arg116;
                                    _arg116
                                };
                            } else {
                                {
                                    let mut count: u32 =
                                        ((!b) & (31i32 as u32)).wrapping_add((1i32 as u32));
                                    if (count == (32i32 as u32)) {
                                        {
                                            let _arg117 = (if ((d == (12i32 as u32))
                                                && ((a as i32) < 0i32))
                                            {
                                                4294967295u32
                                            } else {
                                                (0i32 as u32)
                                            });
                                            self.r[(n as u64) as usize] = _arg117;
                                            _arg117
                                        };
                                    } else {
                                        {
                                            let _arg118 = (if (d == (12i32 as u32)) {
                                                ((a as i32).wrapping_shr(count as u32) as u32)
                                            } else {
                                                (a).wrapping_shr(count as u32)
                                            });
                                            self.r[(n as u64) as usize] = _arg118;
                                            _arg118
                                        };
                                    }
                                }
                            }
                            break 'switch1;
                        }
                    }
                    if (d == (15i32 as u32)) {
                        {
                            let mut left: i32 = (({
                                let _arg119 = self.post(m, (2i32 as u32));
                                let _arg120 = (2i32 as u32);
                                self.rd(bus, _arg119, _arg120)
                            } as i16) as i32);
                            let mut right: i32 = (({
                                let _arg121 = self.post(n, (2i32 as u32));
                                let _arg122 = (2i32 as u32);
                                self.rd(bus, _arg121, _arg122)
                            } as i16) as i32);
                            let mut product: i64 = (left as i64).wrapping_mul((right as i64));
                            if ((self.sr & (2i32 as u32)) != 0) {
                                {
                                    let mut sum: i64 =
                                        ((self.macl as i32) as i64).wrapping_add(product);
                                    {
                                        let _arg123 = ((sum).clamp(
                                            (((2147483647i32).wrapping_neg()).wrapping_sub(1i32)
                                                as i64),
                                            (2147483647i32 as i64),
                                        )
                                            as u32);
                                        self.macl = _arg123;
                                        _arg123
                                    };
                                }
                            } else {
                                {
                                    let mut sum: u64 = ((self.mach as u64)
                                        .wrapping_shl(32i32 as u32)
                                        | (self.macl as u64))
                                        .wrapping_add((product as u64));
                                    {
                                        let _arg124 = ((sum).wrapping_shr(32i32 as u32) as u32);
                                        self.mach = _arg124;
                                        _arg124
                                    };
                                    {
                                        let _arg125 = (sum as u32);
                                        self.macl = _arg125;
                                        _arg125
                                    };
                                }
                            }
                            {
                                let _arg126 = (3i32 as u32);
                                cost = _arg126;
                                _arg126
                            };
                            break 'switch1;
                        }
                    }
                    if (d == (14i32 as u32)) {
                        {
                            {
                                let _arg127 = m;
                                let _arg128 = a;
                                self.set_control(_arg127, _arg128)
                            };
                            break 'switch1;
                        }
                    }
                    if (d == (7i32 as u32)) {
                        {
                            let mut value: u32 = {
                                let _arg129 = self.post(n, (4i32 as u32));
                                let _arg130 = (4i32 as u32);
                                self.rd(bus, _arg129, _arg130)
                            };
                            {
                                let _arg131 = m;
                                let _arg132 = value;
                                self.set_control(_arg131, _arg132)
                            };
                            break 'switch1;
                        }
                    }
                    if (d == (3i32 as u32)) {
                        {
                            {
                                let _arg133 = (self.r[(n as u64) as usize])
                                    .wrapping_sub(((4i32 as u32) as u32));
                                self.r[(n as u64) as usize] = _arg133;
                                _arg133
                            };
                            {
                                let _arg135 = self.r[(n as u64) as usize];
                                let _arg136 = {
                                    let _arg134 = m;
                                    self.control(_arg134)
                                };
                                let _arg137 = (4i32 as u32);
                                self.wr(bus, _arg135, _arg136, _arg137)
                            };
                            break 'switch1;
                        }
                    }
                    'switch6: {
                        match ((op as i32) & 255i32) {
                            0 | 32 => {
                                {
                                    let _arg138 = ((a).wrapping_shr(31i32 as u32) != 0);
                                    self.set_t(_arg138)
                                };
                                {
                                    let _arg139 = (a).wrapping_shl(1i32 as u32);
                                    self.r[(n as u64) as usize] = _arg139;
                                    _arg139
                                };
                                break 'switch6;
                            }
                            1 => {
                                {
                                    let _arg140 = ((a & (1i32 as u32)) != 0);
                                    self.set_t(_arg140)
                                };
                                {
                                    let _arg141 = (a).wrapping_shr(1i32 as u32);
                                    self.r[(n as u64) as usize] = _arg141;
                                    _arg141
                                };
                                break 'switch6;
                            }
                            33 => {
                                {
                                    let _arg142 = ((a & (1i32 as u32)) != 0);
                                    self.set_t(_arg142)
                                };
                                {
                                    let _arg143 = ((a as i32).wrapping_shr(1i32 as u32) as u32);
                                    self.r[(n as u64) as usize] = _arg143;
                                    _arg143
                                };
                                break 'switch6;
                            }
                            4 => {
                                {
                                    let _arg144 = ((a).wrapping_shr(31i32 as u32) != 0);
                                    self.set_t(_arg144)
                                };
                                {
                                    let _arg145 = ((a).wrapping_shl(1i32 as u32)
                                        | (a).wrapping_shr(31i32 as u32));
                                    self.r[(n as u64) as usize] = _arg145;
                                    _arg145
                                };
                                break 'switch6;
                            }
                            5 => {
                                {
                                    let _arg146 = ((a & (1i32 as u32)) != 0);
                                    self.set_t(_arg146)
                                };
                                {
                                    let _arg147 = ((a).wrapping_shr(1i32 as u32)
                                        | (a).wrapping_shl(31i32 as u32));
                                    self.r[(n as u64) as usize] = _arg147;
                                    _arg147
                                };
                                break 'switch6;
                            }
                            36 => {
                                let mut old: bool = { self.t() };
                                {
                                    let _arg148 = ((a).wrapping_shr(31i32 as u32) != 0);
                                    self.set_t(_arg148)
                                };
                                {
                                    let _arg149 = ((a).wrapping_shl(1i32 as u32) | (old as u32));
                                    self.r[(n as u64) as usize] = _arg149;
                                    _arg149
                                };
                                break 'switch6;
                            }
                            37 => {
                                let mut old: bool = { self.t() };
                                {
                                    let _arg150 = ((a & (1i32 as u32)) != 0);
                                    self.set_t(_arg150)
                                };
                                {
                                    let _arg151 = ((a).wrapping_shr(1i32 as u32)
                                        | (old as u32).wrapping_shl(31i32 as u32));
                                    self.r[(n as u64) as usize] = _arg151;
                                    _arg151
                                };
                                break 'switch6;
                            }
                            8 => {
                                {
                                    let _arg152 = (a).wrapping_shl(2i32 as u32);
                                    self.r[(n as u64) as usize] = _arg152;
                                    _arg152
                                };
                                break 'switch6;
                            }
                            9 => {
                                {
                                    let _arg153 = (a).wrapping_shr(2i32 as u32);
                                    self.r[(n as u64) as usize] = _arg153;
                                    _arg153
                                };
                                break 'switch6;
                            }
                            24 => {
                                {
                                    let _arg154 = (a).wrapping_shl(8i32 as u32);
                                    self.r[(n as u64) as usize] = _arg154;
                                    _arg154
                                };
                                break 'switch6;
                            }
                            25 => {
                                {
                                    let _arg155 = (a).wrapping_shr(8i32 as u32);
                                    self.r[(n as u64) as usize] = _arg155;
                                    _arg155
                                };
                                break 'switch6;
                            }
                            40 => {
                                {
                                    let _arg156 = (a).wrapping_shl(16i32 as u32);
                                    self.r[(n as u64) as usize] = _arg156;
                                    _arg156
                                };
                                break 'switch6;
                            }
                            41 => {
                                {
                                    let _arg157 = (a).wrapping_shr(16i32 as u32);
                                    self.r[(n as u64) as usize] = _arg157;
                                    _arg157
                                };
                                break 'switch6;
                            }
                            16 => {
                                {
                                    let _arg158 = (a).wrapping_sub((1i32 as u32));
                                    self.r[(n as u64) as usize] = _arg158;
                                    _arg158
                                };
                                {
                                    let _arg159 = (self.r[(n as u64) as usize] == (0i32 as u32));
                                    self.set_t(_arg159)
                                };
                                break 'switch6;
                            }
                            17 => {
                                {
                                    let _arg160 = ((a as i32) >= 0i32);
                                    self.set_t(_arg160)
                                };
                                break 'switch6;
                            }
                            21 => {
                                {
                                    let _arg161 = ((a as i32) > 0i32);
                                    self.set_t(_arg161)
                                };
                                break 'switch6;
                            }
                            11 => {
                                {
                                    let _arg162 = (here).wrapping_add((4i32 as u32));
                                    self.pr = _arg162;
                                    _arg162
                                };
                                self.jump(a, &mut cost, in_delay);
                                break 'switch6;
                            }
                            43 => {
                                self.jump(a, &mut cost, in_delay);
                                break 'switch6;
                            }
                            27 => {
                                let mut value: u8 = bus.read8(a);
                                {
                                    let _arg163 = ((value as i32) == 0i32);
                                    self.set_t(_arg163)
                                };
                                bus.write8(a, (((value as i32) | 128i32) as u8));
                                {
                                    let _arg164 = (4i32 as u32);
                                    cost = _arg164;
                                    _arg164
                                };
                                break 'switch6;
                            }
                            10 => {
                                {
                                    let _arg165 = (0i32 as u32);
                                    let _arg166 = a;
                                    self.set_system(_arg165, _arg166)
                                };
                                break 'switch6;
                            }
                            26 => {
                                {
                                    let _arg167 = (1i32 as u32);
                                    let _arg168 = a;
                                    self.set_system(_arg167, _arg168)
                                };
                                break 'switch6;
                            }
                            42 => {
                                {
                                    let _arg169 = (2i32 as u32);
                                    let _arg170 = a;
                                    self.set_system(_arg169, _arg170)
                                };
                                break 'switch6;
                            }
                            6 | 22 | 38 => {
                                let mut value: u32 = {
                                    let _arg171 = self.post(n, (4i32 as u32));
                                    let _arg172 = (4i32 as u32);
                                    self.rd(bus, _arg171, _arg172)
                                };
                                {
                                    let _arg173 = m;
                                    let _arg174 = value;
                                    self.set_system(_arg173, _arg174)
                                };
                                break 'switch6;
                            }
                            2 | 18 | 34 => {
                                {
                                    let _arg175 = (self.r[(n as u64) as usize])
                                        .wrapping_sub(((4i32 as u32) as u32));
                                    self.r[(n as u64) as usize] = _arg175;
                                    _arg175
                                };
                                {
                                    let _arg177 = self.r[(n as u64) as usize];
                                    let _arg178 = {
                                        let _arg176 = m;
                                        self.system(_arg176)
                                    };
                                    let _arg179 = (4i32 as u32);
                                    self.wr(bus, _arg177, _arg178, _arg179)
                                };
                                break 'switch6;
                            }
                            _ => {
                                {
                                    let _arg180 = false;
                                    decoded = _arg180;
                                    _arg180
                                };
                                break 'switch6;
                            }
                        }
                    }
                    break 'switch1;
                }
                5 => {
                    {
                        let _arg183 = {
                            let _arg181 = (b).wrapping_add((4i32 as u32).wrapping_mul(d));
                            let _arg182 = (4i32 as u32);
                            self.rd(bus, _arg181, _arg182)
                        };
                        self.r[(n as u64) as usize] = _arg183;
                        _arg183
                    };
                    break 'switch1;
                }
                6 => {
                    'switch7: {
                        match d {
                            0 => {
                                {
                                    let _arg186 = {
                                        let _arg184 = b;
                                        let _arg185 = (1i32 as u32);
                                        self.rd(bus, _arg184, _arg185)
                                    };
                                    self.r[(n as u64) as usize] = _arg186;
                                    _arg186
                                };
                                break 'switch7;
                            }
                            1 => {
                                {
                                    let _arg189 = {
                                        let _arg187 = b;
                                        let _arg188 = (2i32 as u32);
                                        self.rd(bus, _arg187, _arg188)
                                    };
                                    self.r[(n as u64) as usize] = _arg189;
                                    _arg189
                                };
                                break 'switch7;
                            }
                            2 => {
                                {
                                    let _arg192 = {
                                        let _arg190 = b;
                                        let _arg191 = (4i32 as u32);
                                        self.rd(bus, _arg190, _arg191)
                                    };
                                    self.r[(n as u64) as usize] = _arg192;
                                    _arg192
                                };
                                break 'switch7;
                            }
                            3 => {
                                {
                                    let _arg193 = b;
                                    self.r[(n as u64) as usize] = _arg193;
                                    _arg193
                                };
                                break 'switch7;
                            }
                            4 | 5 | 6 => {
                                let mut size: u32 =
                                    (1u32).wrapping_shl((d).wrapping_sub((4i32 as u32)) as u32);
                                let mut value: u32 = {
                                    let _arg194 = b;
                                    let _arg195 = size;
                                    self.rd(bus, _arg194, _arg195)
                                };
                                if (n != m) {
                                    {
                                        let _arg196 = (self.r[(m as u64) as usize])
                                            .wrapping_add((size as u32));
                                        self.r[(m as u64) as usize] = _arg196;
                                        _arg196
                                    };
                                }
                                {
                                    let _arg197 = value;
                                    self.r[(n as u64) as usize] = _arg197;
                                    _arg197
                                };
                                break 'switch7;
                            }
                            7 => {
                                {
                                    let _arg198 = (!b);
                                    self.r[(n as u64) as usize] = _arg198;
                                    _arg198
                                };
                                break 'switch7;
                            }
                            8 => {
                                {
                                    let _arg199 = (((b & 4294901760u32)
                                        | (b & (255i32 as u32)).wrapping_shl(8i32 as u32))
                                        | ((b).wrapping_shr(8i32 as u32) & (255i32 as u32)));
                                    self.r[(n as u64) as usize] = _arg199;
                                    _arg199
                                };
                                break 'switch7;
                            }
                            9 => {
                                {
                                    let _arg200 = ((b).wrapping_shl(16i32 as u32)
                                        | (b).wrapping_shr(16i32 as u32));
                                    self.r[(n as u64) as usize] = _arg200;
                                    _arg200
                                };
                                break 'switch7;
                            }
                            10 => {
                                let mut rhs: u64 = (b as u64).wrapping_add(({ self.t() } as u64));
                                {
                                    let _arg201 = ((0i32 as u64).wrapping_sub(rhs) as u32);
                                    self.r[(n as u64) as usize] = _arg201;
                                    _arg201
                                };
                                {
                                    let _arg202 = (rhs != (0i32 as u64));
                                    self.set_t(_arg202)
                                };
                                break 'switch7;
                            }
                            11 => {
                                {
                                    let _arg203 = (0i32 as u32).wrapping_sub(b);
                                    self.r[(n as u64) as usize] = _arg203;
                                    _arg203
                                };
                                break 'switch7;
                            }
                            12 => {
                                {
                                    let _arg204 = ((b as u8) as u32);
                                    self.r[(n as u64) as usize] = _arg204;
                                    _arg204
                                };
                                break 'switch7;
                            }
                            13 => {
                                {
                                    let _arg205 = ((b as u16) as u32);
                                    self.r[(n as u64) as usize] = _arg205;
                                    _arg205
                                };
                                break 'switch7;
                            }
                            14 => {
                                {
                                    let _arg206 = (((b as i8) as i32) as u32);
                                    self.r[(n as u64) as usize] = _arg206;
                                    _arg206
                                };
                                break 'switch7;
                            }
                            15 => {
                                {
                                    let _arg207 = (((b as i16) as i32) as u32);
                                    self.r[(n as u64) as usize] = _arg207;
                                    _arg207
                                };
                                break 'switch7;
                            }
                            _ => {}
                        }
                    }
                    break 'switch1;
                }
                7 => {
                    {
                        let _arg208 = (a).wrapping_add((imm as u32));
                        self.r[(n as u64) as usize] = _arg208;
                        _arg208
                    };
                    break 'switch1;
                }
                8 => {
                    'switch8: {
                        match n {
                            0 => {
                                {
                                    let _arg209 = (b).wrapping_add(d);
                                    let _arg210 = self.r[(0i32 as u64) as usize];
                                    let _arg211 = (1i32 as u32);
                                    self.wr(bus, _arg209, _arg210, _arg211)
                                };
                                break 'switch8;
                            }
                            1 => {
                                {
                                    let _arg212 = (b).wrapping_add((2i32 as u32).wrapping_mul(d));
                                    let _arg213 = self.r[(0i32 as u64) as usize];
                                    let _arg214 = (2i32 as u32);
                                    self.wr(bus, _arg212, _arg213, _arg214)
                                };
                                break 'switch8;
                            }
                            4 => {
                                {
                                    let _arg217 = {
                                        let _arg215 = (b).wrapping_add(d);
                                        let _arg216 = (1i32 as u32);
                                        self.rd(bus, _arg215, _arg216)
                                    };
                                    self.r[(0i32 as u64) as usize] = _arg217;
                                    _arg217
                                };
                                break 'switch8;
                            }
                            5 => {
                                {
                                    let _arg220 = {
                                        let _arg218 =
                                            (b).wrapping_add((2i32 as u32).wrapping_mul(d));
                                        let _arg219 = (2i32 as u32);
                                        self.rd(bus, _arg218, _arg219)
                                    };
                                    self.r[(0i32 as u64) as usize] = _arg220;
                                    _arg220
                                };
                                break 'switch8;
                            }
                            8 => {
                                {
                                    let _arg221 = (self.r[(0i32 as u64) as usize] == (imm as u32));
                                    self.set_t(_arg221)
                                };
                                break 'switch8;
                            }
                            9 | 11 => {
                                if in_delay {
                                    self.failure("Branch in delay slot");
                                }
                                if (({ self.t() } as i32) == ((n == (9i32 as u32)) as i32)) {
                                    {
                                        {
                                            let _arg222 = ((here).wrapping_add((4i32 as u32)))
                                                .wrapping_add(((imm).wrapping_mul(2i32) as u32));
                                            self.pc = _arg222;
                                            _arg222
                                        };
                                        {
                                            let _arg223 = (3i32 as u32);
                                            cost = _arg223;
                                            _arg223
                                        };
                                    }
                                }
                                break 'switch8;
                            }
                            13 | 15 => {
                                if in_delay {
                                    self.failure("Branch in delay slot");
                                }
                                if (({ self.t() } as i32) == ((n == (13i32 as u32)) as i32)) {
                                    self.jump(
                                        ((here).wrapping_add((4i32 as u32)))
                                            .wrapping_add(((imm).wrapping_mul(2i32) as u32)),
                                        &mut cost,
                                        in_delay,
                                    );
                                }
                                break 'switch8;
                            }
                            _ => {
                                {
                                    let _arg224 = false;
                                    decoded = _arg224;
                                    _arg224
                                };
                                break 'switch8;
                            }
                        }
                    }
                    break 'switch1;
                }
                9 => {
                    {
                        let _arg227 = {
                            let _arg225 = (literal_pc)
                                .wrapping_add(((2i32).wrapping_mul(((op as i32) & 255i32)) as u32));
                            let _arg226 = (2i32 as u32);
                            self.rd(bus, _arg225, _arg226)
                        };
                        self.r[(n as u64) as usize] = _arg227;
                        _arg227
                    };
                    break 'switch1;
                }
                10 | 11 => {
                    let mut displacement: i32 = ((op as i32) & 4095i32);
                    if ((displacement & 2048i32) != 0) {
                        {
                            let _arg228 = (displacement).wrapping_sub((4096i32 as i32));
                            displacement = _arg228;
                            _arg228
                        };
                    }
                    if ((op as i32).wrapping_shr(12i32 as u32) == 11i32) {
                        {
                            let _arg229 = (here).wrapping_add((4i32 as u32));
                            self.pr = _arg229;
                            _arg229
                        };
                    }
                    self.jump(
                        ((here).wrapping_add((4i32 as u32)))
                            .wrapping_add(((displacement).wrapping_mul(2i32) as u32)),
                        &mut cost,
                        in_delay,
                    );
                    break 'switch1;
                }
                12 => {
                    'switch9: {
                        match n {
                            0 => {
                                {
                                    let _arg230 =
                                        (self.gbr).wrapping_add((((op as i32) & 255i32) as u32));
                                    let _arg231 = self.r[(0i32 as u64) as usize];
                                    let _arg232 = (1i32 as u32);
                                    self.wr(bus, _arg230, _arg231, _arg232)
                                };
                                break 'switch9;
                            }
                            1 => {
                                {
                                    let _arg233 = (self.gbr).wrapping_add(
                                        ((2i32).wrapping_mul(((op as i32) & 255i32)) as u32),
                                    );
                                    let _arg234 = self.r[(0i32 as u64) as usize];
                                    let _arg235 = (2i32 as u32);
                                    self.wr(bus, _arg233, _arg234, _arg235)
                                };
                                break 'switch9;
                            }
                            2 => {
                                {
                                    let _arg236 = (self.gbr).wrapping_add(
                                        ((4i32).wrapping_mul(((op as i32) & 255i32)) as u32),
                                    );
                                    let _arg237 = self.r[(0i32 as u64) as usize];
                                    let _arg238 = (4i32 as u32);
                                    self.wr(bus, _arg236, _arg237, _arg238)
                                };
                                break 'switch9;
                            }
                            3 => {
                                if in_delay {
                                    self.failure("TRAPA in delay slot");
                                }
                                bus.write32(
                                    4294967248u32,
                                    (((op as i32) & 255i32).wrapping_shl(2i32 as u32) as u32),
                                );
                                self.exception(bus, (352i32 as u32), self.pc, false);
                                {
                                    let _arg239 = (8i32 as u32);
                                    cost = _arg239;
                                    _arg239
                                };
                                break 'switch9;
                            }
                            4 => {
                                {
                                    let _arg242 = {
                                        let _arg240 = (self.gbr)
                                            .wrapping_add((((op as i32) & 255i32) as u32));
                                        let _arg241 = (1i32 as u32);
                                        self.rd(bus, _arg240, _arg241)
                                    };
                                    self.r[(0i32 as u64) as usize] = _arg242;
                                    _arg242
                                };
                                break 'switch9;
                            }
                            5 => {
                                {
                                    let _arg245 = {
                                        let _arg243 = (self.gbr).wrapping_add(
                                            ((2i32).wrapping_mul(((op as i32) & 255i32)) as u32),
                                        );
                                        let _arg244 = (2i32 as u32);
                                        self.rd(bus, _arg243, _arg244)
                                    };
                                    self.r[(0i32 as u64) as usize] = _arg245;
                                    _arg245
                                };
                                break 'switch9;
                            }
                            6 => {
                                {
                                    let _arg248 = {
                                        let _arg246 = (self.gbr).wrapping_add(
                                            ((4i32).wrapping_mul(((op as i32) & 255i32)) as u32),
                                        );
                                        let _arg247 = (4i32 as u32);
                                        self.rd(bus, _arg246, _arg247)
                                    };
                                    self.r[(0i32 as u64) as usize] = _arg248;
                                    _arg248
                                };
                                break 'switch9;
                            }
                            7 => {
                                {
                                    let _arg249 = (literal_pc & (!3u32)).wrapping_add(
                                        ((4i32).wrapping_mul(((op as i32) & 255i32)) as u32),
                                    );
                                    self.r[(0i32 as u64) as usize] = _arg249;
                                    _arg249
                                };
                                break 'switch9;
                            }
                            8 => {
                                {
                                    let _arg250 = ((self.r[(0i32 as u64) as usize]
                                        & (((op as i32) & 255i32) as u32))
                                        == (0i32 as u32));
                                    self.set_t(_arg250)
                                };
                                break 'switch9;
                            }
                            9 => {
                                {
                                    let _arg251 = (self.r[(0i32 as u64) as usize]
                                        & ((((op as i32) & 255i32) as u32) as u32));
                                    self.r[(0i32 as u64) as usize] = _arg251;
                                    _arg251
                                };
                                break 'switch9;
                            }
                            10 => {
                                {
                                    let _arg252 = (self.r[(0i32 as u64) as usize]
                                        ^ ((((op as i32) & 255i32) as u32) as u32));
                                    self.r[(0i32 as u64) as usize] = _arg252;
                                    _arg252
                                };
                                break 'switch9;
                            }
                            11 => {
                                {
                                    let _arg253 = (self.r[(0i32 as u64) as usize]
                                        | ((((op as i32) & 255i32) as u32) as u32));
                                    self.r[(0i32 as u64) as usize] = _arg253;
                                    _arg253
                                };
                                break 'switch9;
                            }
                            _ => {
                                let mut address: u32 =
                                    (self.gbr).wrapping_add(self.r[(0i32 as u64) as usize]);
                                let mut value: u8 = bus.read8(address);
                                if (n == (12i32 as u32)) {
                                    {
                                        let _arg254 =
                                            (((value as i32) & ((op as i32) & 255i32)) == 0i32);
                                        self.set_t(_arg254)
                                    };
                                } else {
                                    if (n == (13i32 as u32)) {
                                        bus.write8(
                                            address,
                                            (((value as i32) & ((op as i32) & 255i32)) as u8),
                                        );
                                    } else {
                                        if (n == (14i32 as u32)) {
                                            bus.write8(
                                                address,
                                                (((value as i32) ^ ((op as i32) & 255i32)) as u8),
                                            );
                                        } else {
                                            bus.write8(
                                                address,
                                                (((value as i32) | ((op as i32) & 255i32)) as u8),
                                            );
                                        }
                                    }
                                }
                                {
                                    let _arg255 = (3i32 as u32);
                                    cost = _arg255;
                                    _arg255
                                };
                                break 'switch9;
                            }
                        }
                    }
                    break 'switch1;
                }
                13 => {
                    {
                        let _arg258 = {
                            let _arg256 = (literal_pc & (!3u32))
                                .wrapping_add(((4i32).wrapping_mul(((op as i32) & 255i32)) as u32));
                            let _arg257 = (4i32 as u32);
                            self.rd(bus, _arg256, _arg257)
                        };
                        self.r[(n as u64) as usize] = _arg258;
                        _arg258
                    };
                    break 'switch1;
                }
                14 => {
                    {
                        let _arg259 = (imm as u32);
                        self.r[(n as u64) as usize] = _arg259;
                        _arg259
                    };
                    break 'switch1;
                }
                _ => {
                    {
                        let _arg260 = false;
                        decoded = _arg260;
                        _arg260
                    };
                    break 'switch1;
                }
            }
        }
        if (!decoded) {
            self.unsupported(op);
        }
        if in_delay {
            {
                let _arg261 = target;
                self.pc = _arg261;
                _arg261
            };
        }
        {
            let _arg262 = self.steps;
            self.steps = _arg262.wrapping_add(1);
            self.steps
        };
        {
            let _arg263 = (self.cycles).wrapping_add(((cost as u64) as u64));
            self.cycles = _arg263;
            _arg263
        };
        bus.tick(cost);
    }
}
