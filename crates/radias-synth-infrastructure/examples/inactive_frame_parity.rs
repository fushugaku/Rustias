//! Complete original inactive frame RAM, ordered store stream and completion.
use radias_synth_application::inactive_frame::InactiveFrameExecution;
use radias_synth_domain::{
    dsp_buffers::{DspBufferBus, DspRole},
    inactive_frame::{InactiveFrameError, InactiveFrameWork},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
const REGIONS: [(usize, usize); 5] = [
    (0x462, 128),
    (0x2000, 1920),
    (0x3000, 768),
    (0x4000, 50),
    (0x3800, 1),
];
const WORDS: usize = 2867;
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
    fn snapshot(&mut self) -> Vec<u16> {
        (0..WORDS).map(|_| self.word() as u16).collect()
    }
    fn rows<const N: usize>(&mut self) -> Vec<[u32; N]> {
        let count = self.word();
        (0..count)
            .map(|_| core::array::from_fn(|_| self.word()))
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Store {
    clock: u32,
    address: u16,
    value: u16,
}
struct Bus {
    ram: Vec<u16>,
    stores: Vec<Store>,
}
impl Bus {
    fn new(snapshot: &[u16]) -> Self {
        let mut ram = vec![0; 0x10000];
        let mut offset = 0;
        for (start, count) in REGIONS {
            ram[start..start + count].copy_from_slice(&snapshot[offset..offset + count]);
            offset += count;
        }
        Self {
            ram,
            stores: Vec::new(),
        }
    }
    fn snapshot(&self) -> Vec<u16> {
        REGIONS
            .into_iter()
            .flat_map(|(s, n)| self.ram[s..s + n].iter().copied())
            .collect()
    }
}
impl DspBufferBus for Bus {
    fn read_data(&self, address: u16) -> u16 {
        self.ram[address as usize]
    }
    fn write_data(&mut self, clock: u32, address: u16, value: u16) {
        self.ram[address as usize] = value;
        self.stores.push(Store {
            clock,
            address,
            value,
        });
    }
    fn write_io(&mut self, _: u32, _: u16, _: u16) {
        panic!("Inactive frame unexpectedly writes IO");
    }
    fn host_control(&self) -> u16 {
        0
    }
    fn set_host_control(&mut self, _: u32, _: u16) {
        panic!("Inactive frame unexpectedly changes mailbox readiness");
    }
}
struct Case {
    chip: u32,
    frame: u8,
    before: Vec<u16>,
    inputs: Vec<[u32; 3]>,
}
fn run(case: &Case, partition: u32, wrong_gain: bool) -> (u32, Vec<Store>, Vec<u16>) {
    let mut bus = Bus::new(&case.before);
    if wrong_gain {
        bus.ram[0x300c] ^= 0x4000;
    }
    let role = if case.chip == 0 {
        DspRole::Master
    } else {
        DspRole::Slave
    };
    let mut work = InactiveFrameExecution {
        work: InactiveFrameWork::prepare(role, case.frame, &bus)
            .expect("Declared inactive input rejected"),
    };
    let mut input = 0;
    while !work.work.complete() {
        let clock = work.work.elapsed();
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
        work.advance_until(until, &mut bus)
            .expect("Unexpected active voice/vocoder input");
    }
    (work.work.elapsed(), bus.stores.clone(), bus.snapshot())
}
fn rejects(before: &[u16], role: DspRole) -> u32 {
    let mut count = 0;
    let mut bus = Bus::new(before);
    assert_eq!(
        InactiveFrameWork::prepare(role, 4, &bus).err(),
        Some(InactiveFrameError::InvalidFrame)
    );
    count += 1;
    bus.ram[0x3800] = 1;
    assert_eq!(
        InactiveFrameWork::prepare(role, 0, &bus).err(),
        Some(InactiveFrameError::VocoderEnabled)
    );
    count += 1;
    bus.ram[0x3800] = 0;
    for slot in 0..12 {
        bus.ram[0x2000 + 160 * slot] = 1;
        assert_eq!(
            InactiveFrameWork::prepare(role, 0, &bus).err(),
            Some(InactiveFrameError::ActiveVoice(slot as u8))
        );
        count += 1;
        bus.ram[0x2000 + 160 * slot] = 0;
    }
    assert!(bus.stores.is_empty());
    count
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut input = Reader {
        bytes: fs::read(out.join("inactive-frame-original.bin"))?,
        offset: 0,
    };
    if input.word() != 0x49465231 {
        return Err("Unsupported inactive frame observation".into());
    }
    let (
        mut cases,
        mut streams,
        mut states,
        mut clocks,
        mut stores,
        mut state_bytes,
        mut negative,
        mut unsupported,
    ) = (0u32, 0u32, 0u32, 0u32, 0u64, 0u64, 0u32, 0u32);
    let mut first = Value::Null;
    let mut counts = [[0u32; 4]; 2];
    while input.offset < input.bytes.len() - 4 {
        let (chip, variant, frame) = (input.word(), input.word(), input.word() as u8);
        let before = input.snapshot();
        let external = input.rows();
        let duration = input.word();
        let original = input
            .rows::<3>()
            .into_iter()
            .map(|v| Store {
                clock: v[0],
                address: v[1] as u16,
                value: v[2] as u16,
            })
            .collect::<Vec<_>>();
        let after = input.snapshot();
        let case = Case {
            chip,
            frame,
            before,
            inputs: external,
        };
        for partition in [1, 7, 31, 3000] {
            let (clock, native, state) = run(&case, partition, false);
            stores += native.len() as u64;
            state_bytes += 2 * WORDS as u64;
            clocks += u32::from(clock != duration);
            streams += u32::from(native != original);
            states += u32::from(state != after);
            if first.is_null() && (clock != duration || native != original || state != after) {
                let p = native
                    .iter()
                    .zip(&original)
                    .position(|(a, b)| a != b)
                    .unwrap_or(native.len().min(original.len()));
                first = json!({"chip":chip,"variant":variant,"frame":frame,"partition":partition,
                    "native_clock":clock,"original_clock":duration,"position":p,
                    "native_store":native.get(p).map(|s|format!("{s:?}")),"original_store":original.get(p).map(|s|format!("{s:?}")),
                    "native_count":native.len(),"original_count":original.len(),"first_word_difference":state.iter().zip(&after).position(|(a,b)|a!=b)});
            }
            if variant == 0 && frame == 0 {
                let (clock, native, state) = run(&case, partition, true);
                negative += u32::from(clock != duration || native != original || state != after);
            }
        }
        if variant == 0 && frame == 0 {
            unsupported += rejects(
                &case.before,
                if chip == 0 {
                    DspRole::Master
                } else {
                    DspRole::Slave
                },
            );
        }
        cases += 1;
        counts[chip as usize][frame as usize] += 1;
    }
    let instructions = input.word();
    assert_eq!(input.offset, input.bytes.len());
    let passed = cases == 2048
        && streams + states + clocks == 0
        && negative == 8
        && unsupported == 28
        && counts == [[256; 4]; 2];
    let report = json!({"passed":passed,"original_whole_inactive_frames":cases,"cases_by_chip_and_frame":counts,
        "original_instruction_packets":instructions,"ordered_native_stores_compared":stores,"final_state_bytes_compared":state_bytes,
        "stream_errors":streams,"state_errors":states,"clock_errors":clocks,"first_difference":first,
        "wrong_cache_gain_controls_rejected":negative,"unsupported_inputs_rejected_before_mutation":unsupported,
        "advance_partitions":[1,7,31,3000],"timed_external_input_changes_included":true,
        "native_ingress_follower_peak_decay_all12_inactive_slots_scaling_and_output_computed":true,
        "recorded_store_values_or_frame_durations_used_as_native_inputs":false,
        "other_physical_history_and_all_parameter_words_compared":true,
        "active_voice_vocoder_IRQ_DMA_and_whole_instrument_audio_qualified":false});
    fs::write(
        out.join("inactive-frame-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "{cases} whole inactive native frames, {stores} stores: {} errors",
        streams + states + clocks
    );
    if !passed {
        return Err("Native inactive frame differs from original firmware".into());
    }
    Ok(())
}
