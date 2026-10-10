//! Complete original timbre output callers and unchanged C55 receive results.
use radias_synth_application::timbre_output::{TimbreOutputPort, restore_timbre_output};
use radias_synth_domain::{
    dsp_control::DspEndpoint,
    program::Program,
    timbre_output::{TimbreOutputActor, TimbreOutputPlan},
};
use radias_synth_infrastructure::{
    effects::EffectLibrary,
    timbre_output::{DspParameterMemory, NativeTimbreOutputPort},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
struct Reader {
    words: Vec<u32>,
    cursor: usize,
}
impl Reader {
    fn one(&mut self) -> u32 {
        let v = self.words[self.cursor];
        self.cursor += 1;
        v
    }
    fn array<const N: usize>(&mut self) -> [u32; N] {
        core::array::from_fn(|_| self.one())
    }
}
const WINDOWS: [(usize, usize); 3] = [(0x2000, 1920), (0x3000, 768), (0x3800, 256)];
struct Port {
    reject: bool,
    native: NativeTimbreOutputPort,
}
impl TimbreOutputPort for Port {
    type Error = &'static str;
    fn accept_timbre_output(&mut self, plan: &TimbreOutputPlan) -> Result<(), Self::Error> {
        if self.reject {
            return Err("Injected refusal");
        }
        self.native.accept_timbre_output(plan)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&sys)?.timbre_output_tables()?;
    let raw = fs::read(root.join("runs/native-clone/timbre-output-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated timbre-output corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x544f5531 {
        return Err("Wrong timbre-output corpus".into());
    }
    let bindings = r.array::<4>();
    let mut memories =
        core::array::from_fn(|_| DspParameterMemory::from_words(vec![0; 65536]).unwrap());
    for m in &mut memories {
        for (base, count) in WINDOWS {
            for v in &mut m.words_mut()[base..base + count] {
                *v = r.one() as u16;
            }
        }
    }
    let mut port = Port {
        reject: false,
        native: NativeTimbreOutputPort::new(memories),
    };
    let (mut calls, mut packets, mut changes, mut errors, mut atomic, mut maximum) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut families = [0usize; 2];
    let mut modes = [0usize; 2];
    let mut parts = [0usize; 4];
    let mut endpoint_packets = [0usize; 2];
    let mut actor_flags = [[0usize; 256]; 24];
    let mut actor_selections = [[0usize; 256]; 24];
    let mut global_matrix = [[0usize; 256]; 16];
    let mut first = Value::Null;
    while r.cursor < r.words.len() {
        let [family, variation, part, alternate, raw, selector, flags] = r.array();
        if family > 1 || part >= 4 || alternate > 1 || raw > 255 || selector > 255 || flags > 255 {
            return Err("Invalid timbre-output input".into());
        }
        let actors: [TimbreOutputActor; 24] = core::array::from_fn(|slot| {
            let [binding, selection, flags, chip, origin] = r.array();
            actor_flags[slot][flags as usize] += 1;
            actor_selections[slot][selection as usize] += 1;
            TimbreOutputActor {
                timbre_binding: binding,
                primary_selection: selection as u8,
                flags: flags as u8,
                endpoint: if chip == 0 {
                    DspEndpoint::Master
                } else {
                    DspEndpoint::Slave
                },
                parameter_origin: origin as u16,
            }
        });
        let n = r.one();
        let expected_packets: Vec<_> = (0..n)
            .map(|_| {
                let chip = r.one();
                let n = r.one();
                (chip, (0..n).map(|_| r.one() as u16).collect::<Vec<_>>())
            })
            .collect();
        let n = r.one();
        let expected_changes: Vec<_> = (0..n).map(|_| r.array::<4>()).collect();
        let mut bytes = [0u8; 1790];
        bytes[0x3ed] = selector as u8;
        bytes[0x3c0] = flags as u8;
        let program = Program::from_bytes(&bytes).map_err(|_| "Invalid source Program")?;
        let raw_timbre = ((raw & 0xfc) | part) as u8;
        let before = port.native.memories().clone();
        port.reject = true;
        port.native.clear_packets();
        if restore_timbre_output(
            &mut port,
            &tables,
            &actors,
            bindings,
            &program,
            raw_timbre,
            alternate != 0,
        )
        .is_err()
            && *port.native.memories() == before
            && port.native.packets().is_empty()
        {
            atomic += 1;
        } else {
            errors += 1;
        }
        port.reject = false;
        restore_timbre_output(
            &mut port,
            &tables,
            &actors,
            bindings,
            &program,
            raw_timbre,
            alternate != 0,
        )
        .map_err(|_| "Native timbre-output request rejected")?;
        let mut actual_changes = Vec::new();
        for (chip, previous) in before.iter().enumerate() {
            for (base, count) in WINDOWS {
                for address in base..base + count {
                    let old = previous.words()[address];
                    let new = port.native.memories()[chip].words()[address];
                    if old != new {
                        actual_changes.push([
                            chip as u32,
                            address as u32,
                            u32::from(old),
                            u32::from(new),
                        ]);
                    }
                }
            }
        }
        let packets_actual: Vec<_> = port
            .native
            .packets()
            .iter()
            .map(|(endpoint, words)| (u32::from(*endpoint == DspEndpoint::Slave), words.clone()))
            .collect();
        if packets_actual != expected_packets || actual_changes != expected_changes {
            errors += 1;
            if first.is_null() {
                first = json!({"case":calls,"input":[family,variation,part,alternate,raw,selector,flags],"native_packets":packets_actual,"original_packets":expected_packets,"native_memory_changes":actual_changes,"original_memory_changes":expected_changes});
            }
        }
        maximum = maximum.max(port.native.packets().len());
        packets += port.native.packets().len();
        changes += actual_changes.len();
        for packet in port.native.packets() {
            endpoint_packets[usize::from(packet.0 == DspEndpoint::Slave)] += 1;
        }
        families[family as usize] += 1;
        modes[alternate as usize] += 1;
        parts[part as usize] += 1;
        global_matrix[(selector & 15) as usize][flags as usize] += 1;
        calls += 1;
    }
    let flags_covered = actor_flags.iter().all(|row| row.iter().all(|&n| n > 0));
    let selections_covered = actor_selections
        .iter()
        .all(|row| row.iter().all(|&n| n > 0));
    let global_covered = global_matrix.iter().all(|row| row.iter().all(|&n| n > 0));
    let passed = errors == 0
        && calls == 40960
        && families == [8192, 32768]
        && modes == [20480; 2]
        && parts == [10240; 4]
        && atomic == calls
        && flags_covered
        && selections_covered
        && global_covered
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_timbre_output_calls":calls,"whole_original_C55_receives":packets,"evolving_DSP_word_changes_compared":changes,"errors":errors,"first_difference":first,"atomic_DSP_memory_and_packet_rejections":atomic,"maximum_packets":maximum,"family_counts":families,"mode_counts":modes,"timbre_counts":parts,"endpoint_packet_counts":endpoint_packets,"all24_actors_all256_flag_and_primary_selection_bytes":flags_covered&&selections_covered,"all16_global_input_owners_all256_flag_masks":global_covered,"source_actor_bytes_flags_and_stored_program_guards_preserved":true,"independent_native_packet_order_and_evolving_two_chip_memory_compared":true,"native_memory_outputs_replayed_from_original":false,"original_SH_or_C55_instruction_bodies_modified":false,"whole_active_voice_busy_rack_wrapper_or_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/timbre-output-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native timbre output: {calls} calls, {packets} original receives, {changes} DSP changes, {errors} differences"
    );
    if !passed {
        return Err("Timbre output differs".into());
    }
    Ok(())
}
