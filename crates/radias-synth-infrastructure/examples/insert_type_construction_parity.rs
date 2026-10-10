//! Whole SYS07B318 construction, independent evolving eight-slot native state.
use radias_synth_application::insert_type_construction::{InsertTypeConstructionPort, InsertTypeConstructionRequest, construct_insert_type};
use radias_synth_domain::{
    effect_buffer_allocation::EffectBufferInstance,
    effect_buffers::EffectBufferSlice,
    effect_lfo_program::EffectLfoProgram,
    effect_modulation::GrainModulationHistory,
    insert_effect_construction::{InsertEffectInstance, InsertPatch},
    insert_type_construction::{InsertTypeConstruction, PreparedInsertTypeConstruction},
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
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        self.array::<N>().map(|v| v as u8)
    }
}
fn long(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}
fn short(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes(b[i..i + 2].try_into().unwrap())
}
fn put(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_be_bytes());
}
fn decode(raw: [u8; 172]) -> InsertEffectInstance {
    InsertEffectInstance {
        slot: raw[3],
        buffer: EffectBufferInstance {
            kind: raw[7],
            origin: short(&raw, 0x40),
            parameters: raw[0x0c..0x20].try_into().unwrap(),
            buffer_origin: long(&raw, 0x50),
            layout: EffectBufferSlice {
                offset: long(&raw, 0x4c),
                frames: long(&raw, 0x48),
            },
            cached_tempo: short(&raw, 0x44),
            ratio: long(&raw, 0x54),
            limited: long(&raw, 0x58),
            pending_coefficients: [long(&raw, 0x7c), long(&raw, 0x80)],
            pending_argument: long(&raw, 0x84),
        },
        previous_parameters: raw[0x20..0x34].try_into().unwrap(),
        owners: [long(&raw, 0x34), long(&raw, 0x38)],
        lfo: EffectLfoProgram {
            bytes: raw[0x5c..0x62].try_into().unwrap(),
        },
        controller_source: long(&raw, 0x68),
        controller_values: [raw[0x6c] as i8, raw[0x6d] as i8],
        controller_offset: raw[0x62],
        extended_program: long(&raw, 0x64),
        enabled_argument: long(&raw, 0x78),
        rotary_mode: long(&raw, 0x70),
        rotary_speed: long(&raw, 0x74),
        grain_history: GrainModulationHistory {
            left: core::array::from_fn(|i| short(&raw, 136 + 2 * i) as i16),
            right: core::array::from_fn(|i| short(&raw, 154 + 2 * i) as i16),
            left_read: raw[152],
            left_write: raw[153],
            right_read: raw[170],
            right_write: raw[171],
        },
    }
}
fn project(instance: &InsertEffectInstance, anchor: [u8; 172], system: &[u8]) -> [u8; 172] {
    let mut b = anchor;
    put(&mut b, 4, u32::from(instance.buffer.kind));
    put(
        &mut b,
        8,
        long(
            system,
            0x1000 + 0x0cceac + 4 * usize::from(instance.buffer.kind),
        ),
    );
    b[0x0c..0x20].copy_from_slice(&instance.buffer.parameters);
    b[0x20..0x34].copy_from_slice(&instance.previous_parameters);
    put(&mut b, 0x34, instance.owners[0]);
    put(&mut b, 0x38, instance.owners[1]);
    put(&mut b, 0x54, instance.buffer.ratio);
    put(&mut b, 0x58, instance.buffer.limited);
    b[0x62] = instance.controller_offset;
    put(&mut b, 0x64, instance.extended_program);
    put(&mut b, 0x70, instance.rotary_mode);
    put(&mut b, 0x74, instance.rotary_speed);
    put(&mut b, 0x78, instance.enabled_argument);
    for i in 0..8 {
        b[136 + 2 * i..138 + 2 * i].copy_from_slice(&instance.grain_history.left[i].to_be_bytes());
        b[154 + 2 * i..156 + 2 * i].copy_from_slice(&instance.grain_history.right[i].to_be_bytes());
    }
    b[152] = instance.grain_history.left_read;
    b[153] = instance.grain_history.left_write;
    b[170] = instance.grain_history.right_read;
    b[171] = instance.grain_history.right_write;
    b
}
#[derive(Default)]
struct Port {
    reject: bool,
    accepted: bool,
}
impl InsertTypeConstructionPort for Port {
    type Error = ();
    fn accept_insert_type_construction(&mut self, _: &PreparedInsertTypeConstruction) -> Result<(), ()> {
        if self.reject {
            return Err(());
        }
        self.accepted = true;
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = EffectLibrary::from_system(&system)?.insert_type_construction_tables()?;
    let decoded = std::process::Command::new("gzip").arg("-dc").arg(root.join("runs/native-clone/insert-type-construction-original.bin.gz")).output()?;
    if !decoded.status.success() {return Err("Cannot decode Insert type construction corpus".into());}
    let raw = decoded.stdout;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated insert construction corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x49544331 {
        return Err("Wrong insert construction corpus".into());
    }
    let (
        mut calls,
        mut errors,
        mut rejected,
        mut clamped,
        mut history_differences,
        mut grain_resets,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [[0usize; 31]; 8];
    let mut enabled_counts = [0usize; 2];
    let mut owners = [[0usize; 32]; 2];
    let mut first = Value::Null;
    let mut mode_counts = [0usize;2];
    let mut changed_patch_bytes = 0usize;
    for sequence in 0..2u32 {for mode in 0..2u32 {
        if r.array::<3>() != [0x1000, sequence,mode] {
            return Err("Insert construction sequence changed".into());
        }
        let anchors: [[u8; 172]; 8] = core::array::from_fn(|_| r.bytes());
        let mut instances = anchors.map(decode);
        for (i, instance) in instances.iter().enumerate() {
            if project(instance, anchors[i], &system) != anchors[i] {
                return Err("Declared insert initial state differs".into());
            }
        }
        let mut port = Port::default();
        for slot in 0..8usize {
            for position in 0..31u32 {
                for step in 0..256u32 {
                    let kind = ((position + sequence * 27) % 31) as u8;
                    if r.array::<7>()
                        != [
                            0x2000,
                            sequence,
                            slot as u32,
                            position,
                            u32::from(kind),
                            step,
                            mode,
                        ]
                    {
                        return Err("Insert constructor declared input changed".into());
                    }
                    let patch_bytes = r.bytes::<24>();
                    let mut patch = InsertPatch {
                        header: patch_bytes[..4].try_into().unwrap(),
                        parameters: patch_bytes[4..].try_into().unwrap(),
                    };
                    let before = r.bytes::<172>();
                    let after = r.bytes::<172>();
                    let expected_patch = r.bytes::<24>();
                    let saved = instances;
                    port.reject = true;
                    port.accepted = false;
                    if construct_insert_type(&mut instances[slot], &mut patch, &tables, &mut port, InsertTypeConstructionRequest {kind,mode: if mode==0 {InsertTypeConstruction::Defaults} else {InsertTypeConstruction::StoredWithDefaultHistory}})
                    .is_err()
                        && instances == saved
                        && patch.header==patch_bytes[..4] && patch.parameters==patch_bytes[4..]
                        && !port.accepted
                    {
                        rejected += 1;
                    } else {
                        errors += 1;
                    }
                    port.reject = false;
                    construct_insert_type(&mut instances[slot], &mut patch, &tables, &mut port, InsertTypeConstructionRequest {kind,mode: if mode==0 {InsertTypeConstruction::Defaults} else {InsertTypeConstruction::StoredWithDefaultHistory}})
                        .map_err(|_| "Native insert construction rejected")?;
                    let native = project(&instances[slot], anchors[slot], &system);
                    let mut native_patch=[0u8;24];native_patch[..4].copy_from_slice(&patch.header);native_patch[4..].copy_from_slice(&patch.parameters);
                    let prior_matches = project(&saved[slot], anchors[slot], &system) == before;
                    let neighbors_preserved = (0..8)
                        .filter(|&i| i != slot)
                        .all(|i| instances[i] == saved[i]);
                    if !prior_matches
                        || native != after
                        || native_patch != expected_patch
                        || !neighbors_preserved
                        || !port.accepted
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,slot as u32,position,u32::from(kind),step],"prior_matches":prior_matches,"native":native.to_vec(),"original":after.to_vec(),"neighbors_preserved":neighbors_preserved,"input_patch":patch_bytes.to_vec(),"original_patch":expected_patch.to_vec()});
                        }
                    }
                    let count = usize::from(tables.parameter_counts[usize::from(kind)]);
                    clamped += instances[slot].buffer.parameters[..count]
                        .iter()
                        .zip(patch_bytes[4..].iter().copied())
                        .filter(|(a, b)| **a != *b)
                        .count();
                    history_differences += instances[slot].buffer.parameters[..count]
                        .iter()
                        .zip(instances[slot].previous_parameters)
                        .filter(|(a, b)| **a != *b)
                        .count();
                    grain_resets +=
                        usize::from(instances[slot].grain_history != saved[slot].grain_history);
                    enabled_counts[usize::from(instances[slot].enabled_argument != 0)] += 1;
                    owners[0][instances[slot].owners[0] as usize] += 1;
                    owners[1][instances[slot].owners[1] as usize] += 1;
                    counts[slot][usize::from(kind)] += 1;
                    changed_patch_bytes+=native_patch.iter().zip(patch_bytes).filter(|(a,b)|**a!=*b).count();
                    mode_counts[mode as usize]+=1;
                    calls += 1;
                }
            }
        }
        if tables
            .prepare(
                &instances[0],
                InsertPatch {
                    header: [0; 4],
                    parameters: [0; 20],
                },
                31, InsertTypeConstruction::Defaults,
            )
            .is_some()
        {
            return Err("Unsupported insert type accepted".into());
        }
        let mut invalid = instances[0];
        invalid.slot = 8;
        if tables
            .prepare(
                &invalid,
                InsertPatch {
                    header: [0; 4],
                    parameters: [0; 20],
                },
                0, InsertTypeConstruction::Defaults,
            )
            .is_some()
        {
            return Err("Unsupported insert slot accepted".into());
        }
    }}
    let passed = errors == 0
        && calls == 253952
        && rejected == calls
        && counts == [[1024; 31]; 8]
        && r.cursor == r.words.len();
    let mut report = json!({"passed":passed,"whole_original_insert_type_constructor_calls":calls,"slot_type_counts":counts.map(|r|r.to_vec()).to_vec(),"selected_instance_and_history_bytes_compared":calls*172*2,"errors":errors,"first_difference":first,"whole_rack_state_atomic_rejections":rejected,"parameter_byte_changes_from_stored_patch":clamped,"current_previous_history_byte_differences":history_differences,"changed_Grain_history_initializations":grain_resets,"enabled_counts":enabled_counts,"owner_counts":owners.map(|r|r.to_vec()).to_vec(),"evolving_native_slot_or_history_outputs_replayed_from_original":false,"original_neighbor_and_Master_guards_preserved":true,"input_patch_mutations_compared":true,"full_rack_initialization_busy_rebuild_or_FXD03_audio_verified":false});
    report["mode_counts"] = json!(mode_counts);
    report["changed_patch_bytes_compared"]=json!(changed_patch_bytes);
    report["stored_parameters_clamped_with_default_history"]=json!(true);
    fs::write(
        root.join("runs/native-clone/insert-type-construction-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native stored insert construction: {calls} original calls, {errors} differences, {clamped} normalizations"
    );
    if !passed {
        return Err("Insert construction differs".into());
    }
    Ok(())
}
