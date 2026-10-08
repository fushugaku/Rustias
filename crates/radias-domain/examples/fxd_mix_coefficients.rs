//! Execute original SH coefficient routines and observe complete host transfers.
use radias_domain::{
    backup::BackupExtras,
    board::Board,
    controller::{Bus, Sh3},
};
use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

const CALLER: u32 = 0x0c001010;
const STACK: u32 = 0x0c7fff00;
const CONTEXT: u32 = 0x0c700000;

struct Probe {
    board: Board,
    cpu: Sh3,
    queues: [[u32; 5]; 2],
    host_bytes: Arc<Mutex<Vec<[u64; 5]>>>,
}
impl Probe {
    fn new(source: Vec<u8>) -> Self {
        assert_eq!(source.len(), 0xe0000);
        let mut board =
            Board::new(source.clone(), vec![], vec![], BackupExtras::default()).unwrap();
        board.ram[..source.len() - 0x1000].copy_from_slice(&source[0x1000..]);
        let mut queues = [[0; 5]; 2];
        for (q, queue) in queues.iter_mut().enumerate() {
            let descriptor = board.read32(if q == 0 { 0x0c01d730 } else { 0x0c01d72c });
            for (i, value) in queue.iter_mut().enumerate() {
                *value = board.read32(descriptor + i as u32 * 4);
            }
        }
        let host_bytes = Arc::new(Mutex::new(vec![]));
        let observer = host_bytes.clone();
        board.fxd_observer = Some(Box::new(move |clock, pc, address, value, write| {
            observer.lock().unwrap().push([
                clock,
                pc as u64,
                address as u64,
                value as u64,
                u64::from(write),
            ]);
        }));
        Self {
            board,
            cpu: Sh3::default(),
            queues,
            host_bytes,
        }
    }
    fn call(&mut self, entry: u32, r4: u32, r5: u32, r6: u32) -> u32 {
        self.cpu.reset();
        self.cpu.pc = entry;
        self.cpu.pr = CALLER;
        self.cpu.r[15] = STACK;
        self.cpu.r[4] = r4;
        self.cpu.r[5] = r5;
        self.cpu.r[6] = r6;
        for r in 8..15 {
            self.cpu.r[r] = 0xaabbcc00 | r as u32;
        }
        for _ in 0..500000 {
            if self.cpu.pc == CALLER {
                break;
            }
            self.board.current_pc = self.cpu.pc;
            self.cpu.step(&mut self.board);
        }
        assert_eq!(self.cpu.pc, CALLER);
        assert!(!self.cpu.delayed);
        assert_eq!(self.cpu.r[15], STACK);
        for r in 8..15 {
            assert_eq!(self.cpu.r[r], 0xaabbcc00 | r as u32);
        }
        assert!(self.board.fxd_upload.error.is_empty());
        self.cpu.r[0]
    }
    fn run(&mut self, out: &mut impl Write, kind: u32, value: u32, flags: u32, origin: u32) {
        self.host_bytes.lock().unwrap().clear();
        self.board.write8(0x0c147d40, 0);
        self.board.write16(0x0c147cdc, 0);
        for q in self.queues {
            for address in &q[2..] {
                self.board.write16(*address, 0);
            }
        }
        self.board.fxd_upload = Default::default();
        for i in 0..8 {
            self.board.write8(CONTEXT + i, 0);
        }
        if flags == 1 {
            self.board.write8(CONTEXT + 5, 1);
        }
        if flags == 2 {
            self.board.write8(CONTEXT + 6, 1);
        }
        if flags == 3 {
            self.board.write8(CONTEXT + 6, 1);
            self.board.write8(CONTEXT + 1, 1);
        }
        if flags == 4 {
            for i in 0..8 {
                self.board.write8(CONTEXT + i, 0x7f);
            }
        }
        let dry = self.call(0x0c0761be, kind, value, CONTEXT);
        let wet = self.call(0x0c076248, kind, value, CONTEXT);
        self.call(0x0c075140, origin, dry, 0);
        self.call(0x0c075140, origin + 1, wet, 0);
        assert_eq!(self.board.fxd_upload.uploaded32, 0);
        assert_eq!(self.board.fxd_upload.uploaded48, 0);
        let size = self.board.read16(self.queues[0][4]);
        assert_eq!(size, 2);
        assert_eq!(self.board.read16(self.queues[1][4]), 0);
        let mut tags = [0; 2];
        let mut destinations = [0; 2];
        for i in 0..2 {
            tags[i] = self.board.read32(self.queues[0][0] + i as u32 * 4);
            destinations[i] = self.board.read16(self.queues[0][1] + i as u32 * 2);
        }
        for _ in 0..size {
            self.call(0x0c01d7e8, 0, 0, 0);
        }
        assert_eq!(self.board.read16(self.queues[0][4]), 0);
        assert_eq!(self.board.read16(self.queues[1][4]), 0);
        assert_eq!(self.board.fxd_upload.uploaded32, 2);
        assert_eq!(self.board.fxd_upload.uploaded48, 0);
        assert_eq!(self.board.fxd_upload.words32.len(), 2);
        write!(out, "{{\"effect_type\":{kind},\"value\":{value},\"context_profile\":{flags},\"origin\":{origin},\"dry\":{dry},\"wet\":{wet},\"queue\":[[{},{}],[{},{}]],\"host_words\":[", destinations[0], tags[0], destinations[1], tags[1]).unwrap();
        for (i, (index, word)) in self.board.fxd_upload.words32.iter().enumerate() {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "[{index},{word},{}]",
                self.board.fxd_upload.word_controls32[index]
            )
            .unwrap();
        }
        write!(
            out,
            "],\"host_packets\":{},\"host_bus\":[",
            self.board.fxd_upload.packets32
        )
        .unwrap();
        let observed = self.host_bytes.lock().unwrap();
        assert_eq!(observed.len() % 2, 0);
        // The domain's existing observer exposes diagnostic byte lanes.
        // Retain them verbatim, and derive word boundaries only when the
        // actual executing SH opcode and both same-clock lanes prove MOV.W.
        for (i, pair) in observed.chunks_exact(2).enumerate() {
            let (high, low) = (pair[0], pair[1]);
            assert_eq!((high[0], high[1], high[4]), (low[0], low[1], low[4]));
            assert_eq!(high[2] & 1, 0);
            assert_eq!(low[2], high[2] + 1);
            let offset = (high[1] as u32 - 0x0c000000) as usize;
            let opcode = u16::from_be_bytes(self.board.ram[offset..offset + 2].try_into().unwrap());
            assert_eq!(opcode & 0xf, 1);
            assert_eq!(opcode >> 12, if high[4] == 0 { 6 } else { 2 });
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "[{},{},{},16,{}]",
                high[0],
                high[2],
                (high[3] << 8) | low[3],
                high[4]
            )
            .unwrap();
        }
        write!(out, "],\"host_bytes\":[").unwrap();
        for (i, event) in observed.iter().enumerate() {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "[{},{},{},{},{}]",
                event[0], event[1], event[2], event[3], event[4]
            )
            .unwrap();
        }
        writeln!(
            out,
            "],\"fxd_instruction_execution_implemented\":false,\"synthetic_asic_responses\":false}}"
        )
        .unwrap();
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("SYS image path required"));
    let destination = PathBuf::from(args.next().expect("Output JSONL path required"));
    assert!(args.next().is_none());
    let mut probe = Probe::new(fs::read(source).unwrap());
    let mut out = BufWriter::new(fs::File::create(destination).unwrap());
    let mut count = 0;
    for origin in [17, 0xfffe] {
        for kind in 0..31 {
            for flags in 0..5 {
                for value in 0..=100 {
                    probe.run(&mut out, kind, value, flags, origin);
                    count += 1;
                }
            }
        }
    }
    out.flush().unwrap();
    println!("{count} original coefficient pairs and complete transfers, with no ASIC execution");
}
