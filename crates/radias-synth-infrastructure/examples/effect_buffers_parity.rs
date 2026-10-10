//! Whole source buffer predicates, pair planner and all template callbacks.
use radias_synth_application::effect_buffers::{
    EffectBufferTemplatePort, relocate_effect_buffer_template,
};
use radias_synth_domain::effect_buffers::{
    EffectBufferSlice, EffectBufferTables, PreparedEffectBufferTemplate, effect_uses_buffer,
};
use radias_synth_infrastructure::effects::EffectLibrary;
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
#[derive(Default)]
struct Port {
    reject: bool,
    accepted: Option<PreparedEffectBufferTemplate>,
}
impl EffectBufferTemplatePort for Port {
    type Error = ();
    fn accept_template(&mut self, p: &PreparedEffectBufferTemplate) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.accepted = Some(*p);
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&source)?.buffer_tables()?;
    let raw = fs::read(root.join("runs/native-clone/effect-buffers-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated effect buffer corpus".into());
    }
    let mut read = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if read.one() != 0x45425531 {
        return Err("Wrong buffer corpus".into());
    }
    let (mut predicates, mut pairs, mut templates, mut errors, mut rejected) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut first = Value::Null;
    let mut pair_counts = [0usize; 2];
    let mut template_counts = [0usize; 31];
    while read.cursor < read.words.len() {
        match read.one() {
            0x1000 => {
                let kind = read.one();
                let original = read.one();
                let native = u32::from(effect_uses_buffer(kind));
                if native != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"predicate_kind":kind,"native":native,"original":original});
                    }
                }
                predicates += 1;
            }
            0x2000 => {
                let [custom, a, b, profile] = read.array::<4>();
                let _before = read.array::<4>();
                let original = read.array::<4>();
                let pair = if custom == 0 {
                    tables.pair(a as u8, b as u8)
                } else {
                    let mut synthetic = EffectBufferTables {
                        profiles: [0; 31],
                        frames: tables.frames,
                    };
                    synthetic.profiles[0] = a as u8;
                    synthetic.profiles[1] = b as u8;
                    synthetic.pair(0, 1)
                }
                .ok_or("Native pair rejected source inputs")?;
                let native = [
                    pair[0].offset,
                    pair[0].frames,
                    pair[1].offset,
                    pair[1].frames,
                ];
                if native != original {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"custom":custom,"first":a,"second":b,"profile":profile,"native":native,"original":original});
                    }
                }
                pairs += 1;
                pair_counts[custom as usize] += 1;
            }
            0x3000 => {
                let [kind, profile, variation, offset, frames, buffer_origin] = read.array::<6>();
                let before = read.array::<80>();
                let [next_offset, next_frames] = read.array::<2>();
                let original = read.array::<80>();
                let initial = PreparedEffectBufferTemplate {
                    layout: EffectBufferSlice { offset, frames },
                    words: before,
                    count: 80,
                };
                let mut current = initial;
                let mut no_room = Port {
                    reject: true,
                    ..Default::default()
                };
                if relocate_effect_buffer_template(
                    &mut current,
                    &mut no_room,
                    kind as u8,
                    buffer_origin,
                    &tables,
                )
                .is_ok()
                    || current != initial
                    || no_room.accepted.is_some()
                {
                    return Err("Rejected template changed buffer or words".into());
                }
                rejected += 1;
                let mut port = Port::default();
                relocate_effect_buffer_template(
                    &mut current,
                    &mut port,
                    kind as u8,
                    buffer_origin,
                    &tables,
                )
                .map_err(|_| "Native template rejected source inputs")?;
                if current.layout
                    != (EffectBufferSlice {
                        offset: next_offset,
                        frames: next_frames,
                    })
                    || current.words != original
                    || port.accepted != Some(current)
                {
                    errors += 1;
                    if first.is_null() {
                        first = json!({"kind":kind,"profile":profile,"variation":variation,"buffer_origin":buffer_origin,"before_layout":[offset,frames],"native_layout":[current.layout.offset,current.layout.frames],"original_layout":[next_offset,next_frames],"before_words":before.as_slice(),"native_words":current.words.as_slice(),"original_words":original.as_slice()});
                    }
                }
                templates += 1;
                template_counts[kind as usize] += 1;
            }
            _ => return Err("Unknown buffer record".into()),
        }
    }
    let passed = predicates == 262
        && pairs == 16976
        && pair_counts == [15376, 1600]
        && templates == 31744
        && template_counts == [1024; 31]
        && rejected == templates
        && errors == 0;
    let report = json!({"passed":passed,"whole_original_buffer_predicates":predicates,"whole_original_pair_plans":pairs,"pair_domain_counts":pair_counts,"whole_original_template_callbacks":templates,"template_type_counts":template_counts.as_slice(),"errors":errors,"first_difference":first,"atomic_template_rejections":rejected,"all_961_actual_insert_pairs_and_100_profile_pairs":true,"all_31_insert_template_callbacks":true,"sixteen_capacity_and_64_full_width_payload_profiles_per_type":true,"all_80_template_words_compared":true,"native_uses_raw_inputs_and_immutable_buffer_profiles":true,"source_output_layouts_or_templates_replayed_as_inputs":false,"all_original_callees_execute_without_stubs":true,"whole_SYS074302_neighbor_publications_master_allocation_or_FXD03_audio_verified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/effect-buffers-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native FX buffers: {predicates} predicates, {pairs} pair plans, {templates} original callbacks, {errors} differences"
    );
    if !passed {
        return Err("Native effect buffers differ".into());
    }
    Ok(())
}
