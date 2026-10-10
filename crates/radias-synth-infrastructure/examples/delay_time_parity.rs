use radias_synth_domain::delay_time::{DelayClock, DelayTimeState, encode_delay_frames};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
struct Reader {
    words: Vec<u32>,
    cursor: usize,
}
impl Reader {
    fn one(&mut self) -> u32 {
        let value = self.words[self.cursor];
        self.cursor += 1;
        value
    }
    fn array<const N: usize>(&mut self) -> [u32; N] {
        core::array::from_fn(|_| self.one())
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&source)?.delay_time_tables()?;
    let raw = fs::read(root.join("runs/native-clone/delay-time-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated delay core corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x44544d31 {
        return Err("Wrong delay core corpus".into());
    }
    let (mut calls, mut encodings, mut feedback_calls, mut errors) =
        (0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    let mut family_counts = [0usize; 2];
    while read.cursor < read.words.len() {
        match read.one() {
            0x1000 => {
                let family = read.one();
                let parameters = read.bytes();
                let [cached_tempo, capacity, ratio, limited, tempo, status] = read.array::<6>();
                let state = DelayTimeState {
                    cached_tempo: cached_tempo as u16,
                    capacity,
                    ratio,
                    limited,
                };
                let clock = DelayClock {
                    tempo: tempo as u16,
                    status: status as u8,
                };
                let original = read.array::<7>();
                let native = if family == 0 {
                    tables.lcr(&parameters, state, clock)
                } else {
                    tables.stereo(&parameters, state, clock)
                }
                .ok_or("Native delay core rejected source inputs")?;
                let native_words = [
                    native.frames[0],
                    native.frames[1],
                    native.frames[2],
                    u32::from(native.state.cached_tempo),
                    native.state.capacity,
                    native.state.ratio,
                    native.state.limited,
                ];
                if native_words != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"family":family,"parameters":parameters,"before":[cached_tempo,capacity,ratio,limited],"clock":[tempo,status],"native":native_words,"original":original});
                    }
                }
                calls += 1;
                family_counts[family as usize] += 1;
            }
            0x2000 => {
                let frames = read.one();
                let shift = read.one();
                let original = read.one();
                let native = encode_delay_frames(frames, shift);
                if native != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"frames":frames,"shift":shift,"native":native,"original":original});
                    }
                }
                encodings += 1;
            }
            0x3000 => {
                let left = read.one();
                let right = read.one();
                let feedback = read.one();
                let original = read.one();
                let native = tables
                    .feedback_limit(left, right, feedback as u8)
                    .ok_or("Native feedback rejected source inputs")?;
                if native != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"left":left,"right":right,"feedback":feedback,"native":native,"original":original});
                    }
                }
                feedback_calls += 1;
            }
            _ => return Err("Unexpected delay record".into()),
        }
    }
    let passed = calls == 524288
        && family_counts == [262144, 262144]
        && encodings == 88
        && feedback_calls == 15488
        && errors == 0;
    let report = json!({"passed":passed,"whole_original_delay_time_calls":calls,"family_counts":family_counts,"whole_original_encoding_calls":encodings,"whole_original_feedback_limit_calls":feedback_calls,"errors":errors,"first_difference":first,"all_free_ratio_and_delay_byte_domains":true,"free_and_sync_modes_capacity_and_clock_status_profiles":true,"raw_declared_state_and_clock_inputs_only":true,"original_output_states_or_times_replayed_as_inputs":false,"original_instructions_or_callees_modified":false,"complete_parameter_dispatch_or_FXD03_sound_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/delay-time-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native delay core: {calls} original times, {encodings} encodings, {feedback_calls} feedback calls, {errors} differences"
    );
    if !passed {
        return Err("Native delay time core differs".into());
    }
    Ok(())
}
