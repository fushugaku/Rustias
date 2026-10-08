//! Versioned test/state boundary; file transport stays outside the domain.
use super::{C55, DmaState, HostCommand, PendingWrite, WordWrite};
struct Writer(Vec<u8>);
impl Writer {
    fn u(&mut self, v: u64, n: usize) {
        self.0.extend_from_slice(&v.to_le_bytes()[..n]);
    }
    fn string(&mut self, v: &str) {
        self.u(v.len() as u64, 4);
        self.0.extend_from_slice(v.as_bytes());
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn u(&mut self, n: usize) -> Result<u64, String> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or("checkpoint offset overflow")?;
        let source = self
            .bytes
            .get(self.pos..end)
            .ok_or("truncated C55 checkpoint")?;
        let mut value = [0; 8];
        value[..n].copy_from_slice(source);
        self.pos = end;
        Ok(u64::from_le_bytes(value))
    }
    fn string(&mut self) -> Result<String, String> {
        let size = self.u(4)? as usize;
        let end = self
            .pos
            .checked_add(size)
            .ok_or("checkpoint string overflow")?;
        let source = self
            .bytes
            .get(self.pos..end)
            .ok_or("truncated checkpoint string")?;
        self.pos = end;
        String::from_utf8(source.to_vec()).map_err(|_| "invalid checkpoint string".into())
    }
    fn count(&mut self, limit: usize) -> Result<usize, String> {
        let n = self.u(4)? as usize;
        if n > limit {
            return Err("checkpoint collection exceeds supported limit".into());
        }
        Ok(n)
    }
}
impl C55 {
    pub fn checkpoint_bytes(&self) -> Vec<u8> {
        let mut b = Writer(Vec::new());
        b.u(self.memory.len() as u64, 4);
        b.u(self.memory.iter().filter(|&&v| v != 0).count() as u64, 4);
        for (a, &v) in self.memory.iter().enumerate() {
            if v != 0 {
                b.u(a as u64, 4);
                b.u(v as u64, 2);
            }
        }
        b.u(self.io.len() as u64, 4);
        for (&a, &v) in &self.io {
            b.u(a as u64, 2);
            b.u(v as u64, 2);
        }
        for &v in &self.ac {
            b.u(v, 8);
        }
        for &v in &self.t {
            b.u(v as u64, 8);
        }
        for &v in &self.st {
            b.u(v as u64, 8);
        }
        for &v in &self.xar {
            b.u(v as u64, 8);
        }
        macro_rules! u {($($field:expr),*)=>{$(b.u($field as u64,8);)*};}
        u!(
            self.pc,
            self.sp,
            self.ssp,
            self.dp,
            self.cdp,
            self.hpic,
            self.hpia,
            self.hpi_latch,
            self.steps,
            self.host_words,
            self.interrupts,
            self.dma_elements,
            self.dbstat,
            self.pll_lock_at,
            self.last_pc,
            self.repeat_pc,
            self.block_start,
            self.block_end,
            self.repeat_left,
            self.started,
            self.hint,
            self.sleeping,
            self.reset_asserted,
            self.block_active,
            self.loop_type[0],
            self.loop_type[1],
            self.repeat_active,
            self.simple_packets_enabled,
            self.watch_address,
            self.host_pc,
            self.dma_clocks,
            self.dma_unsynchronized,
            self.serial_tx_repeat[0],
            self.serial_tx_repeat[1],
            self.serial_tx_repeat[2],
            self.serial_tx_loaded,
            self.serial_tx_events,
            self.in_packet,
            self.gpio_pins
        );
        for d in &self.dma {
            for &v in &d.config {
                b.u(v as u64, 8);
            }
            u!(
                d.source,
                d.target,
                d.source_ready_at,
                d.source_element,
                d.source_frame,
                d.element,
                d.frame,
                d.active,
                d.waiting
            );
            b.u(d.fifo.len() as u64, 4);
            for &v in &d.fifo {
                b.u(v as u64, 4);
            }
        }
        for q in &self.serial_tx {
            b.u(q.len() as u64, 4);
            for &v in q {
                b.u(v as u64, 4);
            }
        }
        b.u(self.pending.len() as u64, 4);
        for v in &self.pending {
            b.u(v.address as u64, 4);
            b.u(v.value as u64, 2);
            b.u(v.peripheral as u64, 1);
            b.u(v.read_clear as u64, 1);
        }
        b.string(&self.fault);
        b.u(self.watched_writes.len() as u64, 4);
        for v in &self.watched_writes {
            b.u(v.address as u64, 4);
            b.u(v.pc as u64, 4);
            b.u(v.value as u64, 2);
            b.u(v.host as u64, 1);
            b.u(v.dma as u64, 1);
        }
        b.u(self.host_commands.len() as u64, 4);
        for v in &self.host_commands {
            b.u(v.pc as u64, 4);
            for &w in &v.words {
                b.u(w as u64, 2);
            }
        }
        b.0
    }
    pub fn from_checkpoint_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader { bytes, pos: 0 };
        let words = r.count(0x200000)?;
        let mut d = C55::new();
        d.memory = vec![0; words];
        let mut previous = None;
        for _ in 0..r.count(words)? {
            let a = r.u(4)? as usize;
            if a >= words || previous.is_some_and(|p| p >= a) {
                return Err("invalid checkpoint memory ordering".into());
            }
            d.memory[a] = r.u(2)? as u16;
            previous = Some(a);
        }
        d.io.clear();
        for _ in 0..r.count(65536)? {
            let a = r.u(2)? as u16;
            let v = r.u(2)? as u16;
            if d.io.insert(a, v).is_some() {
                return Err("duplicate checkpoint I/O address".into());
            }
        }
        for v in &mut d.ac {
            *v = r.u(8)?;
        }
        for v in &mut d.t {
            *v = r.u(8)? as u16;
        }
        for v in &mut d.st {
            *v = r.u(8)? as u16;
        }
        for v in &mut d.xar {
            *v = r.u(8)? as u32;
        }
        macro_rules! u {($($field:expr),*)=>{$($field=r.u(8)? as _;)*};}
        macro_rules! flag {($($field:expr),*)=>{$({let v=r.u(8)?;if v>1{return Err("invalid checkpoint Boolean".into());}$field=v!=0;})*};}
        u!(
            d.pc,
            d.sp,
            d.ssp,
            d.dp,
            d.cdp,
            d.hpic,
            d.hpia,
            d.hpi_latch,
            d.steps,
            d.host_words,
            d.interrupts,
            d.dma_elements,
            d.dbstat,
            d.pll_lock_at,
            d.last_pc,
            d.repeat_pc,
            d.block_start,
            d.block_end,
            d.repeat_left
        );
        flag!(
            d.started,
            d.hint,
            d.sleeping,
            d.reset_asserted,
            d.block_active
        );
        u!(d.loop_type[0], d.loop_type[1]);
        flag!(d.repeat_active, d.simple_packets_enabled);
        u!(
            d.watch_address,
            d.host_pc,
            d.dma_clocks,
            d.dma_unsynchronized,
            d.serial_tx_repeat[0],
            d.serial_tx_repeat[1],
            d.serial_tx_repeat[2],
            d.serial_tx_loaded,
            d.serial_tx_events
        );
        flag!(d.in_packet);
        u!(d.gpio_pins);
        for state in &mut d.dma {
            *state = DmaState::default();
            for v in &mut state.config {
                *v = r.u(8)? as u16;
            }
            u!(
                state.source,
                state.target,
                state.source_ready_at,
                state.source_element,
                state.source_frame,
                state.element,
                state.frame
            );
            flag!(state.active, state.waiting);
            for _ in 0..r.count(32)? {
                state.fifo.push_back(r.u(4)? as u32);
            }
        }
        for q in &mut d.serial_tx {
            q.clear();
            for _ in 0..r.count(2)? {
                q.push_back(r.u(4)? as u32);
            }
        }
        for _ in 0..r.count(128)? {
            d.pending.push(PendingWrite {
                address: r.u(4)? as u32,
                value: r.u(2)? as u16,
                peripheral: r.u(1)? != 0,
                read_clear: r.u(1)? != 0,
            });
        }
        d.fault = r.string()?;
        for _ in 0..r.count(96)? {
            d.watched_writes.push(WordWrite {
                address: r.u(4)? as u32,
                pc: r.u(4)? as u32,
                value: r.u(2)? as u16,
                host: r.u(1)? != 0,
                dma: r.u(1)? != 0,
            });
        }
        for _ in 0..r.count(96)? {
            let pc = r.u(4)? as u32;
            let mut words = [0; 32];
            for v in &mut words {
                *v = r.u(2)? as u16;
            }
            d.host_commands.push(HostCommand { pc, words });
        }
        if r.pos != bytes.len() {
            return Err("trailing checkpoint bytes".into());
        }
        Ok(d)
    }
}
