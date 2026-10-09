//! Original outputs and store clocks are assertions; buffers, bank selector,
//! ring pointers, and external input changes are the only native inputs.
use radias_synth_application::dsp_buffers::DspBufferExecution;
use radias_synth_domain::dsp_buffers::{DspBufferBus, DspBufferTask, DspBufferWork, DspRole};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
const BUFFER_BASE: usize = 0x462;
const BUFFER_WORDS: usize = 0x2a0;
const SNAPSHOT_WORDS: usize = BUFFER_WORDS + 6;
struct Reader {
    bytes: Vec<u8>,
    offset: usize,
}
impl Reader {
    fn word(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.offset..self.offset + 4].try_into().unwrap());
        self.offset += 4;
        v
    }
    fn snapshot(&mut self) -> [u16; SNAPSHOT_WORDS] {
        core::array::from_fn(|_| self.word() as u16)
    }
    fn rows<const N: usize>(&mut self) -> Vec<[u32; N]> {
        let count = self.word();
        (0..count)
            .map(|_| core::array::from_fn(|_| self.word()))
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Event {
    clock: u32,
    address: u16,
    value: u16,
    space: u8,
}
struct Bus {
    ram: Vec<u16>,
    host: u16,
    events: Vec<Event>,
}
impl Bus {
    fn new(snapshot: &[u16; SNAPSHOT_WORDS], bank: u16) -> Self {
        let mut ram = vec![0; 0x1600];
        ram[BUFFER_BASE..BUFFER_BASE + BUFFER_WORDS].copy_from_slice(&snapshot[..BUFFER_WORDS]);
        ram[0x1500..0x1503].copy_from_slice(&snapshot[BUFFER_WORDS..BUFFER_WORDS + 3]);
        ram[0x100..0x102].copy_from_slice(&snapshot[BUFFER_WORDS + 3..BUFFER_WORDS + 5]);
        ram[0x442] = bank;
        Self {
            ram,
            host: snapshot[SNAPSHOT_WORDS - 1],
            events: Vec::new(),
        }
    }
    fn snapshot(&self) -> [u16; SNAPSHOT_WORDS] {
        let mut result = [0; SNAPSHOT_WORDS];
        result[..BUFFER_WORDS].copy_from_slice(&self.ram[BUFFER_BASE..BUFFER_BASE + BUFFER_WORDS]);
        result[BUFFER_WORDS..BUFFER_WORDS + 3].copy_from_slice(&self.ram[0x1500..0x1503]);
        result[BUFFER_WORDS + 3..BUFFER_WORDS + 5].copy_from_slice(&self.ram[0x100..0x102]);
        result[SNAPSHOT_WORDS - 1] = self.host;
        result
    }
}
impl DspBufferBus for Bus {
    fn read_data(&self, address: u16) -> u16 {
        self.ram[address as usize]
    }
    fn write_data(&mut self, clock: u32, address: u16, value: u16) {
        self.ram[address as usize] = value;
        self.events.push(Event {
            clock,
            address,
            value,
            space: 0,
        });
    }
    fn write_io(&mut self, clock: u32, address: u16, value: u16) {
        self.events.push(Event {
            clock,
            address,
            value,
            space: 1,
        });
    }
    fn host_control(&self) -> u16 {
        self.host
    }
    fn set_host_control(&mut self, clock: u32, value: u16) {
        self.host = value;
        self.events.push(Event {
            clock,
            address: 0,
            value,
            space: 2,
        });
    }
}
struct Case {
    chip: u32,
    kind: u32,
    bank: u16,
    before: [u16; SNAPSHOT_WORDS],
    inputs: Vec<[u32; 3]>,
    duration: u32,
    events: Vec<Event>,
    after: [u16; SNAPSHOT_WORDS],
}
fn run(case: &Case, partition: u32, wrong_bank: bool) -> (u32, Vec<Event>, [u16; SNAPSHOT_WORDS]) {
    let role = if case.chip == 0 {
        DspRole::Master
    } else {
        DspRole::Slave
    };
    let task = match case.kind {
        0 => DspBufferTask::MailboxNoop,
        1 => DspBufferTask::PublishOutput,
        2 => DspBufferTask::LoadInput,
        3 => DspBufferTask::AdvanceOutputRing,
        4 => DspBufferTask::BeginSynthesisBatch,
        _ => panic!("Unsupported buffer job"),
    };
    let mut execution = DspBufferExecution {
        work: DspBufferWork::new(role, task),
    };
    let mut bus = Bus::new(&case.before, case.bank ^ u16::from(wrong_bank));
    let mut input = 0;
    while !execution.work.complete() {
        let clock = execution.work.elapsed();
        while let Some(v) = case.inputs.get(input)
            && v[0] == clock
        {
            bus.ram[v[1] as usize] = v[2] as u16;
            input += 1;
        }
        let mut until = clock + partition;
        if let Some(v) = case.inputs.get(input) {
            until = until.min(v[0]);
        }
        assert!(until > clock);
        execution.advance_until(until, &mut bus);
    }
    (execution.work.elapsed(), bus.events.clone(), bus.snapshot())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut input = Reader {
        bytes: fs::read(out.join("dsp-buffer-jobs-original.bin"))?,
        offset: 0,
    };
    if input.word() != 0x44424a31 {
        return Err("Unsupported original DSP buffer observations".into());
    }
    let (
        mut cases,
        mut stream_errors,
        mut state_errors,
        mut clock_errors,
        mut stores,
        mut state_bytes,
        mut rejected,
    ) = (0u32, 0u32, 0u32, 0u32, 0u64, 0u64, 0u32);
    let mut counts = [[0u32; 5]; 2];
    let mut zero_bank = 0;
    let mut nonzero_bank = 0;
    let mut first = Value::Null;
    while input.offset < input.bytes.len() - 4 {
        let (chip, variant, kind, bank) = (
            input.word(),
            input.word(),
            input.word(),
            input.word() as u16,
        );
        let before = input.snapshot();
        let inputs = input.rows();
        let duration = input.word();
        let events = input
            .rows::<4>()
            .into_iter()
            .map(|v| Event {
                clock: v[0],
                address: v[1] as u16,
                value: v[2] as u16,
                space: v[3] as u8,
            })
            .collect::<Vec<_>>();
        let after = input.snapshot();
        let case = Case {
            chip,
            kind,
            bank,
            before,
            inputs,
            duration,
            events,
            after,
        };
        counts[chip as usize][kind as usize] += 1;
        zero_bank += u32::from(bank == 0);
        nonzero_bank += u32::from(bank != 0);
        for partition in [1, 7, 31, 3000] {
            let (clock, events, after) = run(&case, partition, false);
            stores += events.len() as u64;
            state_bytes += (SNAPSHOT_WORDS * 2) as u64;
            clock_errors += u32::from(clock != case.duration);
            stream_errors += u32::from(events != case.events);
            state_errors += u32::from(after != case.after);
            if first.is_null()
                && (clock != case.duration || events != case.events || after != case.after)
            {
                let p = events
                    .iter()
                    .zip(&case.events)
                    .position(|(a, b)| a != b)
                    .unwrap_or(events.len().min(case.events.len()));
                first = json!({"chip":chip,"variant":variant,"kind":kind,"bank":bank,"partition":partition,
                    "native_clock":clock,"original_clock":case.duration,"position":p,
                    "native_event":events.get(p).map(|v|format!("{v:?}")),
                    "original_event":case.events.get(p).map(|v|format!("{v:?}")),
                    "native_stores":events.len(),"original_stores":case.events.len(),
                    "first_word_difference":after.iter().zip(&case.after).position(|(a,b)|a!=b)});
            }
            if variant == 0 && kind == 1 {
                let (clock, events, after) = run(&case, partition, true);
                rejected += u32::from(
                    clock != case.duration || events != case.events || after != case.after,
                );
            }
        }
        cases += 1;
    }
    let instructions = input.word();
    assert_eq!(input.offset, input.bytes.len());
    let passed = cases == 2560
        && stream_errors + state_errors + clock_errors == 0
        && rejected == 8
        && counts.iter().all(|row| row.iter().all(|v| *v == 256));
    let report = json!({"passed":passed,"original_whole_jobs":cases,"cases_by_chip_and_task":counts,
        "original_instruction_packets":instructions,"timed_native_stores_compared":stores,
        "compared_final_state_bytes":state_bytes,"stream_errors":stream_errors,"state_errors":state_errors,
        "clock_errors":clock_errors,"first_difference":first,"zero_bank_jobs":zero_bank,"nonzero_bank_jobs":nonzero_bank,
        "advance_partitions":[1,7,31,3000],"wrong_bank_controls_rejected":rejected,
        "timed_external_input_changes_included":true,"native_reads_latched_at_original_service_clock":true,
        "native_copy_lanes_slave_input_merge_and_output_ring_computed":true,
        "native_noop_mailbox_cleanup_and_ready_write_clocks_computed":true,
        "recorded_job_budgets_or_store_outputs_used_as_native_inputs":false,
        "peripheral_interrupts_disabled_in_original_jobs":true,
        "actual_DMA_audio_voice_job_and_all_mailbox_handler_timing_qualified":false});
    fs::write(
        out.join("dsp-buffer-jobs-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "{cases} whole native DSP buffer jobs, {stores} timed stores: {} errors",
        stream_errors + state_errors + clock_errors
    );
    if !passed {
        return Err("Native DSP buffer jobs differ".into());
    }
    Ok(())
}
