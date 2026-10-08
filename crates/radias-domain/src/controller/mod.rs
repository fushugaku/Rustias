//! SH3 controller and bus port. Board devices implement the port separately.
mod generated;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Interrupt {
    pub level: u32,
    pub event: u32,
    pub event2: u32,
}
pub trait Bus {
    fn read8(&mut self, address: u32) -> u8;
    fn write8(&mut self, address: u32, value: u8);
    fn tick(&mut self, _cycles: u32) {}
    fn interrupt(&self) -> Interrupt {
        Interrupt::default()
    }
    fn event(&mut self, _event: u32, _irq: bool) {}
    fn event2(&mut self, _event: u32) {}
    fn read16(&mut self, a: u32) -> u16 {
        let high = self.read8(a) as u16;
        high << 8 | self.read8(a.wrapping_add(1)) as u16
    }
    fn read32(&mut self, a: u32) -> u32 {
        let high = self.read16(a) as u32;
        high << 16 | self.read16(a.wrapping_add(2)) as u32
    }
    fn write16(&mut self, a: u32, v: u16) {
        self.write8(a, (v >> 8) as u8);
        self.write8(a.wrapping_add(1), v as u8);
    }
    fn write32(&mut self, a: u32, v: u32) {
        self.write16(a, (v >> 16) as u16);
        self.write16(a.wrapping_add(2), v as u16);
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sh3 {
    pub r: [u32; 16],
    pub bank: [u32; 8],
    pub pc: u32,
    pub sr: u32,
    pub gbr: u32,
    pub vbr: u32,
    pub pr: u32,
    pub mach: u32,
    pub macl: u32,
    pub ssr: u32,
    pub spc: u32,
    pub steps: u64,
    pub cycles: u64,
    pub interrupts: u64,
    pub sleeping: bool,
    pub delayed: bool,
    pub restore_sr: bool,
    pub delayed_pc: u32,
    pub delayed_sr: u32,
    pub last_pc: u32,
    pub last_op: u16,
}
impl Default for Sh3 {
    fn default() -> Self {
        Self {
            r: [0; 16],
            bank: [0; 8],
            pc: 0xa0000000,
            sr: 0x700000f0,
            gbr: 0,
            vbr: 0,
            pr: 0,
            mach: 0,
            macl: 0,
            ssr: 0,
            spc: 0,
            steps: 0,
            cycles: 0,
            interrupts: 0,
            sleeping: false,
            delayed: false,
            restore_sr: false,
            delayed_pc: 0,
            delayed_sr: 0,
            last_pc: 0,
            last_op: 0,
        }
    }
}
impl Sh3 {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn t(&self) -> bool {
        self.sr & 1 != 0
    }
    pub fn set_t(&mut self, v: bool) {
        self.sr = (self.sr & !1) | u32::from(v);
    }
    pub fn set_sr(&mut self, value: u32) {
        let value = value & 0x700003f3;
        let old = self.sr & 0x60000000 == 0x60000000;
        let new = value & 0x60000000 == 0x60000000;
        if old != new {
            for i in 0..8 {
                std::mem::swap(&mut self.r[i], &mut self.bank[i]);
            }
        }
        self.sr = value;
    }
    pub fn exception(&mut self, bus: &mut impl Bus, event: u32, saved_pc: u32, irq: bool) {
        self.spc = saved_pc;
        self.ssr = self.sr;
        self.set_sr(self.sr | 0x70000000);
        bus.event(event, irq);
        self.pc = self.vbr + if irq { 0x600 } else { 0x100 };
        self.sleeping = false;
        self.delayed = false;
        if irq {
            self.interrupts += 1;
        }
    }
    fn control(&self, c: u32) -> u32 {
        match c {
            0 => self.sr,
            1 => self.gbr,
            2 => self.vbr,
            3 => self.ssr,
            4 => self.spc,
            8..=15 => self.bank[c as usize - 8],
            _ => self.failure("Unsupported SH-3 control register"),
        }
    }
    fn set_control(&mut self, c: u32, v: u32) {
        match c {
            0 => self.set_sr(v),
            1 => self.gbr = v,
            2 => self.vbr = v,
            3 => self.ssr = v,
            4 => self.spc = v,
            8..=15 => self.bank[c as usize - 8] = v,
            _ => self.failure("Unsupported SH-3 control register"),
        }
    }
    fn system(&self, c: u32) -> u32 {
        match c {
            0 => self.mach,
            1 => self.macl,
            2 => self.pr,
            _ => self.failure("Unsupported SH-3 system register"),
        }
    }
    fn set_system(&mut self, c: u32, v: u32) {
        match c {
            0 => self.mach = v,
            1 => self.macl = v,
            2 => self.pr = v,
            _ => self.failure("Unsupported SH-3 system register"),
        }
    }
    fn branch(&mut self, a: u32) {
        if self.delayed {
            self.failure("Branch instruction in a delay slot");
        }
        self.delayed = true;
        self.delayed_pc = a;
    }
    fn jump(&mut self, a: u32, cost: &mut u32, in_delay: bool) {
        if in_delay {
            self.failure("Illegal delay-slot branch");
        }
        self.branch(a);
        *cost = 2;
    }
    fn post(&mut self, n: u32, size: u32) -> u32 {
        let a = self.r[n as usize];
        self.r[n as usize] = a.wrapping_add(size);
        a
    }
    fn rd(&mut self, bus: &mut impl Bus, a: u32, size: u32) -> u32 {
        if a & (size - 1) != 0 {
            self.failure("Unaligned SH-3 data read");
        }
        match size {
            1 => bus.read8(a) as i8 as i32 as u32,
            2 => bus.read16(a) as i16 as i32 as u32,
            _ => bus.read32(a),
        }
    }
    fn wr(&mut self, bus: &mut impl Bus, a: u32, v: u32, size: u32) {
        if a & (size - 1) != 0 {
            self.failure(&format!(
                "Unaligned {size}-byte write to {a:08x} at PC {:08x} (opcode {:04x})",
                self.last_pc, self.last_op
            ));
        }
        match size {
            1 => bus.write8(a, v as u8),
            2 => bus.write16(a, v as u16),
            _ => bus.write32(a, v),
        }
    }
    #[cold]
    fn unsupported(&self, op: u16) -> ! {
        self.failure(&format!(
            "Unsupported/illegal SH-3 opcode {op:04x} at {:08x}",
            self.last_pc
        ))
    }
    #[cold]
    fn failure(&self, message: &str) -> ! {
        std::panic::panic_any(message.to_string());
    }
}
