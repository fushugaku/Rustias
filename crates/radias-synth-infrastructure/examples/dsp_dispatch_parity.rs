//! Full inactive frames and buffer/no-op work generate their own data, clocks
//! and readiness. Original jobs and RAM store streams are assertions only.
use radias_synth_application::dsp_dispatch::{DspDispatchExecution, DspTaskPort};
use radias_synth_domain::dsp_buffers::{DspBufferBus, DspBufferTask, DspBufferWork, DspRole};
use radias_synth_domain::dsp_dispatch::{DispatchStage, DspTask};
use radias_synth_domain::inactive_frame::InactiveFrameWork;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

struct Reader {
    data: Vec<u8>,
    offset: usize,
}
impl Reader {
    fn word(&mut self) -> u32 {
        let value = u32::from_le_bytes(self.data[self.offset..self.offset + 4].try_into().unwrap());
        self.offset += 4;
        value
    }
    fn rows<const N: usize>(&mut self) -> Vec<[u32; N]> {
        let count = self.word();
        (0..count)
            .map(|_| core::array::from_fn(|_| self.word()))
            .collect()
    }
}
#[derive(Clone, Copy)]
struct Job {
    task: DspTask,
    start: u64,
    duration: u32,
    ready: u32,
}
struct Case {
    chip: u32,
    scenario: u32,
    end: u64,
    inputs: Vec<[u32; 3]>,
    jobs: Vec<Job>,
    observations: Vec<[u32; 5]>,
    initial_ram: Vec<u16>,
    stores: Vec<(u64, u16, u16)>,
    final_ram: Vec<u16>,
}
fn task(kind: u32, frame: u32) -> DspTask {
    match kind {
        0 => DspTask::Mailbox,
        1 => DspTask::PublishOutputBuffer,
        2 => DspTask::LoadInputBuffer,
        3 => DspTask::AdvanceOutputRing,
        4 => DspTask::ResetSynthesisWorkspace,
        5 => DspTask::SynthesizeFrame(frame as u8),
        _ => panic!("Unknown observed job"),
    }
}
fn control_pc(stage: DispatchStage) -> Option<u32> {
    Some(match stage {
        DispatchStage::Main(n) => [
            0xecad, 0xecb3, 0xecb6, 0xecb9, 0xecbe, 0xecc0, 0xecc5, 0xecc8, 0xeccc, 0xecd1, 0xecd3,
            0xecd4,
        ][n as usize],
        DispatchStage::FrameSetup(n) => {
            [0xe9e4, 0xe9e9, 0xe9eb, 0xe9ee, 0xe9f1, 0xe9f5][n as usize]
        }
        DispatchStage::FramePoll { frame, phase } => [
            [0xe9f9, 0xe9ff, 0xea02, 0xea05, 0xea0a, 0xea0c],
            [0xea10, 0xea16, 0xea19, 0xea1c, 0xea21, 0xea23],
            [0xea27, 0xea2d, 0xea30, 0xea33, 0xea38, 0xea3a],
            [0xea3e, 0xea44, 0xea47, 0xea4a, 0xea4f, 0xea51],
        ][frame as usize][phase as usize],
        DispatchStage::FrameReturn => 0xea55,
        DispatchStage::Working(_) => return None,
    })
}
struct Port<'a> {
    hpic: u16,
    flags: u16,
    jobs: &'a [Job],
    cursor: usize,
    active: Option<Job>,
    task_errors: u32,
    first: Value,
    role: DspRole,
    ram: Vec<u16>,
    native_work: Option<DspBufferWork>,
    ready_write_clock: u32,
    native_frame: Option<InactiveFrameWork>,
    native_job_start: u64,
    stores: Vec<(u64, u16, u16)>,
}
impl<'a> Port<'a> {
    fn new(case: &Case, jobs: &'a [Job]) -> Self {
        Self {
            hpic: 12,
            flags: 0,
            jobs,
            cursor: 0,
            active: None,
            task_errors: 0,
            first: Value::Null,
            role: if case.chip == 0 {
                DspRole::Master
            } else {
                DspRole::Slave
            },
            ram: case.initial_ram.clone(),
            native_work: None,
            ready_write_clock: 0,
            native_frame: None,
            native_job_start: 0,
            stores: Vec::new(),
        }
    }
    fn error(&mut self, value: Value) {
        self.task_errors += 1;
        if self.first.is_null() {
            self.first = value;
        }
    }
}
impl DspBufferBus for Port<'_> {
    fn read_data(&self, address: u16) -> u16 {
        self.ram[address as usize]
    }
    fn write_data(&mut self, clock: u32, address: u16, value: u16) {
        self.ram[address as usize] = value;
        self.stores
            .push((self.native_job_start + u64::from(clock), address, value));
    }
    fn write_io(&mut self, _: u32, _: u16, _: u16) {
        // Actual DMA/device effects are outside this control-only comparison.
    }
    fn host_control(&self) -> u16 {
        self.hpic
    }
    fn set_host_control(&mut self, clock: u32, value: u16) {
        self.hpic = value;
        self.ready_write_clock = clock;
    }
}
impl DspTaskPort for Port<'_> {
    fn hpic(&self) -> u16 {
        self.hpic
    }
    fn frame_flags(&self) -> u16 {
        self.flags
    }
    fn set_frame_flags(&mut self, value: u16, clock: u64) {
        self.flags = value;
        self.ram[0x441] = value;
        self.stores.push((clock, 0x441, value));
    }
    fn begin_task(&mut self, task: DspTask, clock: u64) {
        let Some(job) = self.jobs.get(self.cursor).copied() else {
            self.error(json!({"unexpected_task":format!("{task:?}"),"clock":clock}));
            return;
        };
        if job.task != task || job.start != clock {
            self.error(json!({"task":format!("{task:?}"),"clock":clock,
                "expected_task":format!("{:?}",job.task),"expected_clock":job.start}));
        }
        self.cursor += 1;
        self.active = Some(job);
        self.native_job_start = clock;
        self.native_frame = if let DspTask::SynthesizeFrame(frame) = task {
            Some(
                InactiveFrameWork::prepare(self.role, frame, self)
                    .expect("Comparison input has active voice/vocoder"),
            )
        } else {
            None
        };
        self.ready_write_clock = 0;
        self.native_work = match task {
            DspTask::Mailbox => Some(DspBufferTask::MailboxNoop),
            DspTask::PublishOutputBuffer => Some(DspBufferTask::PublishOutput),
            DspTask::LoadInputBuffer => Some(DspBufferTask::LoadInput),
            DspTask::AdvanceOutputRing => Some(DspBufferTask::AdvanceOutputRing),
            DspTask::ResetSynthesisWorkspace => Some(DspBufferTask::BeginSynthesisBatch),
            DspTask::SynthesizeFrame(_) => None,
        }
        .map(|task| DspBufferWork::new(self.role, task));
    }
    fn task_clock(&mut self, _: DspTask, elapsed: u32, _: u64) -> bool {
        if let Some(mut work) = self.native_work.take() {
            assert_eq!(work.elapsed(), elapsed);
            let complete = work.step(self);
            self.native_work = Some(work);
            return complete;
        }
        if let Some(mut work) = self.native_frame.take() {
            assert_eq!(work.elapsed(), elapsed);
            let complete = work
                .step(self)
                .expect("Unexpected active voice/vocoder during native frame");
            self.native_frame = Some(work);
            return complete;
        }
        panic!("Native DSP task has no work implementation")
    }
    fn end_task(&mut self, _: DspTask, clock: u64) {
        if let Some(job) = self.active.take() {
            if clock != job.start + u64::from(job.duration) {
                self.error(
                    json!({"return_clock":clock,"expected":job.start+u64::from(job.duration)}),
                );
            }
            if job.task == DspTask::Mailbox && self.ready_write_clock != job.ready {
                self.error(json!({"ready_offset":self.ready_write_clock,"expected":job.ready}));
            }
        }
    }
}
struct ResultCounts {
    states: u64,
    control_clocks: u64,
    state_errors: u32,
    task_errors: u32,
    first: Value,
    data_errors: u32,
    stores: u64,
}
fn compare(case: &Case, jobs: &[Job], partition: u64) -> ResultCounts {
    let mut execution = DspDispatchExecution::default();
    let mut port = Port::new(case, jobs);
    let mut input = 0;
    let mut result = ResultCounts {
        states: 0,
        control_clocks: 0,
        state_errors: 0,
        task_errors: 0,
        first: Value::Null,
        data_errors: 0,
        stores: 0,
    };
    loop {
        while let Some(v) = case.inputs.get(input)
            && u64::from(v[0]) == execution.clock
        {
            if port.hpic & 4 != 0 && v[1] & 4 == 0 {
                port.ram[0x100] = 6;
                port.ram[0x101] = 13;
            }
            port.hpic = v[1] as u16;
            port.flags = v[2] as u16;
            port.ram[0x441] = port.flags;
            input += 1;
        }
        let observed = case.observations[execution.clock as usize];
        assert_eq!(u64::from(observed[0]), execution.clock);
        let state_diff = port.hpic != observed[1] as u16
            || port.flags != observed[2] as u16
            || execution.control.interrupts_masked != (observed[3] != 0);
        let pc = control_pc(execution.control.stage);
        let pc_diff = pc.is_some_and(|v| v != observed[4]);
        result.states += 1;
        result.control_clocks += u64::from(pc.is_some());
        if state_diff || pc_diff {
            result.state_errors += 1;
            if result.first.is_null() {
                result.first = json!({"chip":case.chip,"scenario":case.scenario,"partition":partition,
                    "clock":execution.clock,"stage":format!("{:?}",execution.control.stage),
                    "native":[port.hpic,port.flags,u16::from(execution.control.interrupts_masked)],
                    "original":observed,"native_control_pc":pc});
            }
        }
        if execution.clock == case.end {
            break;
        }
        let mut until = (execution.clock + partition).min(case.end);
        if let Some(v) = case.inputs.get(input) {
            until = until.min(u64::from(v[0]));
        }
        assert!(until > execution.clock);
        execution.advance_until(until, &mut port);
    }
    if port.cursor != jobs.len() || port.active.is_some() || execution.task().is_some() {
        port.error(json!({"completed_jobs":port.cursor,"expected_jobs":jobs.len(),"unfinished":execution.task().is_some()}));
    }
    result.task_errors = port.task_errors;
    result.stores = port.stores.len() as u64;
    let final_ram = DATA_REGIONS
        .into_iter()
        .flat_map(|(s, n)| port.ram[s..s + n].iter().copied())
        .collect::<Vec<_>>();
    if port.stores != case.stores || final_ram != case.final_ram {
        result.data_errors += 1;
        if result.first.is_null() {
            let p = port
                .stores
                .iter()
                .zip(&case.stores)
                .position(|(a, b)| a != b)
                .unwrap_or(port.stores.len().min(case.stores.len()));
            result.first = json!({"chip":case.chip,"scenario":case.scenario,"partition":partition,"data_position":p,
                "native_store":port.stores.get(p),"original_store":case.stores.get(p),
                "native_store_count":port.stores.len(),"original_store_count":case.stores.len(),
                "first_word_difference":final_ram.iter().zip(&case.final_ram).position(|(a,b)|a!=b)});
        }
    }
    if result.first.is_null() {
        result.first = port.first;
    }
    result
}
const DATA_REGIONS: [(usize, usize); 8] = [
    (0x100, 2),
    (0x441, 2),
    (0x462, 672),
    (0x1500, 3),
    (0x2000, 1920),
    (0x3000, 768),
    (0x4000, 50),
    (0x3800, 1),
];
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut reader = Reader {
        data: fs::read(out.join("dsp-dispatch-original.bin"))?,
        offset: 0,
    };
    if reader.word() != 0x44535031 {
        return Err("Unsupported DSP dispatch observation".into());
    }
    let mut cases = Vec::new();
    let mut data = Reader {
        data: fs::read(out.join("dsp-dispatch-data-original.bin"))?,
        offset: 0,
    };
    if data.word() != 0x44444131 {
        return Err("Unsupported dispatch data observation".into());
    }
    let mut snapshots = Vec::new();
    for chip in 0..2 {
        let raw = fs::read(out.join(format!(
            "construction-visible-state-source-construction-dsp{chip}.bin"
        )))?;
        if raw.len() != 0x200000 * 4 {
            return Err("Live DSP input memory missing".into());
        }
        let mut words = raw[..0x10000 * 4]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as u16)
            .collect::<Vec<_>>();
        words[0x441] = 0;
        words[0x3800] = 0;
        snapshots.push(words);
    }
    while reader.offset < reader.data.len() - 4 {
        let (chip, scenario, end) = (reader.word(), reader.word(), u64::from(reader.word()));
        let inputs = reader.rows();
        let jobs = reader
            .rows::<5>()
            .into_iter()
            .map(|v| Job {
                task: task(v[0], v[1]),
                start: u64::from(v[2]),
                duration: v[3],
                ready: v[4],
            })
            .collect();
        let observations = reader.rows();
        assert_eq!(data.word(), chip);
        assert_eq!(data.word(), scenario);
        assert_eq!(u64::from(data.word()), end);
        let stores = data
            .rows::<3>()
            .into_iter()
            .map(|v| (u64::from(v[0]), v[1] as u16, v[2] as u16))
            .collect();
        let final_ram = (0..3418).map(|_| data.word() as u16).collect();
        cases.push(Case {
            chip,
            scenario,
            end,
            inputs,
            jobs,
            observations,
            initial_ram: snapshots[chip as usize].clone(),
            stores,
            final_ram,
        });
    }
    let instructions = reader.word();
    assert_eq!(reader.offset, reader.data.len());
    assert_eq!(data.offset, data.data.len());
    let mut idle = DspDispatchExecution::default();
    let mut idle_port = Port::new(&cases[0], &[]);
    let started = std::time::Instant::now();
    idle.advance_until(std::hint::black_box(300_000_007), &mut idle_port);
    let idle_ns = started.elapsed().as_nanos();
    assert_eq!(idle.clock, 300_000_007);
    assert_eq!(idle.control.stage, DispatchStage::Main(7));
    assert_eq!(idle_port.cursor, 0);
    let mut totals = ResultCounts {
        states: 0,
        control_clocks: 0,
        state_errors: 0,
        task_errors: 0,
        first: Value::Null,
        data_errors: 0,
        stores: 0,
    };
    let mut frames = [[0u32; 4]; 2];
    let mut jobs = 0;
    let mut faults_rejected = 0;
    for case in &cases {
        jobs += case.jobs.len();
        for j in &case.jobs {
            if let DspTask::SynthesizeFrame(f) = j.task {
                frames[case.chip as usize][f as usize] += 1;
            }
        }
        for partition in [1, 7, 31, 3000] {
            let r = compare(case, &case.jobs, partition);
            totals.states += r.states;
            totals.control_clocks += r.control_clocks;
            totals.state_errors += r.state_errors;
            totals.task_errors += r.task_errors;
            totals.data_errors += r.data_errors;
            totals.stores += r.stores;
            if totals.first.is_null() {
                totals.first = r.first;
            }
            if case.scenario == 0 {
                let mut changed = case.jobs.clone();
                changed
                    .iter_mut()
                    .find(|j| j.task == DspTask::PublishOutputBuffer)
                    .unwrap()
                    .duration += 1;
                let r = compare(case, &changed, partition);
                faults_rejected += u32::from(r.state_errors + r.task_errors != 0);
            }
        }
    }
    let passed = cases.len() == 64
        && totals.state_errors == 0
        && totals.task_errors == 0
        && totals.data_errors == 0
        && faults_rejected == 8
        && frames
            .iter()
            .all(|f| f.iter().all(|n| *n == f[0] && *n != 0));
    let report = json!({"passed":passed,"original_scenarios":cases.len(),"original_complete_jobs":jobs,
        "original_instruction_packets":instructions,"state_checkpoints":totals.states,
        "control_pc_checkpoints":totals.control_clocks,"state_errors":totals.state_errors,
        "task_order_start_and_return_errors":totals.task_errors,"first_difference":totals.first,
        "frame_job_counts_by_chip":frames,"advance_partitions":[1,7,31,3000],
        "one_clock_wrong_job_budgets_rejected":faults_rejected,
        "host_and_frame_reads_latched_at_native_checkpoints":true,
        "synthesis_frame_durations_are_declared_inputs":false,
        "native_complete_inactive_frame_data_and_clocks_computed":true,
        "continuous_native_RAM_stores_compared":totals.stores,
        "data_store_and_final_RAM_errors":totals.data_errors,
        "buffer_job_durations_and_noop_ready_offsets_computed_independently":true,
        "buffer_job_original_durations_and_ready_offsets_are_assertions_only":true,
        "idle_control_clocks_advanced":300_000_007u64,
        "idle_control_advance_ns":idle_ns,
        "idle_skip_preserves_partial_final_poll":true,
        "original_job_start_clocks_are_assertions_only":true,
        "peripheral_interrupts_disabled_in_control_comparison":true,
        "independent_audio_job_budgets_and_receiver_writes_qualified":false,
        "independent_whole_audio_timing_qualified":false});
    fs::write(
        out.join("dsp-dispatch-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "64 DSP dispatch cases, {jobs} jobs, {} state checkpoints: {} errors",
        totals.states,
        totals.state_errors + totals.task_errors
    );
    if !passed {
        return Err("DSP dispatcher differs from original control flow".into());
    }
    Ok(())
}
