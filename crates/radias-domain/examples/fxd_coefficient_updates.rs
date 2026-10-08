//! Original SH coefficient-slot allocation, update commands and host transfers.
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
const SLOTS: u32 = 0x0c169f74;
const ORDER: u32 = 0x0c16a004;
struct Probe {
    board: Board,
    cpu: Sh3,
    queues: [[u32; 5]; 2],
    lanes: Arc<Mutex<Vec<[u64; 5]>>>,
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
                *value = board.read32(descriptor + 4 * i as u32);
            }
        }
        let lanes = Arc::new(Mutex::new(vec![]));
        let observer = lanes.clone();
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
            lanes,
        }
    }
    fn call(&mut self, entry: u32, r4: u32, r5: u32, r6: u32, r7: u32) {
        self.cpu.reset();
        self.cpu.pc = entry;
        self.cpu.pr = CALLER;
        self.cpu.r[15] = STACK;
        self.cpu.r[4] = r4;
        self.cpu.r[5] = r5;
        self.cpu.r[6] = r6;
        self.cpu.r[7] = r7;
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
    }
    fn state(&mut self, out: &mut impl Write) {
        write!(out, "{{\"order\":[").unwrap();
        for i in 0..9 {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(out, "{}", self.board.read8(ORDER + i)).unwrap();
        }
        write!(out, "],\"slots\":[").unwrap();
        for i in 0..9 {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            let address = SLOTS + 16 * i;
            write!(out, "[").unwrap();
            for j in 0..4 {
                if j != 0 {
                    write!(out, ",").unwrap();
                }
                write!(out, "{}", self.board.read16(address + 2 * j)).unwrap();
            }
            write!(
                out,
                ",{},{}]",
                self.board.read32(address + 8),
                self.board.read32(address + 12)
            )
            .unwrap();
        }
        write!(out, "]}}").unwrap();
    }
    fn begin(&mut self) {
        self.board.write8(0x0c147d40, 0);
        self.board.write16(0x0c147cdc, 0);
        for q in self.queues {
            for a in &q[2..] {
                self.board.write16(*a, 0);
            }
        }
        for i in 0..9 * 16 {
            self.board.write8(SLOTS + i, 0);
        }
        for i in 0..9 {
            self.board.write8(ORDER + i, 0);
        }
        self.call(0x0c07519a, 0, 0, 0, 0);
    }
    fn action(
        &mut self,
        out: &mut impl Write,
        sequence: u32,
        step: u32,
        direct: u32,
        api: u32,
        enabled: u32,
        mode: u32,
        target: u32,
        value: u32,
    ) {
        self.board.write32(0x0c0cce90, direct);
        self.board.fxd_upload = Default::default();
        self.lanes.lock().unwrap().clear();
        write!(out,"{{\"sequence\":{sequence},\"step\":{step},\"direct_switch\":{direct},\"call_mode\":{api},\"enabled\":{enabled},\"mode\":{mode},\"target\":{target},\"value\":{value},\"before\":").unwrap();
        self.state(out);
        if api != 0 {
            self.call(0x0c0751f8, target, value, mode, 0);
        } else {
            self.call(0x0c0752ee, enabled, target, value, mode);
        }
        assert_eq!(self.board.fxd_upload.uploaded32, 0);
        assert_eq!(self.board.fxd_upload.uploaded48, 0);
        write!(out, ",\"after\":").unwrap();
        self.state(out);
        let count = self.board.read16(self.queues[0][4]);
        assert!(count > 0 && count <= 5);
        assert_eq!(self.board.read16(self.queues[1][4]), 0);
        let read_index = self.board.read16(self.queues[0][3]);
        write!(out, ",\"queue\":[").unwrap();
        for i in 0..count {
            let at = ((read_index as u32 + i as u32) & 2047) as u32;
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "[{},{}]",
                self.board.read16(self.queues[0][1] + 2 * at),
                self.board.read32(self.queues[0][0] + 4 * at)
            )
            .unwrap();
        }
        write!(out, "]").unwrap();
        for _ in 0..16 {
            if self.board.read16(self.queues[0][4]) == 0 {
                break;
            }
            self.call(0x0c01d7e8, 0, 0, 0, 0);
        }
        assert_eq!(self.board.read16(self.queues[0][4]), 0);
        assert_eq!(self.board.read16(self.queues[1][4]), 0);
        assert_eq!(self.board.fxd_upload.uploaded48, 0);
        write!(out, ",\"host_words\":[").unwrap();
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
            "],\"uploaded_words\":{},\"host_packets\":{},\"host_bus\":[",
            self.board.fxd_upload.uploaded32, self.board.fxd_upload.packets32
        )
        .unwrap();
        let lanes = self.lanes.lock().unwrap();
        assert_eq!(lanes.len() % 2, 0);
        for (i, pair) in lanes.chunks_exact(2).enumerate() {
            let (h, l) = (pair[0], pair[1]);
            assert_eq!((h[0], h[1], h[4]), (l[0], l[1], l[4]));
            assert_eq!(h[2] & 1, 0);
            assert_eq!(l[2], h[2] + 1);
            let offset = (h[1] as u32 - 0x0c000000) as usize;
            let opcode = u16::from_be_bytes(self.board.ram[offset..offset + 2].try_into().unwrap());
            assert_eq!(opcode & 15, 1);
            assert_eq!(opcode >> 12, if h[4] == 0 { 6 } else { 2 });
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "[{},{},{},16,{}]",
                h[0],
                h[2],
                (h[3] << 8) | l[3],
                h[4]
            )
            .unwrap();
        }
        write!(out, "],\"host_bytes\":[").unwrap();
        for (i, e) in lanes.iter().enumerate() {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(out, "[{},{},{},{},{}]", e[0], e[1], e[2], e[3], e[4]).unwrap();
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
    let source = PathBuf::from(args.next().expect("SYS required"));
    let output = PathBuf::from(args.next().expect("JSONL required"));
    assert!(args.next().is_none());
    let mut probe = Probe::new(fs::read(source).unwrap());
    let mut out = BufWriter::new(fs::File::create(output).unwrap());
    let (mut sequence, mut cases) = (0, 0);
    let targets = [
        17, 18, 19, 20, 21, 22, 23, 24, 25, 21, 26, 17, 26, 0x2f7, 0xfffe, 0xffff, 18, 19, 20, 21,
        22, 23, 24, 25, 26, 17,
    ];
    let values = [
        0, 0x7fffff, 0xff800000, 0xffa66666, 0x123456, 0xffffffff, 0x12345678,
    ];
    for direct in [0, 1] {
        for api in [0, 1] {
            for enabled in [0, 1, 0x0c700000] {
                for mode in [0, 1, 2, 0xff] {
                    if api != 0 && enabled != 1 {
                        continue;
                    }
                    probe.begin();
                    for (step, target) in targets.iter().enumerate() {
                        probe.action(
                            &mut out,
                            sequence,
                            step as u32,
                            direct,
                            api,
                            enabled,
                            mode,
                            *target,
                            values[step % 7],
                        );
                        cases += 1;
                    }
                    sequence += 1;
                }
            }
        }
    }
    out.flush().unwrap();
    println!(
        "{cases} original update transitions in {sequence} complete sequences; no ASIC execution"
    );
}
