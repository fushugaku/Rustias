//! Machine lifecycle and user commands. No UI, file or audio-device dependency.
use radias_domain::{
    backup::{self, BackupExtras},
    board::Board,
    controller::{Bus, Sh3},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmPolicy {
    SilentGuards,
    NativeFlash,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupScope {
    Full,
    GlobalOnly,
}
#[derive(Default)]
struct PcmOffProfile {
    transferring: bool,
    saved_registers: [u32; 16],
    saved_pr: u32,
    scratch_values: [u16; 2],
}
impl PcmOffProfile {
    fn before_step(&mut self, cpu: &mut Sh3, board: &mut Board) -> bool {
        const LOADER: u32 = 0x0c082184;
        const SCRATCH: u32 = 0x0cfffffc;
        const SAMPLE_BASE: u32 = 0x48000;
        if cpu.pc != LOADER || cpu.delayed {
            return false;
        }
        if !self.transferring {
            self.saved_registers = cpu.r;
            self.saved_pr = cpu.pr;
            for i in 0..2 {
                self.scratch_values[i] = board.read16(SCRATCH + 2 * i as u32);
            }
            board.write32(0x0c1725b4, 1);
            for a in [0x0c16a19c, 0x0c16a1a0, 0x0c16a1a4, 0x0c16a1a8, 0x0c16a1ac] {
                board.write32(a, 0);
            }
            for (a, n) in [
                (0x0c16d1b0, 20),
                (0x0c16e5b0, 12),
                (0x0c16a1b0, 16),
                (0x0c16b1b0, 8),
                (0x0c1715b0, 16),
            ] {
                for i in 0..n {
                    board.write8(a + i, 0);
                }
            }
            board.write32(0x0c16d1b8, SAMPLE_BASE);
            board.write32(0x0c16e5b0, SAMPLE_BASE);
            board.write32(0x0c16a1a0, 1);
            board.write8(0x0c16b1b0, 127);
            for i in 0..2 {
                board.write16(SCRATCH + 2 * i, 0);
            }
            cpu.r[4] = SAMPLE_BASE;
            cpu.r[5] = SCRATCH;
            cpu.r[6] = 4;
            cpu.r[7] = 4;
            cpu.pr = LOADER;
            cpu.pc = 0x0c02a602;
            self.transferring = true;
        } else {
            if cpu.r[15] != self.saved_registers[15] {
                std::panic::panic_any(
                    "PCM profile transfer did not restore the SH stack".to_string(),
                );
            }
            cpu.r = self.saved_registers;
            cpu.r[0] = 1;
            cpu.pr = self.saved_pr;
            cpu.pc = self.saved_pr;
            for i in 0..2 {
                board.write16(SCRATCH + 2 * i as u32, self.scratch_values[i]);
            }
            board.pcm_skipped = true;
            board.pcm_placeholder_words = 2;
            self.transferring = false;
        }
        true
    }
}
/// Instrumentation is an application port; file formats stay in infrastructure.
pub trait StepObserver: Send {
    fn before_step(&mut self, _cpu: &Sh3, _board: &mut Board) -> Result<(), String> {
        Ok(())
    }
    fn reset(&mut self, _board: &Board) -> Result<(), String> {
        Ok(())
    }
    fn finish(&mut self, _cpu: &Sh3, _board: &mut Board) -> Result<(), String> {
        Ok(())
    }
}
pub struct Machine {
    pub cpu: Sh3,
    pub board: Board,
    pub fault: String,
    pub pcm_policy: PcmPolicy,
    pub step_observer: Option<Box<dyn StepObserver>>,
    pcm_off: PcmOffProfile,
}
impl Machine {
    pub fn new(
        firmware: Vec<u8>,
        librarian: Option<&[u8]>,
        pcm_policy: PcmPolicy,
    ) -> Result<Self, String> {
        Self::new_scoped(firmware, librarian, pcm_policy, BackupScope::Full)
    }
    pub fn new_scoped(
        firmware: Vec<u8>,
        librarian: Option<&[u8]>,
        pcm_policy: PcmPolicy,
        scope: BackupScope,
    ) -> Result<Self, String> {
        if firmware.len() != 0xe0000 {
            return Err("Expected the 0xe0000-byte extracted RADIAS SYS payload".into());
        }
        let (global, library, extras) = if let Some(data) = librarian {
            if scope == BackupScope::GlobalOnly {
                (backup::global(data)?, Vec::new(), BackupExtras::default())
            } else {
                let extras = backup::extras(data)?;
                (
                    backup::global(data)?,
                    backup::native_usr_flash(data, &extras)?,
                    extras,
                )
            }
        } else {
            (Vec::new(), Vec::new(), BackupExtras::default())
        };
        Ok(Self {
            cpu: Sh3::default(),
            board: Board::new(firmware, global, library, extras)?,
            fault: String::new(),
            pcm_policy,
            step_observer: None,
            pcm_off: PcmOffProfile::default(),
        })
    }
    pub fn reset(&mut self) {
        if let Some(observer) = &mut self.step_observer {
            if let Err(error) = observer.reset(&self.board) {
                self.fault = error;
                return;
            }
        }
        self.board.reset();
        self.cpu.reset();
        self.pcm_off = PcmOffProfile::default();
        self.fault.clear();
    }
    pub fn finish_observation(&mut self) -> Result<(), String> {
        if let Some(observer) = &mut self.step_observer {
            observer.finish(&self.cpu, &mut self.board)?;
        }
        Ok(())
    }
    pub fn run_steps(&mut self, budget: u64, end_audio_frame: Option<u64>) {
        if !self.fault.is_empty() {
            return;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..budget {
                if end_audio_frame.is_some_and(|end| self.board.audio_frames >= end) {
                    break;
                }
                self.board.current_pc = self.cpu.pc;
                if self.pcm_policy == PcmPolicy::SilentGuards
                    && self.pcm_off.before_step(&mut self.cpu, &mut self.board)
                {
                    continue;
                }
                if let Some(observer) = &mut self.step_observer {
                    if let Err(error) = observer.before_step(&self.cpu, &mut self.board) {
                        self.fault = error;
                        break;
                    }
                }
                self.cpu.step(&mut self.board);
                for chip in 0..2 {
                    if !self.board.dsp[chip].fault.is_empty() {
                        self.fault = format!(
                            "{} DSP: {}",
                            if chip == 0 { "Master" } else { "Slave" },
                            self.board.dsp[chip].fault
                        );
                        break;
                    }
                }
                if !self.fault.is_empty() {
                    break;
                }
            }
        }));
        if let Err(error) = outcome {
            self.fault = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "Rust machine execution panic".into());
        }
    }
    pub fn run_frames(&mut self, n: u64) {
        self.run_steps(u64::MAX, Some(self.board.audio_frames + n));
    }
    pub fn midi(&mut self, bytes: &[u8]) {
        self.board.midi_in.extend(bytes.iter().copied());
    }
    pub fn key(&mut self, row: usize, column: u8, down: bool) -> Result<(), String> {
        if row >= 8 || column >= 8 {
            return Err("Invalid panel key".into());
        }
        if down {
            self.board.panel_keys[row] |= 1 << column;
        } else {
            self.board.panel_keys[row] &= !(1 << column);
        }
        Ok(())
    }
    pub fn pot(&mut self, channel: usize, mux: usize, value: u16) -> Result<(), String> {
        if channel >= 5 || mux >= 8 || value > 1023 || (channel == 4 && mux >= 4) {
            return Err("Invalid panel potentiometer".into());
        }
        self.board.panel_adc[channel][mux] = value;
        Ok(())
    }
}

pub use radias_domain::program::{PROGRAM_BYTES, Program};
