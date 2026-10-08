//! Original SH linker observations, not execution of the FXD03 ASIC.
use radias_application::StepObserver;
use radias_domain::{
    board::Board,
    controller::{Bus, Sh3},
};
use serde_json::json;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

#[derive(Default)]
struct LinkCall {
    pending: bool,
    buffer: u32,
    count: u32,
    caller: u32,
    stack: u32,
    clock: u64,
    origins: [u32; 3],
    before: Vec<u64>,
}
impl LinkCall {
    fn words(board: &Board, pointer: u32, length: u32) -> Result<Vec<u64>, String> {
        let address = pointer & 0x1fffffff;
        if address < 0x0c000000
            || (address - 0x0c000000) as u64 + length as u64 * 6 > board.ram.len() as u64
        {
            return Err("FXD linker trace buffer is outside controller SDRAM".into());
        }
        Ok(board.ram[(address - 0x0c000000) as usize
            ..(address - 0x0c000000) as usize + length as usize * 6]
            .chunks_exact(6)
            .map(|bytes| {
                bytes
                    .iter()
                    .fold(0u64, |word, &byte| word << 8 | byte as u64)
            })
            .collect())
    }
    fn finish(
        &mut self,
        board: &Board,
        out: &mut impl Write,
        reason: Option<&str>,
    ) -> Result<(), String> {
        if !self.pending {
            return Ok(());
        }
        let mut record = json!({"entry_clock":self.clock,"exit_clock":board.ticks,"caller":self.caller,"buffer":self.buffer,"count":self.count,"data_origin":self.origins[0],"coefficient_origin":self.origins[1],"program_origin":self.origins[2],"complete":reason.is_none(),"asic_execution":false,"before":self.before.iter().map(|word|format!("{word:012x}")).collect::<Vec<_>>()});
        if let Some(reason) = reason {
            record["after"] = json!(null);
            record["reason"] = json!(reason);
        } else {
            record["after"] = json!(
                Self::words(board, self.buffer, self.count)?
                    .iter()
                    .map(|word| format!("{word:012x}"))
                    .collect::<Vec<_>>()
            );
        }
        writeln!(out, "{record}").map_err(|e| format!("Cannot write FXD linker trace: {e}"))?;
        self.pending = false;
        self.before.clear();
        Ok(())
    }
    fn before_step(
        &mut self,
        cpu: &Sh3,
        board: &Board,
        out: &mut impl Write,
    ) -> Result<(), String> {
        if self.pending && cpu.pc == self.caller && cpu.r[15] == self.stack {
            self.finish(board, out, None)?;
        }
        if cpu.pc != 0x0c07494a {
            return Ok(());
        }
        if self.pending {
            return Err("Unexpected nested FXD linker call".into());
        }
        self.buffer = cpu.r[4];
        self.count = cpu.r[5] as u16 as u32;
        self.caller = cpu.pr;
        self.stack = cpu.r[15];
        self.clock = board.ticks;
        let physical_stack = self.stack & 0x1fffffff;
        if physical_stack < 0x0c000000
            || (physical_stack - 0x0c000000) as u64 + 4 > board.ram.len() as u64
        {
            return Err("FXD linker argument is outside controller SDRAM".into());
        }
        let at = (physical_stack - 0x0c000000) as usize;
        let fifth = u32::from_be_bytes(board.ram[at..at + 4].try_into().unwrap());
        self.origins = [
            cpu.r[6] as u16 as u32,
            cpu.r[7] as u16 as u32,
            fifth as u16 as u32,
        ];
        self.before = Self::words(board, self.buffer, self.count)?;
        self.pending = true;
        Ok(())
    }
}
pub struct Diagnostics {
    trace: Option<BufWriter<File>>,
    link: Option<BufWriter<File>>,
    call: LinkCall,
}
impl Diagnostics {
    pub fn new(trace: Option<&Path>, link: Option<&Path>) -> Result<Self, String> {
        let mut trace = trace
            .map(|path| File::create(path).map(BufWriter::new))
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(file) = &mut trace {
            file.write_all(b"step\tpc\top\tsr\tr0\tr1\tr15\n")
                .map_err(|e| e.to_string())?;
        }
        let link = link
            .map(|path| File::create(path).map(BufWriter::new))
            .transpose()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            trace,
            link,
            call: LinkCall::default(),
        })
    }
}
impl StepObserver for Diagnostics {
    fn before_step(&mut self, cpu: &Sh3, board: &mut Board) -> Result<(), String> {
        if let Some(out) = &mut self.link {
            self.call.before_step(cpu, board, out)?;
        }
        if cpu.steps < 200000 {
            if let Some(out) = &mut self.trace {
                let op = board.read16(cpu.pc);
                writeln!(
                    out,
                    "{}\t{:x}\t{:x}\t{:x}\t{:x}\t{:x}\t{:x}",
                    cpu.steps, cpu.pc, op, cpu.sr, cpu.r[0], cpu.r[1], cpu.r[15]
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn reset(&mut self, board: &Board) -> Result<(), String> {
        if let Some(out) = &mut self.link {
            self.call
                .finish(board, out, Some("machine-reset-before-return"))?;
        }
        Ok(())
    }
    fn finish(&mut self, cpu: &Sh3, board: &mut Board) -> Result<(), String> {
        if let Some(out) = &mut self.link {
            self.call.before_step(cpu, board, out)?;
            self.call
                .finish(board, out, Some("trace-ended-before-return"))?;
            out.flush().map_err(|e| e.to_string())?;
        }
        if let Some(out) = &mut self.trace {
            out.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
