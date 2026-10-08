//! Known host/SRAM transport. FXD03 instruction execution remains unresolved.
use std::collections::BTreeMap;

pub struct Sram {
    pub cells: Vec<u16>,
    pub access_mode: u16,
    pub lane: u16,
    pub address_high: u16,
    pub address_low: u16,
    pub control: u16,
    pub read_latch: u16,
    pub read_latched: bool,
    pub pending: bool,
    pub pending_address: u32,
    pub pending_value: u16,
    pub pending_lane: u16,
    pub pending_write: bool,
    pub host_reads: u64,
    pub host_writes: u64,
    pub aborted: u64,
}
impl Default for Sram {
    fn default() -> Self {
        Self {
            cells: vec![0; 0x40000],
            access_mode: 0,
            lane: 0,
            address_high: 0,
            address_low: 0,
            control: 0,
            read_latch: 0,
            read_latched: false,
            pending: false,
            pending_address: 0,
            pending_value: 0,
            pending_lane: 0,
            pending_write: false,
            host_reads: 0,
            host_writes: 0,
            aborted: 0,
        }
    }
}
impl Sram {
    pub fn read_sample(&self, a: u32) -> u32 {
        (self.cells[(a & 0x3ffff) as usize] as u32) << 8
    }
    pub fn write_sample(&mut self, a: u32, v: u32) {
        self.cells[(a & 0x3ffff) as usize] = (v >> 8) as u16;
    }
    fn selected(&self) -> bool {
        self.access_mode == 4 && self.lane <= 1
    }
    fn address(&self) -> u32 {
        (self.address_high as u32) << 16 | self.address_low as u32
    }
    fn cancel(&mut self) {
        if self.pending {
            self.aborted += 1;
        }
        self.pending = false;
        self.control = 0;
    }
    pub fn controller_reset(&mut self) {
        self.cancel();
        self.read_latched = false;
    }
    fn request(&mut self, write: bool, v: u16) {
        if self.pending {
            self.aborted += 1;
        }
        self.pending = true;
        self.pending_write = write;
        self.pending_address = self.address();
        self.pending_value = v;
        self.pending_lane = self.lane;
    }
    pub fn write_word(&mut self, port: u32, v: u16, clocked: bool) {
        match port {
            0x22 => {
                self.access_mode = v;
                if !self.selected() {
                    self.cancel();
                }
            }
            0x2e => {
                self.lane = v;
                if !self.selected() {
                    self.cancel();
                }
            }
            0x30 => self.address_high = v,
            0x32 => self.address_low = v,
            0x3a => {
                self.cancel();
                if self.selected() && clocked && (v == 2 || v == 6) {
                    self.control = v;
                    if v == 2 {
                        self.request(false, 0);
                    }
                }
            }
            0x36 => {
                if self.selected() && clocked && self.control == 6 {
                    self.request(true, v);
                }
            }
            _ => {}
        }
    }
    pub fn read_word(&self, port: u32, shadow: u16, clocked: bool) -> u16 {
        if !self.selected() || !clocked {
            return shadow;
        }
        if port == 0x3e {
            return (shadow & !4) | if self.pending { 4 } else { 0 };
        }
        if port == 0x36 && self.read_latched {
            return self.read_latch;
        }
        shadow
    }
    pub fn advance(&mut self, clocks: u32, clocked: bool) {
        if clocks == 0 || !self.pending || !clocked {
            return;
        }
        if self.pending_write {
            if self.pending_lane == 0 {
                self.write_sample(self.pending_address, (self.pending_value as u32) << 8);
            }
            self.host_writes += 1;
        } else {
            let v = self.read_sample(self.pending_address);
            self.read_latch = if self.pending_lane != 0 {
                (v & 255) as u16
            } else {
                (v >> 8) as u16
            };
            self.read_latched = true;
            self.host_reads += 1;
        }
        self.pending = false;
    }
}
pub struct HostWindow {
    pub shadow: [u8; 512],
    pub sram: Sram,
    pub control_lines_observed: bool,
    pub reset_high: bool,
    pub pll_reset_high: bool,
    pub reset_assertions: u64,
    pub reset_releases: u64,
    pub pll_reset_assertions: u64,
    pub pll_reset_releases: u64,
    pub word_reads: u64,
    pub word_writes: u64,
    pub byte_reads: u64,
    pub byte_writes: u64,
}
impl Default for HostWindow {
    fn default() -> Self {
        Self {
            shadow: [0; 512],
            sram: Sram::default(),
            control_lines_observed: false,
            reset_high: true,
            pll_reset_high: true,
            reset_assertions: 0,
            reset_releases: 0,
            pll_reset_assertions: 0,
            pll_reset_releases: 0,
            word_reads: 0,
            word_writes: 0,
            byte_reads: 0,
            byte_writes: 0,
        }
    }
}
impl HostWindow {
    pub fn selected(a: u32) -> bool {
        (0x10000000..0x14000000).contains(&a)
    }
    pub fn offset(a: u32) -> u32 {
        a & 511
    }
    pub fn control_lines(&mut self, reset: bool, pll: bool) {
        if self.control_lines_observed {
            if reset != self.reset_high {
                if reset {
                    self.reset_releases += 1;
                } else {
                    self.reset_assertions += 1;
                }
            }
            if pll != self.pll_reset_high {
                if pll {
                    self.pll_reset_releases += 1;
                } else {
                    self.pll_reset_assertions += 1;
                }
            }
        }
        if !reset || !pll {
            self.sram.controller_reset();
        }
        self.control_lines_observed = true;
        self.reset_high = reset;
        self.pll_reset_high = pll;
    }
    pub fn read(&self, a: u32) -> u8 {
        let port = Self::offset(a);
        if self.control_lines_observed && !self.reset_high {
            if port & !1 == 0x16 {
                return if port & 1 != 0 { 0x7f } else { 0 };
            }
            if port & !1 == 0x1e {
                return 0;
            }
        }
        let aligned = port & !1;
        let shadow =
            (self.shadow[aligned as usize] as u16) << 8 | self.shadow[aligned as usize + 1] as u16;
        let word = self
            .sram
            .read_word(aligned, shadow, self.reset_high && self.pll_reset_high);
        if port & 1 != 0 {
            word as u8
        } else {
            (word >> 8) as u8
        }
    }
    pub fn write(&mut self, a: u32, v: u8) {
        self.shadow[Self::offset(a) as usize] = v;
    }
    pub fn read_word(&self, a: u32) -> u16 {
        (self.read(a) as u16) << 8 | self.read(a + 1) as u16
    }
    pub fn write_word(&mut self, a: u32, v: u16) {
        self.write(a, (v >> 8) as u8);
        self.write(a + 1, v as u8);
        self.sram
            .write_word(Self::offset(a), v, self.reset_high && self.pll_reset_high);
    }
    pub fn advance(&mut self, clocks: u32) {
        self.sram
            .advance(clocks, self.reset_high && self.pll_reset_high);
    }
}
#[derive(Default)]
pub struct Packet {
    pub active: bool,
    pub index: u16,
    pub parts: u32,
    pub partial: u64,
    pub words: Vec<u64>,
}
pub struct Upload {
    pub bytes: [u8; 256],
    pub words48: BTreeMap<u16, u64>,
    pub words32: BTreeMap<u16, u32>,
    pub word_controls48: BTreeMap<u16, u16>,
    pub word_controls32: BTreeMap<u16, u16>,
    pub controls48: BTreeMap<u16, u64>,
    pub controls32: BTreeMap<u16, u64>,
    pub packets48: u64,
    pub packets32: u64,
    pub uploaded48: u64,
    pub uploaded32: u64,
    pub p48: Packet,
    pub p32: Packet,
    pub error: String,
}
impl Default for Upload {
    fn default() -> Self {
        Self {
            bytes: [0; 256],
            words48: BTreeMap::new(),
            words32: BTreeMap::new(),
            word_controls48: BTreeMap::new(),
            word_controls32: BTreeMap::new(),
            controls48: BTreeMap::new(),
            controls32: BTreeMap::new(),
            packets48: 0,
            packets32: 0,
            uploaded48: 0,
            uploaded32: 0,
            p48: Packet::default(),
            p32: Packet::default(),
            error: String::new(),
        }
    }
}
impl Upload {
    pub const EXECUTION_IMPLEMENTED: bool = false;
    fn fail(&mut self, s: &str) {
        if self.error.is_empty() {
            self.error = s.into();
        }
    }
    fn index(&mut self, width: u32, v: u16) {
        let p = if width == 48 {
            &mut self.p48
        } else {
            &mut self.p32
        };
        if p.active {
            self.fail("FXD upload index changed before commit");
            return;
        }
        *p = Packet {
            active: true,
            index: v,
            ..Default::default()
        };
    }
    fn part(&mut self, width: u32, ordinal: u32, count: u32, v: u16) {
        let p = if width == 48 {
            &mut self.p48
        } else {
            &mut self.p32
        };
        if !p.active || p.parts != ordinal {
            self.fail("FXD upload data without index or out of order");
            return;
        }
        p.partial = (p.partial << 16) | v as u64;
        p.parts += 1;
        if p.parts == count {
            p.words.push(p.partial);
            p.parts = 0;
            p.partial = 0;
        }
    }
    fn commit(&mut self, width: u32, control: u16) {
        let p = if width == 48 { &self.p48 } else { &self.p32 };
        if !p.active || p.parts != 0 || p.words.is_empty() {
            self.fail("FXD incomplete upload at commit");
            return;
        }
        if control & 1 == 0 {
            self.fail("FXD upload command without observed commit bit");
            return;
        }
        if p.words.len() > if width == 48 { 16 } else { 32 } {
            self.fail("FXD upload exceeds original driver chunk limit");
            return;
        }
        let p = if width == 48 {
            std::mem::take(&mut self.p48)
        } else {
            std::mem::take(&mut self.p32)
        };
        let mut a = p.index;
        if width == 48 {
            self.uploaded48 += p.words.len() as u64;
            for v in p.words {
                self.words48.insert(a, v);
                self.word_controls48.insert(a, control);
                a = a.wrapping_add(1);
            }
            *self.controls48.entry(control).or_default() += 1;
            self.packets48 += 1;
        } else {
            self.uploaded32 += p.words.len() as u64;
            for v in p.words {
                self.words32.insert(a, v as u32);
                self.word_controls32.insert(a, control);
                a = a.wrapping_add(1);
            }
            *self.controls32.entry(control).or_default() += 1;
            self.packets32 += 1;
        }
    }
    pub fn write_word(&mut self, offset: u32, v: u16) {
        if offset & 1 != 0 || offset + 1 >= 256 || !self.error.is_empty() {
            return;
        }
        self.bytes[offset as usize] = (v >> 8) as u8;
        self.bytes[offset as usize + 1] = v as u8;
        match offset {
            0x82 => self.index(48, v),
            0x8a => self.part(48, 0, 3, v),
            0x8c => self.part(48, 1, 3, v),
            0x8e => self.part(48, 2, 3, v),
            0x86 => self.commit(48, v),
            0x92 => self.index(32, v),
            0x9c => self.part(32, 0, 2, v),
            0x9e => self.part(32, 1, 2, v),
            0x96 => self.commit(32, v),
            _ => {}
        }
    }
    pub fn write(&mut self, offset: u32, v: u8) {
        if offset >= 256 || !self.error.is_empty() {
            return;
        }
        self.bytes[offset as usize] = v;
        if offset & 1 != 0 {
            self.write_word(
                offset - 1,
                (self.bytes[offset as usize - 1] as u16) << 8 | v as u16,
            );
        }
    }
}
