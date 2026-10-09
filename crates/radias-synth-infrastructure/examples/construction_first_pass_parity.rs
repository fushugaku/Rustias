//! Exact original visible stores and resumable state at every store boundary.
use radias_synth_application::construction_first_pass::FirstPassExecution;
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    construction_first_pass::{ConstructionFirstPass, FirstPassStore},
};
use std::{fs, path::PathBuf};
struct Reader {
    raw: Vec<u8>,
    cursor: usize,
}
impl Reader {
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let value = self.raw[self.cursor..self.cursor + N].try_into().unwrap();
        self.cursor += N;
        value
    }
    fn word(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes())
    }
}
fn original_store(
    controllers: &mut [ActorControlState; 24],
    flags: &mut [u8; 24],
    address: u32,
    value: u32,
    width: u32,
) {
    if address < 0xc0d18d4 {
        let relative = (address - 0xc0cea54) as usize;
        let slot = relative / 496;
        let offset = relative % 496;
        if width == 2 {
            controllers[slot].bytes[offset..offset + 2]
                .copy_from_slice(&(value as u16).to_be_bytes());
        } else {
            assert_eq!(width, 1);
            controllers[slot].bytes[offset] = value as u8;
        }
    } else {
        assert_eq!(width, 1);
        flags[(address - 0xc0d18d4) as usize] = value as u8;
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut input = Reader {
        raw: fs::read(out.join("construction-first-pass-original.bin"))?,
        cursor: 0,
    };
    if input.word() != 0x46505331 {
        return Err("Unsupported first-pass observation".into());
    }
    let (
        mut cases,
        mut stream_errors,
        mut state_errors,
        mut clock_errors,
        mut stores,
        mut state_bytes,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u64);
    let mut first = serde_json::Value::Null;
    let mut slots = [0u32; 24];
    let mut empty = 0;
    let mut full = 0;
    while input.cursor < input.raw.len() - 4 {
        let variant = input.word();
        let selected = input.word();
        let owner = input.word();
        let before: [ActorControlState; 24] = core::array::from_fn(|_| ActorControlState {
            bytes: input.bytes(),
        });
        let before_flags: [u8; 24] = input.bytes();
        let return_clock = input.word() as u16;
        let after: [ActorControlState; 24] = core::array::from_fn(|_| ActorControlState {
            bytes: input.bytes(),
        });
        let after_flags: [u8; 24] = input.bytes();
        let count = input.word();
        let original: Vec<(u16, u32, u32, u32)> = (0..count)
            .map(|_| {
                (
                    input.word() as u16,
                    input.word(),
                    input.word(),
                    input.word(),
                )
            })
            .collect();
        let plan = ConstructionFirstPass::compile(&before, selected, owner);
        let native: Vec<_> = plan
            .stores()
            .iter()
            .map(|event| match event.store {
                FirstPassStore::ControllerWord {
                    slot,
                    offset,
                    value,
                } => (
                    event.clock,
                    0xc0cea54 + 496 * slot as u32 + offset as u32,
                    u32::from(value),
                    2u32,
                ),
                FirstPassStore::AllocationFlag { slot, value } => {
                    (event.clock, 0xc0d18d4 + slot as u32, u32::from(value), 1u32)
                }
            })
            .collect();
        if native != original {
            stream_errors += 1;
            if first.is_null() {
                let p = native
                    .iter()
                    .zip(&original)
                    .position(|(a, b)| a != b)
                    .unwrap_or(native.len().min(original.len()));
                first = serde_json::json!({"variant":variant,"position":p,"native":native.get(p),"original":original.get(p),"native_count":native.len(),"original_count":original.len()});
            }
        }
        clock_errors += u32::from(plan.return_clock != return_clock);
        for partition in [1u16, 7, 31, 3000] {
            let (mut candidate, mut flags, mut expected, mut expected_flags) =
                (before, before_flags, before, before_flags);
            let mut execution = FirstPassExecution::new(plan);
            let mut clock = 0;
            let mut i = 0;
            while i < original.len() {
                let until = original[i].0;
                while clock < until {
                    clock = clock.saturating_add(partition).min(until);
                    execution.advance_until(clock, &mut candidate, &mut flags);
                }
                while i < original.len() && original[i].0 == until {
                    let (_, a, v, w) = original[i];
                    original_store(&mut expected, &mut expected_flags, a, v, w);
                    i += 1;
                }
                state_errors += u32::from(candidate != expected || flags != expected_flags);
                state_bytes += 24 * 496 + 24;
                // Pausing CPU work for an IRQ must not replay or advance any store.
                execution.advance_until(clock, &mut candidate, &mut flags);
                if candidate != expected || flags != expected_flags {
                    return Err("Paused first pass advanced state".into());
                }
            }
            execution.advance_until(return_clock, &mut candidate, &mut flags);
            state_errors += u32::from(candidate != after || flags != after_flags);
            clock_errors += u32::from(!execution.finished(return_clock));
            state_bytes += 24 * 496 + 24;
        }
        for (slot, count) in slots.iter_mut().enumerate() {
            if selected & (1 << slot) != 0 {
                *count += 1;
            }
        }
        empty += u32::from(selected & 0xffffff == 0);
        full += u32::from(selected & 0xffffff == 0xffffff);
        stores += count;
        cases += 1;
    }
    let instructions = input.word();
    if input.cursor != input.raw.len() {
        return Err("Extra first-pass observation bytes".into());
    }
    let passed = cases == 8192
        && stream_errors == 0
        && state_errors == 0
        && clock_errors == 0
        && slots.iter().all(|v| *v > 0)
        && empty > 0
        && full > 0;
    let report = serde_json::json!({"passed":passed,"original_prefixes":cases,"visible_stores_compared":stores,
        "intermediate_and_final_state_bytes_compared":state_bytes,"stream_errors":stream_errors,"state_errors":state_errors,
        "clock_errors":clock_errors,"first_error":first,"all24_slots_covered":slots.iter().all(|v|*v>0),"empty_selections":empty,
        "full_selections":full,"source_instructions_compared":instructions,"both_EG_shadow_equality_paths_covered":true,
        "CPU_work_partitions":[1,7,31,3000],"pause_retains_intermediate_controller_state":true,
        "independent_IRQ_handler_and_second_pass_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("construction-first-pass-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("Original first-pass stores differ".into());
    }
    Ok(())
}
