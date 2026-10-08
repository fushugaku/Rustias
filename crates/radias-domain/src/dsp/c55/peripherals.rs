use super::{C55, DmaState, PendingWrite, WordWrite};

fn width_of(parameters: u16) -> u32 {
    1 << (parameters & 3)
}
fn mcbsp_pair(a: u32, offset: u32) -> bool {
    matches!(a>>1,word if word==0x2800+offset||word==0x2c00+offset||word==0x3000+offset)
}
fn next_address(a: u32, mode: u32, width: u32, element: i16, frame: i16, last: bool) -> u32 {
    if mode == 0 {
        return a;
    }
    if mode == 1 {
        return a.wrapping_add(width) & 0xffffff;
    }
    a.wrapping_add(width)
        .wrapping_sub(1)
        .wrapping_add(if mode == 3 && last {
            frame as i32 as u32
        } else {
            element as i32 as u32
        })
        & 0xffffff
}
impl C55 {
    pub(super) fn dma_status(&mut self, channel: usize, flags: u16) {
        let base = 0xc00 + channel as u32 * 0x20;
        self.io_set(base + 3, self.io_value(base + 3) | flags);
        if flags & self.io_value(base + 2) != 0 {
            self.request_interrupt([18, 9, 20, 21, 14, 15][channel]);
        }
    }
    pub(super) fn emif_clock_divider(&self) -> u32 {
        let v = self.io.get(&0x1c90).copied().unwrap_or(0x8003);
        if v & 0x8000 != 0 {
            (v as u32 & 31) + 1
        } else {
            1
        }
    }
    pub(super) fn emif_read_latency(&self) -> u32 {
        let v = self.io.get(&0x810).copied().unwrap_or(0x5fdf);
        (2 + (v as u32 & 1)) * self.emif_clock_divider()
    }
    pub(super) fn dma_read(&mut self, port: u32, a: u32, width: u32) -> u32 {
        if a & (width - 1) != 0 {
            self.unsupported("unaligned DMA source");
        }
        if port < 3 {
            if (port < 2 && a + width > 0x10000)
                || (port == 2 && (a < 0x10000 || a + width > 0x400000))
            {
                self.unsupported("DMA source outside selected memory port");
            }
            let mut v = 0;
            for i in 0..width {
                v = (v << 8) | self.program(a + i) as u32;
            }
            return v;
        }
        if port != 3 || a + width > 0x20000 {
            self.unsupported("DMA source peripheral port");
        }
        if width == 4 && mcbsp_pair(a, 0) {
            let second = self.read_io(((a >> 1) + 1) as u16);
            return (self.read_io((a >> 1) as u16) as u32) << 16 | second as u32;
        }
        let mut v = self.read_io((a >> 1) as u16) as u32;
        if width == 1 {
            return v >> if a & 1 == 0 { 8 } else { 0 } & 255;
        }
        if width == 4 {
            v = v << 16 | self.read_io(((a >> 1) + 1) as u16) as u32;
        }
        v
    }
    pub(super) fn dma_write(&mut self, port: u32, a: u32, width: u32, v: u32) {
        if a & (width - 1) != 0 {
            self.unsupported("unaligned DMA destination");
        }
        if port < 3 {
            if (port < 2 && a + width > 0x10000)
                || (port == 2 && (a < 0x10000 || a + width > 0x400000))
            {
                self.unsupported("DMA destination outside selected memory port");
            }
            for i in 0..width {
                let word = (a + i) as usize / 2;
                let shift = if (a + i) & 1 == 0 { 8 } else { 0 };
                self.memory[word] = (self.memory[word] & !(255u16 << shift))
                    | (((v >> (8 * (width - 1 - i))) as u16 & 255) << shift);
            }
            let first = a / 2;
            let last = (a + width - 1) / 2;
            if (first..=last).contains(&self.watch_address) {
                self.watched_writes.push(WordWrite {
                    address: self.watch_address,
                    pc: self.pc,
                    value: self.memory[self.watch_address as usize],
                    host: false,
                    dma: true,
                });
                if self.watched_writes.len() > 96 {
                    self.watched_writes.remove(0);
                }
            }
            return;
        }
        if port != 3 || a + width > 0x20000 {
            self.unsupported("DMA destination peripheral port");
        }
        if width == 1 {
            let previous = self.read_io((a >> 1) as u16);
            let shift = if a & 1 == 0 { 8 } else { 0 };
            self.write_io(
                (a >> 1) as u16,
                (previous & !(255u16 << shift)) | ((v as u16 & 255) << shift),
            );
        } else if width == 2 {
            self.write_io((a >> 1) as u16, v as u16);
        } else if mcbsp_pair(a, 2) {
            self.write_io(((a >> 1) + 1) as u16, v as u16);
            self.write_io((a >> 1) as u16, (v >> 16) as u16);
        } else {
            self.write_io((a >> 1) as u16, (v >> 16) as u16);
            self.write_io(((a >> 1) + 1) as u16, v as u16);
        }
    }
    pub(super) fn dma_source_read(&mut self, channel: usize) {
        let base = 0xc00 + channel as u32 * 32;
        let width = width_of(self.dma[channel].config[0]);
        if self.dma[channel].source_frame >= self.dma[channel].config[9] as u32 {
            return;
        }
        let port = self.dma[channel].config[0] as u32 >> 2 & 15;
        if port == 2 && self.dma_clocks < self.dma[channel].source_ready_at {
            return;
        }
        if port == 3
            && self.dma[channel].source_frame + 1 == self.dma[channel].config[9] as u32
            && self.dma[channel].source_element == 0
        {
            self.dma_status(channel, 16);
        }
        let value = self.dma_read(port, self.dma[channel].source, width);
        self.dma[channel].fifo.push_back(value);
        self.io_set(base + 12, (self.dma[channel].source + width - 1) as u16);
        if port == 2 {
            self.dma[channel].source_ready_at = self.dma_clocks + self.emif_clock_divider() as u64;
        }
        self.dma[channel].source_element += 1;
        let last = self.dma[channel].source_element == self.dma[channel].config[8] as u32;
        self.dma[channel].source = next_address(
            self.dma[channel].source,
            self.dma[channel].config[1] as u32 >> 12 & 3,
            width,
            self.dma[channel].config[11] as i16,
            self.dma[channel].config[10] as i16,
            last,
        );
        if last {
            self.dma[channel].source_element = 0;
            self.dma[channel].source_frame += 1;
        }
    }
    pub(super) fn dma_refill(&mut self, channel: usize) {
        if !self.dma[channel].active || (self.dma[channel].config[0] >> 2 & 15) == 3 {
            return;
        }
        let capacity = 32 / width_of(self.dma[channel].config[0]);
        while (self.dma[channel].fifo.len() as u32) < capacity
            && self.dma[channel].source_frame < (self.dma[channel].config[9] as u32)
            && ((self.dma[channel].config[0] >> 2 & 15) != 2
                || self.dma_clocks >= self.dma[channel].source_ready_at)
        {
            self.dma_source_read(channel);
        }
    }
    pub(super) fn dma_start(&mut self, channel: usize) {
        let base = 0xc00 + channel as u32 * 32;
        self.dma[channel] = DmaState::default();
        self.dma_unsynchronized &= !(1 << channel);
        for i in 0..16 {
            self.dma[channel].config[i] = self.io_value(base + i as u32);
        }
        let c = self.dma[channel].config;
        if c[1] & 128 == 0 {
            return;
        }
        if c[0] & 3 == 3 || (c[0] >> 2 & 15) > 3 || (c[0] >> 9 & 15) > 3 || c[8] == 0 || c[9] == 0 {
            self.unsupported("DMA channel configuration");
        }
        self.dma[channel].source = (c[5] as u32) << 16 | c[4] as u32;
        self.dma[channel].target = (c[7] as u32) << 16 | c[6] as u32;
        self.dma[channel].active = true;
        if c[1] & 31 == 0 {
            self.dma_unsynchronized |= 1 << channel;
        }
        if c[0] >> 2 & 15 == 2 {
            self.dma[channel].source_ready_at = self.dma_clocks + self.emif_read_latency() as u64;
        }
        self.io_set(base + 1, self.io_value(base + 1) & !0x800);
        self.dma_refill(channel);
    }
    pub(super) fn dma_transfer(&mut self, channel: usize) {
        if !self.dma[channel].active {
            return;
        }
        let base = 0xc00 + channel as u32 * 32;
        let width = width_of(self.dma[channel].config[0]);
        let port = self.dma[channel].config[0] >> 2 & 15;
        self.dma_refill(channel);
        if self.dma[channel].fifo.is_empty() {
            self.dma_source_read(channel);
        }
        let Some(value) = self.dma[channel].fifo.pop_front() else {
            return;
        };
        let mut flags = if port != 3
            && self.dma[channel].frame + 1 == self.dma[channel].config[9] as u32
            && self.dma[channel].element == 0
        {
            16
        } else {
            0
        };
        self.dma_write(
            self.dma[channel].config[0] as u32 >> 9 & 15,
            self.dma[channel].target,
            width,
            value,
        );
        self.io_set(base + 13, (self.dma[channel].target + width - 1) as u16);
        self.dma_elements += 1;
        self.dma[channel].element += 1;
        let last = self.dma[channel].element == self.dma[channel].config[8] as u32;
        self.dma[channel].target = next_address(
            self.dma[channel].target,
            self.dma[channel].config[1] as u32 >> 14 & 3,
            width,
            self.dma[channel].config[14] as i16,
            self.dma[channel].config[15] as i16,
            last,
        );
        if self.dma[channel].element == (self.dma[channel].config[8] as u32 + 1) / 2 {
            flags |= 4;
        }
        if last {
            self.dma[channel].element = 0;
            self.dma[channel].frame += 1;
            flags |= 8;
        }
        let complete = self.dma[channel].frame == self.dma[channel].config[9] as u32;
        if complete {
            flags |= 32;
        }
        if flags != 0 {
            self.dma_status(channel, flags);
        }
        if complete {
            let control = self.io_value(base + 1);
            self.dma[channel].active = false;
            self.dma_unsynchronized &= !(1 << channel);
            if control & 0x100 != 0 {
                if control & 0xa00 != 0 {
                    self.dma_start(channel);
                } else {
                    self.dma[channel].waiting = true;
                }
            } else {
                self.io_set(base + 1, self.io_value(base + 1) & !128);
            }
        } else {
            self.dma_refill(channel);
        }
    }
    pub(super) fn dma_tick(&mut self) {
        self.dma_clocks += 1;
        if self.dma_unsynchronized == 0 {
            return;
        }
        for channel in 0..6 {
            if self.dma_unsynchronized & (1 << channel) != 0 {
                self.dma_transfer(channel);
            }
        }
    }
    pub fn dma_event(&mut self, event: u32) {
        for channel in 0..6 {
            if !self.dma[channel].active || self.dma[channel].config[1] as u32 & 31 != event {
                continue;
            }
            let base = 0xc00 + channel as u32 * 32;
            self.io_set(base + 3, self.io_value(base + 3) | 64);
            let count = if self.dma[channel].config[1] & 32 != 0 {
                self.dma[channel].config[8] as u32 - self.dma[channel].element
            } else {
                1
            };
            for _ in 0..count {
                if !self.dma[channel].active {
                    break;
                }
                self.dma_transfer(channel);
            }
            self.io_set(base + 3, self.io_value(base + 3) & !64);
        }
    }
    pub(super) fn peripheral_write(&mut self, a: u16, v: u16) -> bool {
        let a = a as u32;
        if (0xc00..0xcc0).contains(&a) {
            let offset = (a - 0xc00) & 31;
            let channel = (a - 0xc00) as usize / 32;
            if offset >= 16 {
                self.unsupported("reserved DMA register");
            }
            if matches!(offset, 3 | 12 | 13) {
                return true;
            }
            let previous = self.io_value(a);
            self.io_set(a, v);
            if offset == 1 {
                if v & 128 == 0 {
                    self.dma[channel] = DmaState::default();
                    self.dma_unsynchronized &= !(1 << channel);
                } else if previous & 128 == 0 || (self.dma[channel].waiting && v & 0x800 != 0) {
                    self.dma_start(channel);
                }
            }
            return true;
        }
        for port in 0..3 {
            let base = 0x2800 + 0x400 * port as u32;
            if a < base || a > base + 0x12 {
                continue;
            }
            if a == base + 2 {
                self.io_set(a, v);
                if self.io_value(base + 5) & 1 == 0 {
                    return true;
                }
                let data = (self.io_value(base + 3) as u32) << 16 | v as u32;
                if self.serial_tx[port].len() == 2 {
                    *self.serial_tx[port].back_mut().unwrap() = data;
                } else {
                    self.serial_tx[port].push_back(data);
                }
                self.serial_tx_repeat[port] = data;
                self.serial_tx_loaded |= 1 << port;
                self.io_set(base + 5, self.io_value(base + 5) & !2);
                if self.serial_tx[port].len() == 1 {
                    self.io_set(base + 5, self.io_value(base + 5) | 4);
                    self.serial_tx_ready(port as u32);
                }
                return true;
            }
            if a == base + 3 {
                self.io_set(a, v);
                let data = (v as u32) << 16 | self.io_value(base + 2) as u32;
                if self.serial_tx[port].len() == 2 {
                    *self.serial_tx[port].back_mut().unwrap() = data;
                }
                if self.serial_tx_loaded & (1 << port) != 0 {
                    self.serial_tx_repeat[port] = data;
                }
                return true;
            }
            if a == base + 4 {
                self.io_set(a, (v & !6) | (self.io_value(a) & 6));
                if v & 1 == 0 {
                    self.io_set(a, self.io_value(a) & !6);
                }
                return true;
            }
            if a == base + 5 {
                let enabled = self.io_value(a) & 1 != 0;
                self.io_set(a, (v & !6) | (self.io_value(a) & 6));
                if v & 1 == 0 {
                    self.serial_tx[port].clear();
                    self.serial_tx_repeat[port] = 0;
                    self.serial_tx_loaded &= !(1 << port);
                    self.serial_tx_events &= !(1 << port);
                    self.io_set(a, self.io_value(a) & !6);
                } else if !enabled {
                    self.serial_tx_ready(port as u32);
                }
                return true;
            }
            self.io_set(a, v);
            return true;
        }
        false
    }
    pub(super) fn peripheral_read(&mut self, a: u16) -> Option<u16> {
        let a = a as u32;
        for port in 0..3 {
            let base = 0x2800 + 0x400 * port;
            if a < base || a > base + 0x12 {
                continue;
            }
            let v = self.io_value(a);
            if a == base {
                let next = self.io_value(base + 4) & !2;
                if self.in_packet {
                    self.pending.push(PendingWrite {
                        address: base + 4,
                        value: next,
                        peripheral: true,
                        read_clear: true,
                    });
                } else {
                    self.io_set(base + 4, next);
                }
            }
            return Some(v);
        }
        None
    }
    pub fn serial_receive(&mut self, port: u32, wire_word: u32) {
        if port >= 3 {
            return;
        }
        let base = 0x2800 + 0x400 * port;
        if self.io_value(base + 4) & 1 == 0 {
            return;
        }
        if self.io_value(base + 7) & 0x8000 != 0 {
            self.unsupported("dual-phase McBSP receive frame");
        }
        let encoded = self.io_value(base + 6) >> 5 & 7;
        if encoded >= 6 {
            self.unsupported("McBSP receive word length");
        }
        let width = [8, 12, 16, 20, 24, 32][encoded as usize];
        let mut value = wire_word;
        if width < 32 {
            value >>= 32 - width;
            let justification = self.io_value(base + 4) >> 13 & 3;
            if justification == 1 {
                value = ((value << (32 - width)) as i32 >> (32 - width)) as u32;
            } else if justification == 2 {
                value <<= 32 - width;
            } else if justification == 3 {
                self.unsupported("McBSP receive justification");
            }
        }
        let ready = self.io_value(base + 4) & 2 != 0;
        self.io_set(base, value as u16);
        self.io_set(base + 1, (value >> 16) as u16);
        self.io_set(base + 4, self.io_value(base + 4) | 2);
        if !ready && self.io_value(base + 4) & 0x30 == 0 {
            self.request_interrupt([5, 6, 12][port as usize]);
        }
        self.dma_event(1 + 4 * port);
    }
    pub(super) fn serial_tx_ready(&mut self, port: u32) {
        let control = 0x2805 + 0x400 * port;
        if self.io_value(control) & 2 != 0 {
            return;
        }
        self.io_set(control, self.io_value(control) | 2);
        self.serial_tx_events |= 1 << port;
        if self.io_value(control) & 0x30 == 0 {
            self.request_interrupt([17, 7, 13][port as usize]);
        }
    }
    pub(super) fn serial_tx_tick(&mut self) {
        let mut pass = 0;
        while self.serial_tx_events != 0 {
            if pass >= 8 {
                self.unsupported("McBSP transmit event feedback without buffering");
            }
            let events = self.serial_tx_events;
            self.serial_tx_events = 0;
            for port in 0..3 {
                if events & (1 << port) != 0 && self.io_value(0x2805 + 0x400 * port) & 1 != 0 {
                    self.dma_event(2 + 4 * port);
                }
            }
            pass += 1;
        }
    }
    pub(super) fn serial_tx_wire(&self, port: u32, data: u32) -> u32 {
        let base = 0x2800 + 0x400 * port;
        if self.io_value(base + 9) & 0x8000 != 0 {
            self.unsupported("dual-phase McBSP transmit frame");
        }
        if self.io_value(base + 9) & 0x18 != 0 {
            self.unsupported("McBSP transmit companding/bit reversal");
        }
        let encoded = self.io_value(base + 8) >> 5 & 7;
        if encoded >= 6 {
            self.unsupported("McBSP transmit word length");
        }
        let width = [8, 12, 16, 20, 24, 32][encoded as usize];
        if width == 32 {
            data
        } else {
            data << (32 - width)
        }
    }
    pub fn serial_transmit(&mut self, port: u32) -> Option<u32> {
        if port >= 3 {
            return None;
        }
        let base = 0x2800 + 0x400 * port;
        if self.io_value(base + 5) & 1 == 0 {
            return None;
        }
        let index = port as usize;
        let data = self.serial_tx[index]
            .front()
            .copied()
            .unwrap_or(self.serial_tx_repeat[index]);
        let wire = self.serial_tx_wire(port, data);
        self.serial_tx[index].pop_front();
        if self.serial_tx[index].is_empty() {
            self.io_set(base + 5, self.io_value(base + 5) & !4);
        } else {
            self.io_set(base + 5, self.io_value(base + 5) | 4);
            self.serial_tx_ready(port);
        }
        Some(wire)
    }
}
