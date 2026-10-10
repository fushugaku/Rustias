//! Independent evolving capacity and all eighty original callback output words.
use radias_synth_application::master_effect_buffers::{
    MasterBufferTemplatePort, relocate_master_effect_buffer_template,
};
use radias_synth_domain::master_effect_buffers::{
    MasterBufferState, PreparedMasterBufferTemplate, relocate_master_template,
};
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
}
#[derive(Default)]
struct Port {
    reject: bool,
    accepted: Option<PreparedMasterBufferTemplate>,
}
impl MasterBufferTemplatePort for Port {
    type Error = ();
    fn accept_master_template(&mut self, p: &PreparedMasterBufferTemplate) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.accepted = Some(*p);
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/master-effect-buffers-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master buffer corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d424631 {
        return Err("Wrong Master buffer corpus".into());
    }
    let (mut calls, mut errors, mut rejected, mut changed_capacity, mut changed_words) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [0usize; 31];
    let mut first = Value::Null;
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master buffer sequence changed".into());
        }
        let mut state = MasterBufferState { capacity: r.one() };
        let mut port = Port::default();
        for kind in 0..31u8 {
            for step in 0..128u32 {
                if r.array::<4>() != [0x2000, sequence, u32::from(kind), step] {
                    return Err("Master buffer input changed".into());
                }
                let input = r.array::<80>();
                let before = r.one();
                let after = r.one();
                let original = r.array::<80>();
                let saved = state;
                port.reject = true;
                port.accepted = None;
                if relocate_master_effect_buffer_template(&mut state, &mut port, kind, &input)
                    .is_err()
                    && state == saved
                    && port.accepted.is_none()
                {
                    rejected += 1;
                } else {
                    errors += 1;
                }
                port.reject = false;
                relocate_master_effect_buffer_template(&mut state, &mut port, kind, &input)
                    .map_err(|_| "Native Master buffer rejected")?;
                let native = port.accepted.take().ok_or("Missing Master template")?;
                if saved.capacity != before
                    || state.capacity != after
                    || native.words != original
                    || native.count != 80
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"input":[sequence,u32::from(kind),step],"native_prior_capacity":saved.capacity,"original_prior_capacity":before,"native_capacity":state.capacity,"original_capacity":after,"native_words":native.words.to_vec(),"original_words":original.to_vec()});
                    }
                }
                changed_capacity += usize::from(state != saved);
                changed_words += native
                    .words
                    .iter()
                    .zip(input)
                    .filter(|(a, b)| **a != *b)
                    .count();
                counts[usize::from(kind)] += 1;
                calls += 1;
            }
        }
    }
    let invalid_kind_rejected =
        relocate_master_template(31, MasterBufferState::default(), &[0; 80]).is_none();
    let mut short_rejections = 0;
    for kind in 0..31u8 {
        for count in 0..=81 {
            if relocate_master_template(kind, MasterBufferState::default(), &vec![0; count])
                .is_none()
            {
                short_rejections += 1;
            }
        }
    }
    let passed = errors == 0
        && calls == 15872
        && rejected == calls
        && counts == [512; 31]
        && r.cursor == r.words.len()
        && invalid_kind_rejected;
    let report = json!({"passed":passed,"whole_original_master_template_callbacks":calls,"type_counts":counts.to_vec(),"words_compared":calls*80,"errors":errors,"first_difference":first,"full_queue_atomic_rejections":rejected,"capacity_changes":changed_capacity,"changed_template_words":changed_words,"unsupported_type_rejected":invalid_kind_rejected,"invalid_template_lengths_rejected":short_rejections,"evolving_native_capacity_replayed_from_original":false,"original_unrelated_instance_and_template_guards_preserved":true,"Master_initialization_FXD03_audio_or_physical_memory_timing_verified":false});
    fs::write(
        root.join("runs/native-clone/master-effect-buffers-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("Native Master buffer callbacks: {calls} whole original calls, {errors} differences");
    if !passed {
        return Err("Master buffer callbacks differ".into());
    }
    Ok(())
}
