//! Functional VC5502 interpreter port. Unknown instructions stop execution.
//! Arithmetic/operation dispatch is generated from the independent C++ reference.
use std::collections::{BTreeMap, VecDeque};

mod checkpoint;
mod execution;
mod generated;
mod numeric;
mod peripherals;

const MASK40: u64 = 0xffffffffff;
fn signed40(value: u64) -> i64 {
    ((value << 24) as i64) >> 24
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DmaState {
    pub config: [u16; 16],
    pub source: u32,
    pub target: u32,
    pub source_ready_at: u64,
    pub source_element: u32,
    pub source_frame: u32,
    pub element: u32,
    pub frame: u32,
    pub active: bool,
    pub waiting: bool,
    pub fifo: VecDeque<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WordWrite {
    pub address: u32,
    pub pc: u32,
    pub value: u16,
    pub host: bool,
    pub dma: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCommand {
    pub pc: u32,
    pub words: [u16; 32],
}
#[derive(Clone, Copy)]
struct PendingWrite {
    address: u32,
    value: u16,
    peripheral: bool,
    read_clear: bool,
}
#[derive(Clone, Copy, Default)]
struct CompactPacket {
    first: [u8; 4],
    second: [u8; 4],
    first_size: u32,
    second_size: u32,
    dual_addressing: bool,
}
#[derive(Clone, Copy)]
struct CoreState {
    ac: [u64; 4],
    t: [u16; 4],
    st: [u16; 4],
    xar: [u32; 8],
    pc: u32,
    sp: u32,
    ssp: u32,
    dp: u32,
    cdp: u32,
    repeat_pc: u32,
    block_start: u32,
    block_end: u32,
    repeat_left: u32,
    dbstat: u16,
    block_active: bool,
    repeat_active: bool,
    sleeping: bool,
    loop_type: [u8; 2],
}

pub struct C55 {
    pub memory: Vec<u16>,
    pub io: BTreeMap<u16, u16>,
    pub ac: [u64; 4],
    pub t: [u16; 4],
    pub st: [u16; 4],
    pub xar: [u32; 8],
    pub pc: u32,
    pub sp: u32,
    pub ssp: u32,
    pub dp: u32,
    pub cdp: u32,
    pub hpic: u16,
    pub hpia: u16,
    pub hpi_latch: u16,
    pub steps: u64,
    pub host_words: u64,
    pub interrupts: u64,
    pub dma_elements: u64,
    pub dbstat: u16,
    pub pll_lock_at: u64,
    pub last_pc: u32,
    pub repeat_pc: u32,
    pub block_start: u32,
    pub block_end: u32,
    pub repeat_left: u32,
    pub started: bool,
    pub hint: bool,
    pub sleeping: bool,
    pub reset_asserted: bool,
    pub block_active: bool,
    pub loop_type: [u8; 2],
    pub repeat_active: bool,
    pub simple_packets_enabled: bool,
    pub fault: String,
    pub gpio_pins: u16,
    pub watch_address: u32,
    pub host_pc: u32,
    pub watched_writes: Vec<WordWrite>,
    pub host_commands: Vec<HostCommand>,
    pub observed_writes: Vec<WordWrite>,
    pub observe_stores: bool,
    pub store_observer: Option<Box<dyn FnMut(&WordWrite, &[u16], &[u32; 8], &[u16; 4]) + Send>>,
    pub dma: [DmaState; 6],
    pub dma_clocks: u64,
    dma_unsynchronized: u8,
    serial_tx: [VecDeque<u32>; 3],
    serial_tx_repeat: [u32; 3],
    serial_tx_loaded: u8,
    serial_tx_events: u8,
    in_packet: bool,
    pending: Vec<PendingWrite>,
}

impl Default for C55 {
    fn default() -> Self {
        Self::new()
    }
}
impl C55 {
    pub fn new() -> Self {
        let mut value = Self {
            memory: vec![0; 0x200000],
            io: BTreeMap::new(),
            ac: [0; 4],
            t: [0; 4],
            st: [0; 4],
            xar: [0; 8],
            pc: 0,
            sp: 0x90,
            ssp: 0x80,
            dp: 0,
            cdp: 0,
            hpic: 8,
            hpia: 0,
            hpi_latch: 0,
            steps: 0,
            host_words: 0,
            interrupts: 0,
            dma_elements: 0,
            dbstat: 0,
            pll_lock_at: 0,
            last_pc: 0,
            repeat_pc: 0,
            block_start: 0,
            block_end: 0,
            repeat_left: 0,
            started: false,
            hint: false,
            sleeping: false,
            reset_asserted: false,
            block_active: false,
            loop_type: [0; 2],
            repeat_active: false,
            simple_packets_enabled: true,
            fault: String::new(),
            gpio_pins: 0xc5,
            watch_address: u32::MAX,
            host_pc: 0,
            watched_writes: Vec::new(),
            host_commands: Vec::new(),
            observed_writes: Vec::new(),
            observe_stores: false,
            store_observer: None,
            dma: std::array::from_fn(|_| DmaState::default()),
            dma_clocks: 0,
            dma_unsynchronized: 0,
            serial_tx: std::array::from_fn(|_| VecDeque::new()),
            serial_tx_repeat: [0; 3],
            serial_tx_loaded: 0,
            serial_tx_events: 0,
            in_packet: false,
            pending: Vec::new(),
        };
        value.reset_state(true);
        value
    }
    pub fn reset(&mut self) {
        self.reset_state(true);
    }
    fn reset_state(&mut self, clear: bool) {
        if clear {
            self.memory.fill(0);
        } else {
            self.memory[..0x60].fill(0);
        }
        self.io.clear();
        self.ac.fill(0);
        self.t.fill(0);
        self.xar.fill(0);
        self.st = [0x3800, 0x2920, 0x7000, 0x1c00];
        self.io.extend([
            (0x6c00, 1),
            (0x8800, 0),
            (0x8c00, 0),
            (0x8400, 2),
            (0x3400, 0),
            (0x1c80, 0x48),
            (0x1c82, 11),
            (0x1c88, 0),
            (0x1c8a, 0x8000),
            (0x1c8c, 0x8003),
            (0x1c8e, 0x8003),
            (0x1c90, 0x8003),
            (0x1c92, 0x8003),
            (0x1c98, 31),
            (0xe00, 0),
            (0xe01, 0),
            (1, 0),
            (2, 0),
            (0x9400, 0),
            (0x9401, 0),
            (0x9402, 0),
            (0x9403, 0),
            (0xf, 5),
            (0x3401, 0),
        ]);
        for channel in 0..6 {
            for offset in 0..16 {
                self.io.insert(
                    0xc00 + channel * 0x20 + offset,
                    if offset == 2 { 0x183 } else { 0 },
                );
            }
        }
        for port in 0..3 {
            for offset in 0..=0x12 {
                self.io.insert(0x2800 + port * 0x400 + offset, 0);
            }
        }
        self.dma = std::array::from_fn(|_| DmaState::default());
        self.dma_unsynchronized = 0;
        for q in &mut self.serial_tx {
            q.clear();
        }
        self.serial_tx_repeat.fill(0);
        self.serial_tx_loaded = 0;
        self.serial_tx_events = 0;
        self.pc = 0;
        self.sp = 0x90;
        self.ssp = 0x80;
        self.dp = 0;
        self.cdp = 0;
        self.hpic = 8;
        self.hpia = 0;
        self.hpi_latch = 0;
        self.steps = 0;
        self.host_words = 0;
        self.interrupts = 0;
        self.dma_elements = 0;
        self.pll_lock_at = 0;
        self.dma_clocks = 0;
        self.dbstat = 0;
        self.last_pc = 0;
        self.repeat_pc = 0;
        self.repeat_left = 0;
        self.block_start = 0;
        self.block_end = 0;
        self.started = false;
        self.hint = false;
        self.sleeping = false;
        self.block_active = false;
        self.repeat_active = false;
        self.reset_asserted = false;
        self.loop_type.fill(0);
        self.in_packet = false;
        self.pending.clear();
        self.fault.clear();
        self.watched_writes.clear();
        self.host_commands.clear();
        self.observed_writes.clear();
    }
    pub fn set_reset(&mut self, asserted: bool) {
        if asserted == self.reset_asserted {
            return;
        }
        if asserted {
            self.reset_state(false);
            self.reset_asserted = true;
            return;
        }
        self.reset_asserted = false;
        self.memory[0x60] &= 0xff;
        *self.io.entry(0x3400).or_default() |= 0x10;
        *self.io.entry(0x3401).or_default() &= !0x10;
    }
    pub fn program(&self, address: u32) -> u8 {
        if address as usize / 2 >= self.memory.len() {
            std::panic::panic_any("C55x program fetch outside mapped RAM".to_string());
        }
        (self.memory[address as usize / 2] >> if address & 1 == 0 { 8 } else { 0 }) as u8
    }
    pub fn reg(&self, n: u32) -> u64 {
        if n < 4 {
            self.ac[n as usize]
        } else if n < 8 {
            self.t[n as usize - 4] as u64
        } else {
            (self.xar[n as usize - 8] & 0xffff) as u64
        }
    }
    pub fn set_reg(&mut self, n: u32, v: u64) {
        if n < 4 {
            self.ac[n as usize] = v & MASK40;
        } else if n < 8 {
            self.t[n as usize - 4] = v as u16;
            if n == 6 && self.st[1] & 0x20 != 0 {
                self.st[1] = (self.st[1] & !0x1f) | (v as u16 & 31);
            }
        } else {
            self.xar[n as usize - 8] = (self.xar[n as usize - 8] & 0x7f0000) | (v as u32 & 0xffff);
        }
    }
    pub fn xreg(&self, n: u32) -> u32 {
        match n {
            0..=3 => self.ac[n as usize] as u32 & 0x7fffff,
            4 => self.sp,
            5 => self.ssp,
            6 => self.dp,
            7 => self.cdp,
            _ => self.xar[n as usize - 8],
        }
    }
    pub fn set_xreg(&mut self, n: u32, v: u32) {
        let v = v & 0x7fffff;
        match n {
            0..=3 => self.ac[n as usize] = v as u64,
            4 => self.sp = v,
            5 => self.ssp = v,
            6 => self.dp = v,
            7 => self.cdp = v,
            _ => self.xar[n as usize - 8] = v,
        }
    }
    pub fn read(&mut self, mut a: u32) -> u16 {
        if a as usize >= self.memory.len() {
            self.unsupported(&format!("data-memory device at word {a}"));
        }
        match a {
            2 | 6 => return self.st[0],
            3 | 7 => return self.st[1],
            4 | 0x1d => return self.st[3],
            0x4b => return self.st[2],
            0x10..=0x17 => return self.xar[a as usize - 0x10] as u16,
            0x20..=0x23 => return self.t[a as usize - 0x20],
            0xe => return self.t[3],
            0x18 | 0x4d => return self.sp as u16,
            0x4c => return self.ssp as u16,
            0x4e => return (self.sp >> 16) as u16,
            0x2e => return self.dp as u16,
            0x2b => return (self.dp >> 16) as u16,
            0x27 => return self.cdp as u16,
            0x4f => return (self.cdp >> 16) as u16,
            _ => {}
        }
        let acc = match a {
            8..=13 => Some(((a - 8) / 3, (a - 8) % 3)),
            0x24..=0x26 => Some((2, a - 0x24)),
            0x28..=0x2a => Some((3, a - 0x28)),
            _ => None,
        };
        if let Some((n, p)) = acc {
            return ((self.ac[n as usize] >> (p * 16)) & if p == 2 { 255 } else { 65535 }) as u16;
        }
        if a == 0x44 {
            return self.repeat_left as u16;
        }
        if a == 0x1b {
            a = 0x3d;
        }
        if a == 0x1c {
            a = 0x3f;
        }
        self.memory[a as usize]
    }
    pub fn write(&mut self, mut a: u32, v: u16) {
        if a as usize >= self.memory.len() {
            self.unsupported(&format!("data-memory device at word {a}"));
        }
        if a == 1 || a == 0x46 {
            if self.in_packet {
                self.pending.push(PendingWrite {
                    address: a,
                    value: v,
                    peripheral: false,
                    read_clear: false,
                });
            } else {
                self.memory[a as usize] &= !v;
            }
            return;
        }
        match a {
            2 | 6 => {
                self.st[0] = v;
                return;
            }
            3 | 7 => {
                self.write_st1(v, a == 7);
                return;
            }
            4 | 0x1d => {
                self.st[3] = v;
                return;
            }
            0x4b => {
                self.st[2] = (v & !0x1000) | (self.st[2] & 0x1000);
                return;
            }
            0x10..=0x17 => {
                self.set_reg(8 + a - 0x10, v as u64);
                return;
            }
            0x20..=0x23 => {
                self.set_reg(4 + a - 0x20, v as u64);
                return;
            }
            0xe => {
                self.t[3] = v;
                return;
            }
            0x18 | 0x4d => {
                self.sp = (self.sp & 0x7f0000) | v as u32;
                return;
            }
            0x4c => {
                self.ssp = (self.ssp & 0x7f0000) | v as u32;
                return;
            }
            0x4e => {
                self.sp = (self.sp & 0xffff) | ((v as u32 & 127) << 16);
                self.ssp = (self.ssp & 0xffff) | ((v as u32 & 127) << 16);
                return;
            }
            0x2e => {
                self.dp = (self.dp & 0x7f0000) | v as u32;
                return;
            }
            0x2b => {
                self.dp = (self.dp & 0xffff) | ((v as u32 & 127) << 16);
                return;
            }
            0x27 => {
                self.cdp = (self.cdp & 0x7f0000) | v as u32;
                return;
            }
            0x4f => {
                self.cdp = (self.cdp & 0xffff) | ((v as u32 & 127) << 16);
                return;
            }
            _ => {}
        }
        let acc = match a {
            8..=13 => Some(((a - 8) / 3, (a - 8) % 3)),
            0x24..=0x26 => Some((2, a - 0x24)),
            0x28..=0x2a => Some((3, a - 0x28)),
            _ => None,
        };
        if let Some((n, p)) = acc {
            let mask = (if p == 2 { 255u64 } else { 65535u64 }) << (p * 16);
            self.ac[n as usize] = (self.ac[n as usize] & !mask) | (((v as u64) << (p * 16)) & mask);
            return;
        }
        if a == 0x44 {
            self.repeat_left = v as u32;
            return;
        }
        if a == 0x1b {
            a = 0x3d;
        }
        if a == 0x1c {
            a = 0x3f;
        }
        if self.in_packet {
            self.pending.push(PendingWrite {
                address: a,
                value: v,
                peripheral: false,
                read_clear: false,
            });
            return;
        }
        self.memory[a as usize] = v;
        if a == 0x39 {
            self.memory[0x3a] = v;
        }
        let event = WordWrite {
            address: a,
            pc: self.last_pc,
            value: v,
            host: false,
            dma: false,
        };
        if let Some(observer) = self.store_observer.as_mut() {
            observer(&event, &self.memory, &self.xar, &self.t);
        }
        if self.observe_stores {
            self.observed_writes.push(event);
        }
        if a == self.watch_address {
            self.watched_writes.push(event);
            if self.watched_writes.len() > 96 {
                self.watched_writes.remove(0);
            }
        }
    }
    fn io_value(&self, a: u32) -> u16 {
        self.io.get(&(a as u16)).copied().unwrap_or(0)
    }
    fn io_set(&mut self, a: u32, v: u16) {
        self.io.insert(a as u16, v);
    }
    pub fn read_io(&mut self, a: u16) -> u16 {
        if a == 0xa018 {
            return self.hpic;
        }
        if a == 0xa01a || a == 0xa01c {
            return self.hpia;
        }
        if a == 0x3401 {
            return (self.io_value(a as u32) & self.io_value(0x3400))
                | (self.gpio_pins & !self.io_value(0x3400));
        }
        if a == 0x1c80 && self.pll_lock_at != 0 && self.steps >= self.pll_lock_at {
            self.io_set(a as u32, self.io_value(a as u32) | 0x20);
            self.pll_lock_at = 0;
        }
        if (0xc00..0xcc0).contains(&a) && ((a - 0xc00) & 31) == 3 {
            let v = self.io_value(a as u32);
            if self.in_packet {
                self.pending.push(PendingWrite {
                    address: a as u32,
                    value: 0,
                    peripheral: true,
                    read_clear: true,
                });
            } else {
                self.io_set(a as u32, 0);
            }
            return v;
        }
        if let Some(v) = self.peripheral_read(a) {
            return v;
        }
        match self.io.get(&a) {
            Some(v) => *v,
            None => self.unsupported(&format!("I/O peripheral at port 0x{a:x}")),
        }
    }
    pub fn write_io(&mut self, a: u16, v: u16) {
        if self.in_packet {
            self.pending.push(PendingWrite {
                address: a as u32,
                value: v,
                peripheral: true,
                read_clear: false,
            });
            return;
        }
        if a == 0xa018 {
            if v & 4 != 0 {
                self.hpic |= 4;
                self.hint = true;
            }
            if v & 2 != 0 {
                self.hpic &= !2;
            }
            return;
        }
        if a == 0xa01a || a == 0xa01c {
            self.hpia = v;
            return;
        }
        if self.peripheral_write(a, v) {
            return;
        }
        if a == 0x1c80 {
            let previous = self.io_value(a as u32);
            self.io_set(a as u32, (previous & 0x60) | (v & 15));
            if v & 10 != 0 {
                self.io_set(a as u32, self.io_value(a as u32) & !0x20);
                self.pll_lock_at = 0;
            } else if previous & 8 != 0 {
                self.pll_lock_at = self.steps + 256;
            }
            return;
        }
        self.io_set(a as u32, v);
    }
    pub fn host_read(&mut self, offset: u32) -> u8 {
        if self.reset_asserted {
            return 0;
        }
        let kind = (offset >> 1) & 3;
        let v = if kind == 0 {
            self.hpic
        } else if kind == 2 {
            self.hpia
        } else {
            self.memory[(self.hpia & 0x7fff) as usize]
        };
        let mut high = offset & 1 == 0;
        if self.hpic & 1 != 0 {
            high = !high;
        }
        let result = (v >> if high { 8 } else { 0 }) as u8;
        if offset & 1 != 0 && kind == 1 {
            self.hpia = self.hpia.wrapping_add(1);
        }
        result
    }
    pub fn host_write(&mut self, offset: u32, value: u8) {
        if self.reset_asserted {
            return;
        }
        let kind = (offset >> 1) & 3;
        let mut high = offset & 1 == 0;
        if self.hpic & 1 != 0 {
            high = !high;
        }
        if kind == 0 {
            if high {
                return;
            }
            let v = (self.hpic & 0xff00) | value as u16;
            if v & 4 != 0 {
                self.hint = false;
                self.hpic &= !4;
                if self.memory[0x100] == 6 {
                    let mut words = [0; 32];
                    words.copy_from_slice(&self.memory[0x100..0x120]);
                    self.host_commands.push(HostCommand {
                        pc: self.host_pc,
                        words,
                    });
                    if self.host_commands.len() > 96 {
                        self.host_commands.remove(0);
                    }
                }
            }
            if v & 2 != 0 && self.hpic & 2 == 0 {
                self.hpic |= 2;
                self.request_interrupt(10);
            }
            self.hpic = (self.hpic & !1) | (value as u16 & 1);
            return;
        }
        self.hpi_latch = (self.hpi_latch & if high { 0xff } else { 0xff00 })
            | ((value as u16) << if high { 8 } else { 0 });
        if offset & 1 == 0 {
            return;
        }
        if kind == 2 {
            self.hpia = self.hpi_latch;
            return;
        }
        self.memory[(self.hpia & 0x7fff) as usize] = self.hpi_latch;
        self.host_words += 1;
        if (self.hpia & 0x7fff) as u32 == self.watch_address {
            self.watched_writes.push(WordWrite {
                address: (self.hpia & 0x7fff) as u32,
                pc: self.host_pc,
                value: self.hpi_latch,
                host: true,
                dma: false,
            });
            if self.watched_writes.len() > 96 {
                self.watched_writes.remove(0);
            }
        }
        if !self.started && self.hpia == 0x60 && self.hpi_latch & 0xff00 != 0 {
            self.pc = ((self.hpi_latch as u32 & 255) << 16) | self.memory[0x61] as u32;
            self.started = true;
        }
        if kind == 1 {
            self.hpia = self.hpia.wrapping_add(1);
        }
    }
    #[cold]
    fn unsupported(&self, why: &str) -> ! {
        std::panic::panic_any(format!("Unsupported {why} at C55x PC {:06x}", self.last_pc));
    }
    fn push(&mut self, v: u16) {
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_sub(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_sub(1) & 0xffff);
        self.write(self.sp, v);
    }
    fn pop(&mut self) -> u16 {
        let v = self.read(self.sp);
        self.sp = (self.sp & 0x7f0000) | (self.sp.wrapping_add(1) & 0xffff);
        self.ssp = (self.ssp & 0x7f0000) | (self.ssp.wrapping_add(1) & 0xffff);
        v
    }
    fn pop_register(&mut self, n: u32) {
        let v = self.pop();
        if n < 4 {
            self.ac[n as usize] = (self.ac[n as usize] & !65535) | v as u64;
        } else {
            self.set_reg(n, v as u64);
        }
    }
    fn signed_reg(&self, n: u32) -> i64 {
        if n < 4 {
            signed40(self.reg(n))
        } else {
            self.reg(n) as i16 as i64
        }
    }
}

struct Decoder {
    p: u32,
    address: u32,
    extension: u32,
    qualifier: u32,
    m: u8,
    mem: bool,
    start: u32,
    compact: bool,
    dual_addressing: bool,
    circular: bool,
    linear: bool,
    encoding: [u8; 4],
}
impl Decoder {
    fn new(
        p: u32,
        compact: bool,
        encoding: [u8; 4],
        dual_addressing: bool,
        circular: bool,
        linear: bool,
    ) -> Self {
        Self {
            p,
            address: 0,
            extension: 0,
            qualifier: 0,
            m: 0,
            mem: false,
            start: p,
            compact,
            dual_addressing,
            circular,
            linear,
            encoding,
        }
    }
    fn byte(&mut self, cpu: &C55) -> u8 {
        let v = if self.compact {
            let offset = self.p - self.start;
            if offset >= 4 {
                cpu.unsupported("compact instruction extension");
            }
            self.encoding[offset as usize]
        } else {
            cpu.program(self.p)
        };
        self.p += 1;
        v
    }
    fn word(&mut self, cpu: &C55) -> u32 {
        let a = self.byte(cpu) as u32;
        (a << 8) | self.byte(cpu) as u32
    }
    fn prepare_mem(&mut self, cpu: &mut C55, sm: u8, units: u32) {
        self.mem = true;
        self.m = sm;
        let m = sm as u32;
        if (self.circular || self.linear)
            && ((m & 31 == 0x11 && (m >> 5 <= 2 || m >> 5 >= 6))
                || (m & 1 != 0 && (m >> 1 & 15 == 6 || m >> 1 & 15 == 7)))
        {
            cpu.unsupported("address qualifier with extended memory address");
        }
        if m & 31 == 0x11 {
            let base = m >> 5;
            if base == 1 {
                self.extension = (self.byte(cpu) as u32) << 16;
                self.extension |= self.word(cpu);
            } else if base <= 2 || base >= 6 {
                self.extension = self.word(cpu);
            }
        } else if m & 1 != 0 && ((m >> 1 & 15) == 6 || (m >> 1 & 15) == 7) {
            self.extension = self.word(cpu);
        }
        let q = if self.compact { 0 } else { cpu.program(self.p) };
        if (0x98..=0x9a).contains(&q) {
            if self.circular || self.linear {
                cpu.unsupported("multiple instruction qualifiers");
            }
            self.qualifier = q as u32;
            self.p += 1;
        }
        if (self.circular || self.linear)
            && self.encoding_opcode(cpu) != 0xd1
            && (m & 1 == 0 || (m & 31 == 0x11 && m >> 5 <= 2))
        {
            cpu.unsupported("address qualifier requires indirect memory addressing");
        }
        if m & 1 == 0 {
            self.address = if self.qualifier == 0x98 {
                m >> 1
            } else if self.qualifier == 0x99 || self.qualifier == 0x9a {
                ((cpu.read(0x2f) as u32 & 0x1ff) << 7) | (m >> 1)
            } else {
                let base = if cpu.st[1] & 0x4000 != 0 {
                    cpu.sp
                } else {
                    cpu.dp
                };
                (base & 0x7f0000) | (base.wrapping_add(m >> 1) & 0xffff)
            };
            return;
        }
        let n = m >> 5;
        let mode = m >> 1 & 15;
        let mut delta = 0i32;
        let mut offset = 0i32;
        if mode == 8 {
            if n == 0 {
                self.address = (cpu.dp & 0x7f0000) | self.extension;
                return;
            }
            if n == 1 {
                self.address = self.extension & 0x7fffff;
                return;
            }
            if n == 2 {
                self.address = self.extension;
                if self.qualifier == 0 {
                    self.qualifier = 0x9b;
                }
                return;
            }
            if n == 4 {
                delta = units as i32;
            } else if n == 5 {
                delta = -(units as i32);
            } else if n >= 6 {
                offset = self.extension as i16 as i32;
            }
            if n == 7 {
                delta = offset;
            }
            self.address = cpu.pointer_address(
                8,
                cpu.pointer_offset(8, cpu.cdp, offset, self.circular, self.linear),
                self.circular,
                self.linear,
            );
            if delta != 0 {
                cpu.cdp = cpu.pointer_offset(8, cpu.cdp, delta, self.circular, self.linear);
            }
            return;
        }
        if cpu.st[2] & 0x8000 != 0 && mode >= 9 && !self.dual_addressing {
            self.address = cpu.pointer_address(
                n,
                cpu.pointer_offset(
                    n,
                    cpu.xar[n as usize],
                    mode as i32 - 8,
                    self.circular,
                    self.linear,
                ),
                self.circular,
                self.linear,
            );
            return;
        }
        if mode >= 14 {
            let pointer = cpu.xar[n as usize];
            let base = if !self.linear && (self.circular || cpu.st[2] & (1 << n) != 0) {
                cpu.memory[(0x32 + n / 2) as usize] as u32
            } else {
                0
            };
            self.address = (pointer & 0x7f0000) | pointer.wrapping_add(base) as u16 as u32;
            cpu.xar[n as usize] = cpu.pointer_bitreverse(pointer, mode == 15);
            return;
        }
        let index = if cpu.st[1] & 0x20 != 0 {
            cpu.xar[0] as u16 as i16 as i32
        } else {
            cpu.t[0] as i16 as i32
        };
        match mode {
            0 => {}
            1 => delta = units as i32,
            2 => delta = -(units as i32),
            3 => delta = index,
            4 => delta = -index,
            5 => offset = index,
            6 => offset = self.extension as i16 as i32,
            7 => {
                offset = self.extension as i16 as i32;
                delta = offset;
            }
            9 => delta = cpu.t[1] as i16 as i32,
            10 => delta = -(cpu.t[1] as i16 as i32),
            11 => offset = cpu.t[1] as i16 as i32,
            12 => {
                offset = units as i32;
                delta = offset;
            }
            13 => {
                offset = -(units as i32);
                delta = offset;
            }
            _ => unreachable!("four-bit addressing mode was handled above"),
        }
        self.address = cpu.pointer_address(
            n,
            cpu.pointer_offset(n, cpu.xar[n as usize], offset, self.circular, self.linear),
            self.circular,
            self.linear,
        );
        if delta != 0 {
            cpu.xar[n as usize] =
                cpu.pointer_offset(n, cpu.xar[n as usize], delta, self.circular, self.linear);
        }
    }
    fn encoding_opcode(&self, cpu: &C55) -> u8 {
        if self.compact {
            self.encoding[0]
        } else {
            cpu.program(self.start)
        }
    }
    fn load(&mut self, cpu: &mut C55) -> u16 {
        if self.qualifier == 0x99 || self.qualifier == 0x9b {
            cpu.read_io(self.address as u16)
        } else {
            cpu.read(self.address)
        }
    }
}
