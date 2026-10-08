use std::collections::VecDeque;
pub struct Scif {
    pub mode: u8,
    pub baud: u8,
    pub control: u8,
    pub fifo_control: u8,
    pub status: u16,
    pub seen: u16,
    pub rx: VecDeque<u8>,
    pub tx: VecDeque<u8>,
    pub transmit_shift: Option<u8>,
    pub transmit_ignored: u64,
    pub receive_phase: u64,
    pub transmit_phase: u64,
    pub receive_age: u64,
}
impl Default for Scif {
    fn default() -> Self {
        Self {
            mode: 0,
            baud: 255,
            control: 0,
            fifo_control: 0,
            status: 0x60,
            seen: 0,
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            transmit_shift: None,
            transmit_ignored: 0,
            receive_phase: 0,
            transmit_phase: 0,
            receive_age: 0,
        }
    }
}
impl Scif {
    fn receive_trigger(&self) -> usize {
        [1, 4, 8, 14][(self.fifo_control >> 6) as usize]
    }
    fn transmit_trigger(&self) -> usize {
        [8, 4, 2, 1][(self.fifo_control >> 4 & 3) as usize]
    }
    fn bit_units(&self) -> u64 {
        144 * 32 * (self.baud as u64 + 1) * (1 << (2 * (self.mode & 3)))
    }
    fn frame_bits(&self) -> u64 {
        1 + if self.mode & 0x40 != 0 { 7 } else { 8 }
            + if self.mode & 0x20 != 0 { 1 } else { 0 }
            + if self.mode & 8 != 0 { 2 } else { 1 }
    }
    fn start_transmit(&mut self) {
        if self.control & 0x20 == 0 || self.transmit_shift.is_some() || self.tx.is_empty() {
            return;
        }
        let v = self.tx.pop_front().unwrap();
        self.transmit_shift = Some(if self.mode & 0x40 != 0 { v & 127 } else { v });
        self.status &= !0x40;
        if self.tx.len() <= self.transmit_trigger() {
            self.status |= 0x20;
        }
    }
    pub fn read(&mut self, offset: u32) -> u8 {
        match offset {
            0 => self.mode,
            2 => self.baud,
            4 => self.control,
            8 => {
                self.seen |= self.status & 0xff00;
                (self.status >> 8) as u8
            }
            9 => {
                self.seen |= self.status & 255;
                self.status as u8
            }
            10 => self.rx.pop_front().unwrap_or(0),
            12 => self.fifo_control,
            14 => self.tx.len() as u8,
            15 => self.rx.len() as u8,
            _ => 0,
        }
    }
    pub fn write(&mut self, offset: u32, v: u8) {
        match offset {
            0 => self.mode = v & 0x7b,
            2 => self.baud = v,
            4 => {
                self.control = v & 0xf3;
                if self.control & 0x20 == 0 {
                    self.transmit_shift = None;
                    self.transmit_phase = 0;
                    self.status |= 0x40;
                } else {
                    self.start_transmit();
                }
            }
            6 => {
                if self.fifo_control & 4 != 0 {
                    return;
                }
                if self.tx.len() == 16 {
                    self.transmit_ignored += 1;
                    return;
                }
                self.tx.push_back(v);
                self.status &= !0x40;
                if self.tx.len() > self.transmit_trigger() {
                    self.status &= !0x20;
                }
                self.start_transmit();
            }
            9 => {
                self.status &= !(self.seen & !(v as u16) & 0xf3);
                self.seen &= !0xff;
            }
            12 => {
                self.fifo_control = v;
                if v & 2 != 0 {
                    self.rx.clear();
                    self.receive_age = 0;
                    self.receive_phase = 0;
                    self.status &= !3;
                    self.seen &= !3;
                }
                if v & 4 != 0 {
                    self.tx.clear();
                    self.status |= 0x20;
                }
            }
            _ => {}
        }
    }
    fn receive(&mut self, v: u8) {
        if self.control & 0x10 == 0 || self.fifo_control & 2 != 0 {
            return;
        }
        if self.rx.len() == 16 {
            std::panic::panic_any("SCIF2 receive FIFO overrun".to_string());
        }
        self.rx
            .push_back(if self.mode & 0x40 != 0 { v & 127 } else { v });
        self.receive_age = 0;
        if self.rx.len() >= self.receive_trigger() {
            self.status |= 2;
        }
    }
    pub fn tick(&mut self, cycles: u32, incoming: &mut VecDeque<u8>, outgoing: &mut VecDeque<u8>) {
        let bit = self.bit_units();
        let frame = bit * self.frame_bits();
        self.receive_age += cycles as u64 * 24;
        if incoming.is_empty() {
            self.receive_phase = 0;
        } else {
            self.receive_phase += cycles as u64 * 24;
            while self.receive_phase >= frame && !incoming.is_empty() {
                self.receive_phase -= frame;
                let v = incoming.pop_front().unwrap();
                if self.fifo_control & 1 == 0 {
                    self.receive(v);
                }
            }
        }
        if !self.rx.is_empty()
            && self.rx.len() < self.receive_trigger()
            && self.receive_age >= bit * 15
        {
            self.status |= 1;
        }
        self.start_transmit();
        if self.transmit_shift.is_some() {
            self.transmit_phase += cycles as u64 * 24;
            while self.transmit_phase >= frame && self.transmit_shift.is_some() {
                self.transmit_phase -= frame;
                let v = self.transmit_shift.take().unwrap();
                if self.fifo_control & 1 != 0 {
                    self.receive(v);
                } else {
                    outgoing.push_back(v);
                }
                self.start_transmit();
            }
            if self.transmit_shift.is_none() {
                self.transmit_phase = 0;
            }
        } else {
            self.transmit_phase = 0;
        }
        if self.transmit_shift.is_none() && self.tx.is_empty() {
            self.status |= 0x40;
        }
        if self.tx.len() <= self.transmit_trigger() {
            self.status |= 0x20;
        }
        if self.rx.len() >= self.receive_trigger() {
            self.status |= 2;
        }
    }
    pub fn interrupt_event(&self) -> u32 {
        if self.control & 0x40 != 0 {
            if self.status & 0x80 != 0 {
                return 0x900;
            }
            if self.status & 3 != 0 {
                return 0x920;
            }
            if self.status & 0x10 != 0 {
                return 0x940;
            }
        }
        if self.control & 0x80 != 0 && self.status & 0x20 != 0 {
            0x960
        } else {
            0
        }
    }
}
