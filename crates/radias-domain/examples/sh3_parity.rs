//! Replay the independent C++ opcode matrix and exact bus transcripts.
use radias_domain::controller::{Bus, Interrupt, Sh3};
use std::{
    fs::File,
    io::{BufReader, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};

fn bytes<const N: usize>(r: &mut impl Read) -> [u8; N] {
    let mut b = [0; N];
    r.read_exact(&mut b).expect("Truncated SH3 oracle");
    b
}
fn u32le(r: &mut impl Read) -> u32 {
    u32::from_le_bytes(bytes(r))
}
fn read_cpu(r: &mut impl Read) -> Sh3 {
    let mut s = Sh3::default();
    for v in &mut s.r {
        *v = u32le(r);
    }
    for v in &mut s.bank {
        *v = u32le(r);
    }
    for v in [
        &mut s.pc,
        &mut s.sr,
        &mut s.gbr,
        &mut s.vbr,
        &mut s.pr,
        &mut s.mach,
        &mut s.macl,
        &mut s.ssr,
        &mut s.spc,
    ] {
        *v = u32le(r);
    }
    for v in [&mut s.steps, &mut s.cycles, &mut s.interrupts] {
        *v = u64::from_le_bytes(bytes(r));
    }
    s.sleeping = bytes::<1>(r)[0] != 0;
    s.delayed = bytes::<1>(r)[0] != 0;
    s.restore_sr = bytes::<1>(r)[0] != 0;
    s.delayed_pc = u32le(r);
    s.delayed_sr = u32le(r);
    s.last_pc = u32le(r);
    s.last_op = u16::from_le_bytes(bytes(r));
    s
}
struct ReplayBus {
    events: Vec<[u32; 4]>,
    at: usize,
}
impl ReplayBus {
    fn take(&mut self, kind: u32, a: Option<u32>, b: Option<u32>) -> [u32; 4] {
        let value = *self
            .events
            .get(self.at)
            .expect("Unexpected Rust bus access");
        self.at += 1;
        assert_eq!(value[0], kind, "Bus access kind differs");
        if let Some(a) = a {
            assert_eq!(value[1], a, "Bus address/tick/event differs");
        }
        if let Some(b) = b {
            assert_eq!(value[2], b, "Bus write/event payload differs");
        }
        value
    }
}
impl Bus for ReplayBus {
    fn read8(&mut self, a: u32) -> u8 {
        self.take(0, Some(a), None)[2] as u8
    }
    fn write8(&mut self, a: u32, v: u8) {
        self.take(1, Some(a), Some(v as u32));
    }
    fn tick(&mut self, n: u32) {
        self.take(2, Some(n), Some(0));
    }
    fn interrupt(&self) -> Interrupt {
        let v = *self
            .events
            .get(self.at)
            .expect("Missing interrupt observation");
        assert_eq!(v[0], 3);
        Interrupt {
            level: v[1],
            event: v[2],
            event2: v[3],
        }
    }
    fn event(&mut self, e: u32, irq: bool) {
        self.take(4, Some(e), Some(u32::from(irq)));
    }
    fn event2(&mut self, e: u32) {
        self.take(5, Some(e), Some(0));
    }
}
// Bus::interrupt is &self, so the cursor must advance through interior state.
// Keep the instruction fixture separate from the engine's Board implementation.
struct CheckedBus {
    inner: std::cell::RefCell<ReplayBus>,
}
impl Bus for CheckedBus {
    fn read8(&mut self, a: u32) -> u8 {
        self.inner.get_mut().read8(a)
    }
    fn write8(&mut self, a: u32, v: u8) {
        self.inner.get_mut().write8(a, v)
    }
    fn tick(&mut self, n: u32) {
        self.inner.get_mut().tick(n)
    }
    fn interrupt(&self) -> Interrupt {
        let v = self.inner.borrow_mut().take(3, None, None);
        Interrupt {
            level: v[1],
            event: v[2],
            event2: v[3],
        }
    }
    fn event(&mut self, e: u32, irq: bool) {
        self.inner.get_mut().event(e, irq)
    }
    fn event2(&mut self, e: u32) {
        self.inner.get_mut().event2(e)
    }
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("sh3_parity ORACLE REPORT"));
    let report = PathBuf::from(args.next().expect("Missing report path"));
    let mut input = BufReader::new(File::open(source).unwrap());
    assert_eq!(&bytes::<8>(&mut input), b"RSHO0001");
    let count = u32le(&mut input);
    let mut failures = Vec::new();
    let start = std::time::Instant::now();
    for index in 0..count {
        let mut cpu = read_cpu(&mut input);
        let expected = read_cpu(&mut input);
        let length = u32le(&mut input) as usize;
        assert!(length < 65536);
        let mut error = vec![0; length];
        input.read_exact(&mut error).unwrap();
        let expected_error = String::from_utf8(error).unwrap();
        let n = u32le(&mut input) as usize;
        assert!(n < 1024);
        let events = (0..n)
            .map(|_| {
                [
                    u32le(&mut input),
                    u32le(&mut input),
                    u32le(&mut input),
                    u32le(&mut input),
                ]
            })
            .collect();
        let mut bus = CheckedBus {
            inner: std::cell::RefCell::new(ReplayBus { events, at: 0 }),
        };
        let actual_error = match catch_unwind(AssertUnwindSafe(|| cpu.step(&mut bus))) {
            Ok(()) => String::new(),
            Err(error) => error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Non-string panic".into()),
        };
        let consumed = bus.inner.borrow().at == n;
        if cpu != expected || actual_error != expected_error || !consumed {
            if failures.len() < 12 {
                eprintln!(
                    "case {index}: state={} error={} bus={} cpp_error={expected_error:?} rust_error={actual_error:?}",
                    cpu == expected,
                    actual_error == expected_error,
                    consumed
                );
                if cpu != expected {
                    eprintln!("CPP {expected:?}\nRust {cpu:?}");
                }
            }
            failures.push(index);
        }
    }
    let mut tail = [0];
    assert_eq!(input.read(&mut tail).unwrap(), 0, "Unconsumed oracle tail");
    if let Some(parent) = report.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let result = format!(
        "{{\"scope\":\"Original C++ SH3 opcode matrix and IRQ fixtures; complete CPU state and bus transcript\",\"cases\":{count},\"passed\":{},\"failed\":{},\"elapsed_seconds\":{},\"first_failed_cases\":{:?}}}\n",
        count - failures.len() as u32,
        failures.len(),
        start.elapsed().as_secs_f64(),
        &failures[..failures.len().min(12)]
    );
    std::fs::write(report, result.as_bytes()).unwrap();
    print!("{result}");
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
