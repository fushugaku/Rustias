//! S29JL032H bottom-boot NOR protocol; deterministic timings match the reference.
use super::CPU_HZ;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Array,
    Autoselect,
    Cfi,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Unlock1,
    Unlock2,
    Program,
    EraseUnlock1,
    EraseUnlock2,
    EraseConfirm,
    BypassExit,
}
pub const SIZE: usize = 0x400000;
pub const MASK: u32 = SIZE as u32 - 1;
pub struct NorFlash {
    pub protected_sectors: [bool; 71],
    pub native_bypass_f0_compatibility: bool,
    pub clock: u64,
    pub revision: u64,
    pub programs: u64,
    pub erases: u64,
    pub ignored_writes: u64,
    modes: [Mode; 4],
    phase: Phase,
    bypass: bool,
    programming: bool,
    erasing: bool,
    selection: bool,
    suspended: bool,
    suspend_pending: bool,
    chip_erase: bool,
    toggle6: bool,
    toggle2: bool,
    program_address: u32,
    program_data: u16,
    erase_banks: u8,
    erase_sectors: [bool; 71],
    program_deadline: u64,
    select_deadline: u64,
    erase_deadline: u64,
    suspend_deadline: u64,
    remaining_erase: u64,
}
impl Default for NorFlash {
    fn default() -> Self {
        Self {
            protected_sectors: [false; 71],
            native_bypass_f0_compatibility: false,
            clock: 0,
            revision: 0,
            programs: 0,
            erases: 0,
            ignored_writes: 0,
            modes: [Mode::Array; 4],
            phase: Phase::Idle,
            bypass: false,
            programming: false,
            erasing: false,
            selection: false,
            suspended: false,
            suspend_pending: false,
            chip_erase: false,
            toggle6: false,
            toggle2: false,
            program_address: 0,
            program_data: 0xffff,
            erase_banks: 0,
            erase_sectors: [false; 71],
            program_deadline: 0,
            select_deadline: 0,
            erase_deadline: 0,
            suspend_deadline: 0,
            remaining_erase: 0,
        }
    }
}
impl NorFlash {
    pub fn selected(a: u32) -> bool {
        a < 0x04000000
    }
    pub fn sector(a: u32) -> usize {
        let a = a & MASK;
        if a < 0x10000 {
            (a / 0x2000) as usize
        } else {
            7 + (a / 0x10000) as usize
        }
    }
    pub fn sector_start(n: usize) -> usize {
        if n < 8 { n * 0x2000 } else { (n - 7) * 0x10000 }
    }
    pub fn sector_size(n: usize) -> usize {
        if n < 8 { 0x2000 } else { 0x10000 }
    }
    pub fn bank(a: u32) -> usize {
        let a = a & MASK;
        if a < 0x80000 {
            0
        } else if a < 0x200000 {
            1
        } else if a < 0x380000 {
            2
        } else {
            3
        }
    }
    pub fn busy(&self) -> bool {
        self.programming || self.erasing
    }
    pub fn bypass_mode(&self) -> bool {
        self.bypass
    }
    pub fn erase_suspended(&self) -> bool {
        self.suspended
    }
    pub fn array_read_mode(&self, a: u32) -> bool {
        !self.busy() && self.modes[Self::bank(a)] == Mode::Array
    }
    pub fn power_cycle(&mut self) {
        self.clock = 0;
        self.phase = Phase::Idle;
        self.modes.fill(Mode::Array);
        self.bypass = false;
        self.programming = false;
        self.erasing = false;
        self.selection = false;
        self.suspended = false;
        self.suspend_pending = false;
        self.chip_erase = false;
        self.erase_sectors.fill(false);
        self.erase_banks = 0;
        self.toggle6 = false;
        self.toggle2 = false;
    }
    pub fn read_word(&mut self, a: u32, cells: &[u8]) -> u16 {
        let a = a & MASK & !1;
        let b = Self::bank(a);
        let s = Self::sector(a);
        if self.programming && Self::bank(self.program_address) == b {
            self.toggle6 = !self.toggle6;
            return (!self.program_data & 0x80) | if self.toggle6 { 0x40 } else { 0 };
        }
        if self.modes[b] == Mode::Autoselect {
            return match a >> 1 & 255 {
                0 => 1,
                1 => 0x227e,
                0xe => 0x220a,
                0xf => 0x2200,
                2 => u16::from(self.protected_sectors[s]),
                _ => 0xffff,
            };
        }
        if self.modes[b] == Mode::Cfi {
            return Self::cfi(a >> 1 & 255);
        }
        if self.erasing && self.erase_banks & (1 << b) != 0 {
            let chosen = self.erase_sectors[s];
            if self.suspended {
                if chosen {
                    self.toggle2 = !self.toggle2;
                    return 0x80 | if self.toggle2 { 4 } else { 0 };
                }
            } else {
                self.toggle6 = !self.toggle6;
                if chosen {
                    self.toggle2 = !self.toggle2;
                }
                return (if self.toggle6 { 0x40 } else { 0 })
                    | (if chosen && self.toggle2 { 4 } else { 0 })
                    | (if self.selection { 0 } else { 8 });
            }
        }
        (cells[a as usize] as u16) << 8 | cells[a as usize + 1] as u16
    }
    pub fn write_word(&mut self, a: u32, data: u16) {
        let a = a & MASK & !1;
        let word = a >> 1;
        let cmd = data as u8;
        let b = Self::bank(a);
        if self.programming {
            self.ignored_writes += 1;
            return;
        }
        if self.erasing && !self.suspended {
            if cmd == 0xb0 && !self.chip_erase && self.erase_banks & (1 << b) != 0 {
                if self.selection {
                    self.selection = false;
                    self.suspended = true;
                    self.remaining_erase = self.erase_duration();
                } else if !self.suspend_pending {
                    self.suspend_pending = true;
                    self.suspend_deadline = self.clock + CPU_HZ * 20 / 1_000_000;
                }
                return;
            }
            if self.selection {
                if cmd == 0x30 {
                    self.add_erase_sector(a);
                    self.select_deadline = self.clock + CPU_HZ * 80 / 1_000_000;
                    return;
                }
                self.cancel_erase();
                self.phase = Phase::Idle;
            } else {
                self.ignored_writes += 1;
                return;
            }
        }
        if self.phase == Phase::Program {
            self.phase = Phase::Idle;
            if self.erasing && self.erase_sectors[Self::sector(a)] {
                self.ignored_writes += 1;
                return;
            }
            self.program_address = a;
            self.program_data = data;
            self.programming = true;
            self.toggle6 = false;
            self.program_deadline = self.clock
                + if self.protected_sectors[Self::sector(a)] {
                    CPU_HZ / 1_000_000
                } else {
                    CPU_HZ * 6 / 1_000_000
                };
            return;
        }
        if self.bypass {
            if self.phase == Phase::BypassExit {
                self.phase = Phase::Idle;
                if cmd == 0 || (cmd == 0xf0 && self.native_bypass_f0_compatibility) {
                    self.bypass = false;
                    self.modes.fill(Mode::Array);
                } else {
                    self.ignored_writes += 1;
                }
            } else if cmd == 0xf0 && self.native_bypass_f0_compatibility {
                self.phase = Phase::Idle;
                self.bypass = false;
                self.modes.fill(Mode::Array);
            } else if cmd == 0xa0 {
                self.phase = Phase::Program;
            } else if cmd == 0x90 {
                self.phase = Phase::BypassExit;
            } else {
                self.ignored_writes += 1;
            }
            return;
        }
        if cmd == 0xf0 {
            self.phase = Phase::Idle;
            self.modes.fill(Mode::Array);
            return;
        }
        if self.erasing
            && cmd == 0x30
            && self.erase_banks & (1 << b) != 0
            && self.phase == Phase::Idle
        {
            self.suspended = false;
            self.erase_deadline = self.clock + self.remaining_erase;
            self.modes.fill(Mode::Array);
            return;
        }
        if word & 0x7ff == 0x55 && cmd == 0x98 {
            self.phase = Phase::Idle;
            self.modes.fill(Mode::Cfi);
            return;
        }
        if self.modes[b] != Mode::Array {
            self.ignored_writes += 1;
            return;
        }
        match self.phase {
            Phase::Idle => {
                if word & 0x7ff == 0x555 && cmd == 0xaa {
                    self.phase = Phase::Unlock1;
                } else {
                    self.ignored_writes += 1;
                }
                return;
            }
            Phase::Unlock1 => {
                if word & 0x7ff == 0x2aa && cmd == 0x55 {
                    self.phase = Phase::Unlock2;
                    return;
                }
            }
            Phase::Unlock2 => {
                if word & 0x7ff == 0x555 {
                    self.phase = Phase::Idle;
                    if cmd == 0xa0 {
                        self.phase = Phase::Program;
                    } else if cmd == 0x80 && !self.erasing {
                        self.phase = Phase::EraseUnlock1;
                    } else if cmd == 0x90 {
                        self.modes[b] = Mode::Autoselect;
                    } else if cmd == 0x20 {
                        self.bypass = true;
                    } else {
                        self.ignored_writes += 1;
                    }
                    return;
                }
            }
            Phase::EraseUnlock1 => {
                if word & 0x7ff == 0x555 && cmd == 0xaa {
                    self.phase = Phase::EraseUnlock2;
                    return;
                }
            }
            Phase::EraseUnlock2 => {
                if word & 0x7ff == 0x2aa && cmd == 0x55 {
                    self.phase = Phase::EraseConfirm;
                    return;
                }
            }
            Phase::EraseConfirm => {
                self.phase = Phase::Idle;
                if cmd == 0x30 {
                    self.erasing = true;
                    self.selection = true;
                    self.chip_erase = false;
                    self.toggle6 = false;
                    self.toggle2 = false;
                    self.erase_sectors.fill(false);
                    self.erase_banks = 0;
                    self.add_erase_sector(a);
                    self.select_deadline = self.clock + CPU_HZ * 80 / 1_000_000;
                    return;
                }
                if word & 0x7ff == 0x555 && cmd == 0x10 {
                    self.erasing = true;
                    self.chip_erase = true;
                    self.selection = false;
                    self.toggle6 = false;
                    self.toggle2 = false;
                    self.erase_sectors.fill(true);
                    self.erase_banks = 15;
                    self.erase_deadline = self.clock + CPU_HZ * 28;
                    return;
                }
            }
            _ => {}
        }
        self.ignored_writes += 1;
        self.phase = if word & 0x7ff == 0x555 && cmd == 0xaa {
            Phase::Unlock1
        } else {
            Phase::Idle
        };
    }
    pub fn advance(&mut self, cycles: u32, cells: &mut [u8]) {
        self.clock += cycles as u64;
        if self.programming && self.clock >= self.program_deadline {
            if !self.protected_sectors[Self::sector(self.program_address)] {
                let a = self.program_address as usize;
                let old = (cells[a] as u16) << 8 | cells[a + 1] as u16;
                let value = old & self.program_data;
                cells[a] = (value >> 8) as u8;
                cells[a + 1] = value as u8;
                if old != value {
                    self.revision += 1;
                }
            }
            self.programming = false;
            self.programs += 1;
        }
        if !self.erasing || self.suspended {
            return;
        }
        if self.selection && self.clock >= self.select_deadline {
            self.selection = false;
            self.erase_deadline = self.select_deadline + self.erase_duration();
        }
        if self.selection {
            return;
        }
        if self.suspend_pending
            && self.suspend_deadline < self.erase_deadline
            && self.clock >= self.suspend_deadline
        {
            self.remaining_erase = self.erase_deadline - self.suspend_deadline;
            self.suspended = true;
            self.suspend_pending = false;
            return;
        }
        if self.clock >= self.erase_deadline {
            for n in 0..71 {
                if self.erase_sectors[n] && !self.protected_sectors[n] {
                    let first = Self::sector_start(n);
                    let last = first + Self::sector_size(n);
                    if cells[first..last].iter().any(|&v| v != 255) {
                        self.revision += 1;
                    }
                    cells[first..last].fill(255);
                    self.erases += 1;
                }
            }
            self.cancel_erase();
            self.modes.fill(Mode::Array);
        }
    }
    fn add_erase_sector(&mut self, a: u32) {
        self.erase_sectors[Self::sector(a)] = true;
        self.erase_banks |= 1 << Self::bank(a);
    }
    fn erase_duration(&self) -> u64 {
        let n = self
            .erase_sectors
            .iter()
            .zip(self.protected_sectors)
            .filter(|(selected, protected)| **selected && !*protected)
            .count() as u64;
        if n != 0 {
            n * CPU_HZ * 400 / 1000
        } else {
            CPU_HZ * 100 / 1_000_000
        }
    }
    fn cancel_erase(&mut self) {
        self.erasing = false;
        self.selection = false;
        self.suspended = false;
        self.suspend_pending = false;
        self.chip_erase = false;
        self.erase_sectors.fill(false);
        self.erase_banks = 0;
    }
    fn cfi(w: u32) -> u16 {
        match w {
            0x10 => 0x51,
            0x11 => 0x52,
            0x12 => 0x59,
            0x13 => 2,
            0x15 => 0x40,
            0x1b => 0x27,
            0x1c => 0x36,
            0x1f => 3,
            0x21 => 9,
            0x23 => 5,
            0x25 => 4,
            0x27 => 0x16,
            0x28 => 2,
            0x2c => 2,
            0x2d => 7,
            0x2f => 0x20,
            0x31 => 0x3e,
            0x34 => 1,
            0x40 => 0x50,
            0x41 => 0x52,
            0x42 => 0x49,
            0x43 => 0x31,
            0x44 => 0x33,
            0x45 => 0xc,
            0x46 => 2,
            0x47 | 0x48 => 1,
            0x49 => 4,
            0x4a => 0x38,
            0x4d => 0x85,
            0x4e => 0x95,
            0x4f => 2,
            0x50 => 1,
            0x57 => 4,
            0x58 => 15,
            0x59 | 0x5a => 24,
            0x5b => 8,
            _ => {
                if (0x10..=0x5b).contains(&w) {
                    0
                } else {
                    0xffff
                }
            }
        }
    }
}
