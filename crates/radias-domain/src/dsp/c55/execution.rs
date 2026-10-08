use super::{C55, CompactPacket, CoreState};

impl C55 {
    pub(super) fn core_state(&self) -> CoreState {
        CoreState {
            ac: self.ac,
            t: self.t,
            st: self.st,
            xar: self.xar,
            pc: self.pc,
            sp: self.sp,
            ssp: self.ssp,
            dp: self.dp,
            cdp: self.cdp,
            repeat_pc: self.repeat_pc,
            block_start: self.block_start,
            block_end: self.block_end,
            repeat_left: self.repeat_left,
            dbstat: self.dbstat,
            block_active: self.block_active,
            repeat_active: self.repeat_active,
            sleeping: self.sleeping,
            loop_type: self.loop_type,
        }
    }
    pub(super) fn set_core_state(&mut self, s: CoreState) {
        self.ac = s.ac;
        self.t = s.t;
        self.st = s.st;
        self.xar = s.xar;
        self.pc = s.pc;
        self.sp = s.sp;
        self.ssp = s.ssp;
        self.dp = s.dp;
        self.cdp = s.cdp;
        self.repeat_pc = s.repeat_pc;
        self.block_start = s.block_start;
        self.block_end = s.block_end;
        self.repeat_left = s.repeat_left;
        self.dbstat = s.dbstat;
        self.block_active = s.block_active;
        self.repeat_active = s.repeat_active;
        self.sleeping = s.sleeping;
        self.loop_type = s.loop_type;
    }
    pub(super) fn loop_context(&self) -> u8 {
        let mut code = 0;
        if self.loop_type[0] != 0 {
            if self.loop_type[1] == 0 {
                code = if self.loop_type[0] == 2 { 3 } else { 2 };
            } else if self.loop_type[0] == 1 {
                code = if self.loop_type[1] == 2 { 8 } else { 7 };
            } else if self.loop_type[1] == 2 {
                code = 9;
            } else {
                self.unsupported("external block nested in local block");
            }
        }
        code | if self.repeat_active { 128 } else { 0 }
    }
    pub(super) fn set_loop_context(&mut self, v: u8) {
        self.loop_type = match v & 15 {
            0 => [0, 0],
            2 => [1, 0],
            3 => [2, 0],
            7 => [1, 1],
            8 => [1, 2],
            9 => [2, 2],
            _ => self.unsupported("reserved CFCT block context"),
        };
        if v & 0x40 != 0 {
            self.unsupported("conditional repeat context");
        }
        self.repeat_active = v & 128 != 0;
        self.sync_loop_view();
    }
    pub(super) fn begin_block(&mut self, start: u32, end: u32, local: bool) {
        let level = usize::from(self.loop_type[0] != 0);
        if self.st[1] & 0x20 != 0 && level != 0 {
            self.unsupported("nested repeat in C54x compatibility mode");
        }
        if self.loop_type[level] != 0 {
            self.unsupported("third level of block repeat");
        }
        self.loop_type[level] = if local { 2 } else { 1 };
        let base = 0x3c + level as u32 * 4;
        self.write(base, (start >> 16) as u16);
        self.write(base + 1, start as u16);
        self.write(base + 2, (end >> 16) as u16);
        self.write(base + 3, end as u16);
        if level != 0 {
            self.write(0x39, self.memory[0x3a]);
        }
        self.sync_loop_view();
    }
    pub(super) fn call(&mut self, target: u32, retaddr: u32, far: bool) {
        if far && self.st[1] & 0x20 != 0 {
            self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_sub(1) & 0xffff);
            self.write(self.sp, retaddr as u16);
            self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_sub(1) & 0xffff);
            self.write(self.sp, ((retaddr >> 16) & 255) as u16);
            self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_sub(2) & 0xffff);
            self.pc = target & 0xffffff;
            self.repeat_active = false;
            self.sync_loop_view();
            return;
        }
        let context = (self.loop_context() as u16) << 8;
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_sub(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_sub(1) & 0xffff);
        self.write(self.sp, retaddr as u16);
        self.write(self.ssp, context | ((retaddr >> 16) as u16 & 255));
        self.pc = target & 0xffffff;
        if self.st[1] & 0x20 == 0 {
            self.loop_type.fill(0);
        }
        self.repeat_active = false;
        self.sync_loop_view();
    }
    pub(super) fn ret(&mut self, far: bool) {
        if far && self.st[1] & 0x20 != 0 {
            let low_address = (self.sp & 0x7f0000) | (self.sp.wrapping_add(1) & 0xffff);
            let target = ((self.read(self.sp) as u32 & 255) << 16) | self.read(low_address) as u32;
            self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_add(2) & 0xffff);
            self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_add(2) & 0xffff);
            self.pc = target;
            self.sync_loop_view();
            return;
        }
        let context = self.read(self.ssp);
        let target = self.read(self.sp) as u32 | ((context as u32 & 255) << 16);
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_add(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_add(1) & 0xffff);
        if self.st[1] & 0x20 == 0 {
            self.set_loop_context((context >> 8) as u8);
        } else {
            self.repeat_active = context & 0x8000 != 0;
        }
        if self.repeat_active {
            self.repeat_pc = target;
        }
        self.pc = target;
        self.sync_loop_view();
    }
    pub fn request_interrupt(&mut self, vector: u32) {
        if !(2..=26).contains(&vector) {
            self.unsupported("invalid maskable interrupt vector");
        }
        if vector < 16 {
            self.memory[1] |= 1 << vector;
        } else {
            self.memory[0x46] |= 1 << (vector - 16);
        }
    }
    pub fn serial_frame(&mut self, port: u32) {
        if port >= 3 {
            return;
        }
        let base = 0x2800 + 0x400 * port;
        if self.io_value(base + 4) >> 4 & 3 == 2 {
            self.request_interrupt([5, 6, 12][port as usize]);
        }
        if self.io_value(base + 5) >> 4 & 3 == 2 {
            self.request_interrupt([17, 7, 13][port as usize]);
        }
    }
    pub(super) fn interrupt_entry(&mut self, vector: u32, return_address: u32) {
        if self.st[1] & 0x20 != 0 {
            self.unsupported("interrupt context in C54x compatibility mode");
        }
        let base = (self.read(if (16..24).contains(&vector) {
            0x4a
        } else {
            0x49
        }) as u32)
            << 8;
        let address = base + (vector << 3);
        let target = (self.program(address + 1) as u32) << 16
            | (self.program(address + 2) as u32) << 8
            | self.program(address + 3) as u32;
        self.pair_push(self.st[2], self.st[0]);
        self.pair_push(self.st[1], self.dbstat);
        self.call(target, return_address, false);
        self.st[1] |= 0x800;
        self.st[2] = (self.st[2] | 0x1000) & !0x800;
        self.sleeping = false;
        self.io_set(2, self.io_value(2) & !1);
        self.interrupts += 1;
    }
    pub(super) fn pair_push(&mut self, data: u16, system: u16) {
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_sub(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_sub(1) & 0xffff);
        self.write(self.sp, data);
        self.write(self.ssp, system);
    }
    pub(super) fn interrupt_return(&mut self) {
        self.ret(false);
        let saved_st1 = self.read(self.sp);
        let saved_debug = self.read(self.ssp);
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_add(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_add(1) & 0xffff);
        let saved_st2 = self.read(self.sp);
        let saved_st0 = self.read(self.ssp);
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_add(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_add(1) & 0xffff);
        self.st = [saved_st0, saved_st1, saved_st2, self.st[3]];
        self.dbstat = saved_debug;
        self.sync_loop_view();
    }
    pub(super) fn service_interrupt(&mut self) {
        if self.st[1] & 0x800 != 0 {
            return;
        }
        if self.memory[1] & self.memory[0] == 0 && self.memory[0x46] & self.memory[0x45] == 0 {
            return;
        }
        for vector in [
            24, 2, 16, 3, 4, 5, 17, 6, 7, 8, 18, 9, 10, 11, 19, 12, 13, 20, 21, 14, 15, 22, 23, 25,
            26,
        ] {
            let flags = if vector < 16 { 1 } else { 0x46 };
            let enable = if vector < 16 { 0 } else { 0x45 };
            let bit = if vector < 16 { vector } else { vector - 16 };
            if self.memory[flags] & self.memory[enable] & (1 << bit) == 0 {
                continue;
            }
            self.interrupt_entry(vector, self.pc);
            self.memory[flags] &= !(1 << bit);
            return;
        }
    }
    pub(super) fn compact_packet(&self, address: u32) -> CompactPacket {
        let mut packet = CompactPacket::default();
        let header = self.program(address);
        let a = self.program(address + 1);
        let b = self.program(address + 2);
        let peer = self.program(address + 3);
        let size = |opcode: u8| {
            if !(0xa0..=0xfd).contains(&opcode) {
                self.unsupported("compact memory opcode family");
            }
            if opcode <= 0xcf {
                2
            } else if opcode <= 0xef {
                3
            } else {
                4
            }
        };
        let smem = |field: u32| {
            (((field >> 3) << 5) | ([0u32, 1, 2, 3, 9, 4, 10, 5][(field & 7) as usize] << 1) | 1)
                as u8
        };
        packet.first[0] = 0x80 | ((header & 7) << 4) | (b & 15);
        packet.first_size = size(packet.first[0]);
        let mut payload = address + 4;
        if peer & 128 != 0 {
            packet.dual_addressing = true;
            let fields = ((a as u32) << 8) | b as u32;
            packet.first[1] = smem(fields >> 10);
            packet.second[0] = peer;
            packet.second[1] = smem(fields >> 4 & 63);
            packet.second_size = size(peer);
            for i in 2..packet.first_size {
                packet.first[i as usize] = self.program(payload);
                payload += 1;
            }
            for i in 2..packet.second_size {
                packet.second[i as usize] = self.program(payload);
                payload += 1;
            }
        } else {
            packet.first[1] = a;
            if (a & 31 == 0x11 && (a >> 5 <= 2 || a >> 5 >= 6))
                || (a & 1 != 0 && (a >> 1 & 15 == 6 || a >> 1 & 15 == 7))
            {
                self.unsupported("extended address in compact packet");
            }
            for i in 2..packet.first_size {
                packet.first[i as usize] = self.program(payload);
                payload += 1;
            }
            packet.second[0] = (peer & 0xf0) | 4;
            packet.second[1] = self.program(payload);
            packet.second[2] = (b & 0xf0) | (peer & 15);
            packet.second_size = 3;
            if packet.second[0] != 0x14 && packet.second[0] != 0x04 {
                self.unsupported("compact MAR/program-control opcode family");
            }
        }
        if packet.first_size + packet.second_size > 6 {
            self.unsupported("compact packet longer than six bytes");
        }
        packet
    }
    pub(super) fn instruction_size(&self, a: u32) -> u32 {
        let op = self.program(a);
        let mut smem = false;
        if (0x88..=0x8f).contains(&op) {
            let p = self.compact_packet(a);
            return p.first_size + p.second_size;
        }
        let mut n = match op {
            0..=0x1f => {
                if op == 0xa || op == 0xb {
                    1
                } else {
                    3
                }
            }
            0x20..=0x21 => 1,
            0x22..=0x67 => 2,
            0x68..=0x69 => 5,
            0x6a..=0x7f => 4,
            0x80..=0x81 => 3,
            0x82..=0x87 => 4,
            0x90..=0x96 | 0x9e | 0x9f => 2,
            0xa0..=0xcf => {
                smem = true;
                2
            }
            0xd0..=0xef => {
                smem = true;
                3
            }
            0xf0..=0xfd => {
                smem = true;
                4
            }
            _ => 1,
        };
        if smem {
            let m = self.program(a + 1);
            if m & 31 == 0x11 {
                let base = m >> 5;
                if base == 1 {
                    n += 3;
                } else if base <= 2 || base >= 6 {
                    n += 2;
                }
            } else if m & 1 != 0 && (m >> 1 & 15 == 6 || m >> 1 & 15 == 7) {
                n += 2;
            }
        }
        if smem || (0x80..=0x87).contains(&op) {
            let q = self.program(a + n);
            if (0x98..=0x9a).contains(&q) {
                n += 1;
            }
        }
        n
    }
    pub(super) fn packet_size(&self, a: u32) -> u32 {
        let first = self.instruction_size(a);
        if (0x88..=0x8f).contains(&self.program(a)) {
            return first;
        }
        let second = self.program(a + first);
        first
            + if self.parallel_opcode(second)
                || second == 0x9c
                || second == 0x9d
                || second == 0x9e
                || second == 0x9f
            {
                self.instruction_size(a + first)
            } else {
                0
            }
    }
    pub(super) fn dual_addresses(
        &mut self,
        fields: u16,
        units: u32,
        circular: bool,
        linear: bool,
    ) -> [u32; 2] {
        let encoded = [(fields as u32 >> 10) & 63, (fields as u32 >> 4) & 63];
        let mut addresses = [0; 2];
        let mut delta = [0i32; 2];
        let mut force = [false; 2];
        let before = self.xar;
        let index = if self.st[1] & 0x20 != 0 {
            before[0] as u16 as i16 as i32
        } else {
            self.t[0] as i16 as i32
        };
        for i in 0..2 {
            let n = encoded[i] >> 3;
            let mode = encoded[i] & 7;
            force[i] = circular && !(self.st[1] & 0x20 != 0 && (mode < 3 || mode == 7));
            addresses[i] = before[n as usize];
            match mode {
                1 => delta[i] = units as i32,
                2 => delta[i] = -(units as i32),
                3 => delta[i] = index,
                4 => delta[i] = self.t[1] as i16 as i32,
                5 => delta[i] = -index,
                6 => delta[i] = -(self.t[1] as i16 as i32),
                7 => {
                    addresses[i] =
                        self.pointer_offset(n, before[n as usize], index, force[i], linear)
                }
                _ => {}
            }
            addresses[i] = self.pointer_address(n, addresses[i], force[i], linear);
        }
        if encoded[0] >> 3 == encoded[1] >> 3 && delta[0] != 0 && delta[1] != 0 {
            self.unsupported("dual address-register modification conflict");
        }
        for i in 0..2 {
            if delta[i] != 0 {
                let n = encoded[i] >> 3;
                self.xar[n as usize] =
                    self.pointer_offset(n, before[n as usize], delta[i], force[i], linear);
            }
        }
        addresses
    }
    pub fn step(&mut self) {
        if self.reset_asserted || !self.started || !self.fault.is_empty() {
            return;
        }
        self.last_pc = self.pc;
        self.serial_tx_tick();
        self.dma_tick();
        self.service_interrupt();
        if self.sleeping {
            return;
        }
        let initial = self.core_state();
        let initial_brc0 = self.memory[0x1a];
        let initial_brc1 = self.memory[0x39];
        let mut mmr = None;
        self.pending.clear();
        self.last_pc = self.pc;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let first_pc = self.pc;
            let compact = (0x88..=0x8f).contains(&self.program(self.pc));
            let decoded = if compact {
                self.compact_packet(self.pc)
            } else {
                CompactPacket::default()
            };
            let first_size = if compact {
                decoded.first_size
            } else {
                self.instruction_size(self.pc)
            };
            let second_pc = self.pc + first_size;
            let second_opcode = if compact {
                decoded.second[0]
            } else {
                self.program(second_pc)
            };
            let paired_xcc = second_opcode == 0x9e || second_opcode == 0x9f;
            let circular = !compact && second_opcode == 0x9d;
            let opcode = self.program(first_pc);
            let modifier = !compact && second_opcode == 0x9c;
            let far = modifier
                && (matches!(opcode, 0x6a | 0x91 | 0x6c | 0x92)
                    || (opcode == 0x48 && self.program(first_pc + 1) == 4));
            let linear = modifier && !far;
            let pair = compact
                || self.parallel_opcode(second_opcode)
                || paired_xcc
                || circular
                || modifier;
            let second_size = if compact {
                decoded.second_size
            } else if pair {
                self.instruction_size(second_pc)
            } else {
                0
            };
            let sequential = second_pc + second_size;
            let mut next = sequential;
            if pair && first_size + second_size > 6 {
                self.unsupported("parallel packet longer than six bytes");
            }
            if circular || linear {
                if !((0x80..=0x87).contains(&opcode) || (0xa0..=0xfd).contains(&opcode)) {
                    self.unsupported("address qualifier requires indirect memory addressing");
                }
                if (0x82..=0x84).contains(&opcode) {
                    self.unsupported("address qualifier on parallel coefficient instruction");
                }
            }
            if self.repeat_active && first_pc == self.repeat_pc {
                if self.repeat_left != 0 {
                    self.repeat_left -= 1;
                    next = first_pc;
                } else {
                    self.repeat_active = false;
                }
            }
            for level in (0..2).rev() {
                if self.loop_type[level] != 0 {
                    let end = self.loop_address(level as u32, true);
                    if end != first_pc && (!pair || end != second_pc) {
                        continue;
                    }
                    let counter = if level != 0 { 0x39 } else { 0x1a };
                    if self.memory[counter] != 0 {
                        self.memory[counter] -= 1;
                        next = self.loop_address(level as u32, false);
                        break;
                    }
                    self.loop_type[level] = 0;
                }
            }
            self.sync_loop_view();
            let input = self.core_state();
            self.in_packet = true;
            if paired_xcc && self.program(second_pc + 1) & 128 != 0 {
                self.unsupported("parallel XCCPART address-phase execution");
            }
            let first_enabled =
                second_opcode != 0x9e || !pair || self.condition(self.program(second_pc + 1));
            if first_enabled {
                if far && self.program(first_pc) != 0x48 {
                    self.braf(false);
                }
                self.execute_one(
                    first_pc,
                    next,
                    sequential,
                    compact,
                    decoded.first,
                    decoded.dual_addressing,
                    circular,
                    far,
                    linear,
                );
            } else {
                self.pc = next;
            }
            if pair && !circular && !modifier {
                let first = self.core_state();
                self.set_core_state(input);
                if second_opcode == 0x9e {
                    self.pc = next;
                } else {
                    self.execute_one(
                        second_pc,
                        next,
                        sequential,
                        compact,
                        decoded.second,
                        decoded.dual_addressing,
                        false,
                        false,
                        false,
                    );
                }
                let second = self.core_state();
                let mut merged = first;
                macro_rules! merge {
                    ($old:expr,$a:expr,$b:expr) => {{
                        let old = $old;
                        let a = $a;
                        let b = $b;
                        if a != old && b != old && a != b {
                            self.unsupported("parallel register write conflict");
                        }
                        if b != old { b } else { a }
                    }};
                }
                for i in 0..4 {
                    merged.ac[i] = merge!(input.ac[i], first.ac[i], second.ac[i]);
                    merged.t[i] = merge!(input.t[i], first.t[i], second.t[i]);
                    let ca = first.st[i] ^ input.st[i];
                    let cb = second.st[i] ^ input.st[i];
                    merged.st[i] =
                        (input.st[i] & !(ca | cb)) | (first.st[i] & ca) | (second.st[i] & cb);
                }
                for i in 0..8 {
                    merged.xar[i] = merge!(input.xar[i], first.xar[i], second.xar[i]);
                }
                merged.pc = merge!(next, first.pc, second.pc);
                merged.sp = merge!(input.sp, first.sp, second.sp);
                merged.ssp = merge!(input.ssp, first.ssp, second.ssp);
                merged.dp = merge!(input.dp, first.dp, second.dp);
                merged.cdp = merge!(input.cdp, first.cdp, second.cdp);
                merged.repeat_pc = merge!(input.repeat_pc, first.repeat_pc, second.repeat_pc);
                merged.repeat_left =
                    merge!(input.repeat_left, first.repeat_left, second.repeat_left);
                merged.repeat_active = merge!(
                    input.repeat_active,
                    first.repeat_active,
                    second.repeat_active
                );
                merged.dbstat = merge!(input.dbstat, first.dbstat, second.dbstat);
                for i in 0..2 {
                    merged.loop_type[i] =
                        merge!(input.loop_type[i], first.loop_type[i], second.loop_type[i]);
                }
                self.set_core_state(merged);
            }
            self.in_packet = false;
            if !self.pending.is_empty() {
                let mut before = [0u16; 0x60];
                before.copy_from_slice(&self.memory[..0x60]);
                before[0x1a] = initial_brc0;
                before[0x39] = initial_brc1;
                mmr = Some(before);
            }
            let pending = std::mem::take(&mut self.pending);
            for w in pending {
                if w.read_clear {
                    self.io_set(w.address, w.value);
                } else if w.peripheral {
                    self.write_io(w.address as u16, w.value);
                } else {
                    self.write(w.address, w.value);
                }
            }
            self.sync_loop_view();
            self.steps += if pair { 2 } else { 1 };
        }));
        if let Err(error) = result {
            self.in_packet = false;
            self.pending.clear();
            self.set_core_state(initial);
            if let Some(before) = mmr {
                self.memory[..0x60].copy_from_slice(&before);
            } else {
                self.memory[0x1a] = initial_brc0;
                self.memory[0x39] = initial_brc1;
            }
            std::panic::resume_unwind(error);
        }
    }
    pub fn run(&mut self, count: u32) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..count {
                if !self.started || !self.fault.is_empty() {
                    break;
                }
                self.step();
            }
        }));
        if let Err(error) = result {
            self.fault = if let Some(message) = error.downcast_ref::<String>() {
                message.clone()
            } else if let Some(message) = error.downcast_ref::<&str>() {
                message.to_string()
            } else {
                "C55 Rust execution panic".to_string()
            };
        }
    }
}
