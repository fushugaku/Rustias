//! Original live controller queries; accepted pre-query claims and age clock.
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VoiceClaim, VoiceOrder},
};
use std::{fs, path::PathBuf};

fn number(value: &serde_json::Value) -> Result<u32, &'static str> {
    value
        .as_u64()
        .map(|n| n as u32)
        .ok_or("Missing group field")
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().ok_or("Repository required")?);
    let name = args.get(1).ok_or("Original scene required")?;
    let group_count = args
        .get(2)
        .map(|n| n.parse::<u32>())
        .transpose()?
        .unwrap_or(1);
    let out = root.join("runs/native-clone");
    let mut expected = None;
    let mut observed = 0u32;
    let mut queries = 0;
    let mut selected = Vec::new();
    for line in fs::read_to_string(out.join(format!("{name}-note-group-events.jsonl")))?.lines() {
        let event: serde_json::Value = serde_json::from_str(line)?;
        match event["kind"].as_str() {
            Some("release_query") => {
                if let Some(mask) = expected {
                    if mask != observed {
                        return Err("Live original group release differs".into());
                    }
                    selected.push(mask);
                }
                let mut groups = NoteGroups {
                    counter: number(&event["counter"])? as u16,
                    ..Default::default()
                };
                let mut claims = [VoiceClaim::default(); 24];
                let mut order = VoiceOrder::default();
                for slot in 0..24 {
                    let c = &event["claims"][slot];
                    claims[slot] = VoiceClaim {
                        owner: AllocationOwner(number(&c[0])?),
                        note_flags: number(&c[1])? as u8,
                        release_flags: number(&c[2])? as u8,
                    };
                    groups.ages[slot] = number(&c[3])? as u16;
                    groups.tags[slot] = number(&c[4])? as u8;
                    order.0[slot] = number(&event["order"][slot])? as u8;
                }
                expected = Some(groups.release_mask(
                    &order,
                    &claims,
                    AllocationOwner(number(&event["owner"])?),
                    number(&event["event"])?,
                ));
                observed = 0;
                queries += 1;
            }
            Some("release_slot") => {
                let slot = number(&event["slot"])?;
                if slot >= 24 || expected.is_none() {
                    return Err("Original group release has no query".into());
                }
                observed |= 1 << slot;
            }
            _ => return Err("Unknown original group observation".into()),
        }
    }
    if expected != Some(observed) {
        return Err("Final original group release differs".into());
    }
    selected.push(observed);
    if queries != 2
        || selected.iter().any(|m| m.count_ones() != group_count)
        || selected[0] == selected[1]
    {
        return Err("Repeated-note source scene did not release separate actors".into());
    }
    let report = serde_json::json!({"passed":true,"queries":queries,"selected_masks":selected,
        "native_release_selector_used":true,"original_selected_masks_replayed":false,
        "original_pre_query_claims_ages_tags_order_accepted":true,"independent_allocation_clock_qualified":false});
    fs::write(
        out.join(format!("{name}-group-scene-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{name}: two live original repeated-note releases match native groups");
    Ok(())
}
