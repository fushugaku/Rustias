//! Complete three-path Master construction; evolving native state never replayed.
use radias_synth_application::master_effect_construction::{
    MasterConstructionPort, construct_master_effect,
};
use radias_synth_domain::{
    delay_time::DelayTimeState,
    effect_lfo_program::EffectLfoProgram,
    effect_modulation::GrainModulationHistory,
    effect_updates::EffectCoefficientAssignments,
    filter_effect::FilterEffectCache,
    master_effect_construction::{
        MasterConstruction, MasterEffectInstance, MasterPatch, PreparedMasterConstruction,
    },
    master_effect_control::{MasterControlState, MasterMidiBinding},
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
fn project(instance: &MasterEffectInstance, anchor: [u8; 152], system: &[u8]) -> [u8; 152] {
    let mut b = anchor;
    put(&mut b, 0, u32::from(instance.kind));
    let offset = 0x0ccf28 + 0x1000 + usize::from(instance.kind) * 4;
    put(&mut b, 4, long(system, offset));
    b[8..28].copy_from_slice(&instance.parameters);
    b[28..48].copy_from_slice(&instance.previous_parameters);
    put(&mut b, 0x30, instance.control.owner);
    put(&mut b, 0x44, instance.control.delay.ratio);
    put(&mut b, 0x48, instance.control.delay.limited);
    b[0x52] = instance.controller_offset;
    put(&mut b, 0x5c, instance.control.rotary_mode);
    put(&mut b, 0x60, instance.control.rotary_speed);
    put(&mut b, 0x64, instance.enabled_argument);
    for i in 0..8 {
        b[116 + 2 * i..118 + 2 * i].copy_from_slice(&instance.grain_history.left[i].to_be_bytes());
        b[134 + 2 * i..136 + 2 * i].copy_from_slice(&instance.grain_history.right[i].to_be_bytes());
    }
    b[132] = instance.grain_history.left_read;
    b[133] = instance.grain_history.left_write;
    b[150] = instance.grain_history.right_read;
    b[151] = instance.grain_history.right_write;
    b
}
#[derive(Default)]
struct Port {
    reject: bool,
    accepted: bool,
}
impl MasterConstructionPort for Port {
    type Error = ();
    fn accept_master_construction(&mut self, _: &PreparedMasterConstruction) -> Result<(), ()> {
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
    let library = EffectLibrary::from_system(&system)?;
    let tables = library.master_control_tables()?;
    let raw = fs::read(root.join("runs/native-clone/master-effect-construction-original.bin"))?;
    if !raw.len().is_multiple_of(4) {
        return Err("Truncated Master construction corpus".into());
    }
    let mut r = Reader {
        words: raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect(),
        cursor: 0,
    };
    if r.one() != 0x4d434f31 {
        return Err("Wrong Master construction corpus".into());
    }
    let paths = [
        MasterConstruction::Defaults,
        MasterConstruction::StoredWithDefaultHistory,
        MasterConstruction::StoredWithRawHistory,
    ];
    let (
        mut calls,
        mut errors,
        mut rejected,
        mut normalizations,
        mut history_differences,
        mut grain_resets,
    ) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut counts = [[0usize; 31]; 3];
    let mut first = Value::Null;
    let mut enabled_counts = [0usize; 2];
    for sequence in 0..4u32 {
        if r.array::<2>() != [0x1000, sequence] {
            return Err("Master construction sequence changed".into());
        }
        let anchor = r.bytes::<152>();
        let mut instance = MasterEffectInstance {
            kind: anchor[3],
            parameters: anchor[8..28].try_into().unwrap(),
            previous_parameters: anchor[28..48].try_into().unwrap(),
            controller_offset: anchor[0x52],
            enabled_argument: long(&anchor, 0x64),
            grain_history: GrainModulationHistory {
                left: core::array::from_fn(|i| short(&anchor, 116 + 2 * i) as i16),
                right: core::array::from_fn(|i| short(&anchor, 134 + 2 * i) as i16),
                left_read: anchor[132],
                left_write: anchor[133],
                right_read: anchor[150],
                right_write: anchor[151],
            },
            control: MasterControlState {
                assignments: EffectCoefficientAssignments::new(
                    library.coefficient_update_indices()?,
                ),
                lfo: EffectLfoProgram {
                    bytes: anchor[0x4c..0x52].try_into().unwrap(),
                },
                delay: DelayTimeState {
                    cached_tempo: short(&anchor, 0x3c),
                    capacity: long(&anchor, 0x40),
                    ratio: long(&anchor, 0x44),
                    limited: long(&anchor, 0x48),
                },
                owner: long(&anchor, 0x30),
                pending: [long(&anchor, 0x68), long(&anchor, 0x6c)],
                pending_control: long(&anchor, 0x70),
                update_marker: 0,
                filter_cache: FilterEffectCache::default(),
                midi_binding: MasterMidiBinding {
                    source: long(&anchor, 0x54),
                    values: [anchor[0x58] as i8, anchor[0x59] as i8],
                },
                rotary_mode: long(&anchor, 0x5c),
                rotary_speed: long(&anchor, 0x60),
                work_slot: 0,
                coefficient_scratch: [0; 73],
            },
        };
        if project(&instance, anchor, &system) != anchor {
            return Err("Declared initial Master construction state differs".into());
        }
        let mut port = Port::default();
        for position in 0..31u32 {
            let kind = ((sequence * 9 + position) % 31) as u8;
            for (path_index, &path) in paths.iter().enumerate() {
                for step in 0..256u32 {
                    if r.array::<6>()
                        != [
                            0x2000,
                            sequence,
                            position,
                            u32::from(kind),
                            path_index as u32,
                            step,
                        ]
                    {
                        return Err("Master construction declared input changed".into());
                    }
                    let patch_bytes = r.bytes::<22>();
                    let mut patch = MasterPatch {
                        header: patch_bytes[..2].try_into().unwrap(),
                        parameters: patch_bytes[2..].try_into().unwrap(),
                    };
                    let before = r.bytes::<152>();
                    let after = r.bytes::<152>();
                    let expected_patch = r.bytes::<22>();
                    let saved = instance;
                    let saved_patch = patch;
                    port.reject = true;
                    port.accepted = false;
                    if construct_master_effect(
                        &mut instance,
                        &mut patch,
                        &mut port,
                        &tables,
                        kind,
                        path,
                    )
                    .is_err()
                        && instance == saved
                        && patch == saved_patch
                        && !port.accepted
                    {
                        rejected += 1;
                    } else {
                        errors += 1;
                    }
                    port.reject = false;
                    construct_master_effect(
                        &mut instance,
                        &mut patch,
                        &mut port,
                        &tables,
                        kind,
                        path,
                    )
                    .map_err(|_| "Native Master construction rejected")?;
                    let native = project(&instance, anchor, &system);
                    let mut native_patch = Vec::from(patch.header);
                    native_patch.extend(patch.parameters);
                    if project(&saved, anchor, &system) != before
                        || native != after
                        || native_patch != expected_patch
                        || !port.accepted
                    {
                        errors += 1;
                        if first.is_null() {
                            first = json!({"case":calls,"input":[sequence,position,u32::from(kind),path_index as u32,step],"native":native.to_vec(),"original":after.to_vec(),"native_patch":native_patch,"original_patch":expected_patch.to_vec(),"prior_matches":project(&saved,anchor,&system)==before});
                        }
                    }
                    let count = tables.definitions[usize::from(kind)].parameter_count;
                    if path != MasterConstruction::Defaults {
                        normalizations += instance.parameters[..count]
                            .iter()
                            .zip(saved_patch.parameters)
                            .filter(|(a, b)| **a != *b)
                            .count();
                    }
                    history_differences += instance.parameters[..count]
                        .iter()
                        .zip(instance.previous_parameters)
                        .filter(|(a, b)| **a != *b)
                        .count();
                    grain_resets +=
                        usize::from(kind == 27 && instance.grain_history != saved.grain_history);
                    enabled_counts[usize::from(instance.enabled_argument != 0)] += 1;
                    calls += 1;
                    counts[path_index][usize::from(kind)] += 1;
                }
            }
        }
        if tables
            .construct_master(
                &instance,
                MasterPatch {
                    header: [0; 2],
                    parameters: [0; 20],
                },
                31,
                MasterConstruction::Defaults,
            )
            .is_some()
        {
            return Err("Unsupported Master type accepted".into());
        }
    }
    let passed = errors == 0
        && calls == 95232
        && rejected == calls
        && counts == [[1024; 31]; 3]
        && r.cursor == r.words.len();
    let report = json!({"passed":passed,"whole_original_master_constructor_calls":calls,"path_type_counts":counts.map(|r|r.to_vec()).to_vec(),"all_instance_and_Grain_history_bytes_compared":calls*152*2,"errors":errors,"first_difference":first,"state_and_patch_atomic_rejections":rejected,"normalized_parameter_bytes":normalizations,"current_previous_history_byte_differences":history_differences,"changed_Grain_history_initializations":grain_resets,"enabled_counts":enabled_counts,"evolving_native_instance_or_history_outputs_replayed_from_original":false,"original_unrelated_instance_bytes_preserved":true,"Master_full_initialization_program_coefficient_routing_FXD03_audio_verified":false});
    fs::write(
        root.join("runs/native-clone/master-effect-construction-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!(
        "Native Master constructors: {calls} original calls, {errors} differences, {normalizations} normalizations"
    );
    if !passed {
        return Err("Master construction differs".into());
    }
    Ok(())
}
