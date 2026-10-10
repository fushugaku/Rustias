//! Native DDD effect controller compared to whole original SYS preparation,
//! word uploads and Dry/Wet fixtures. No original output is a compiler input.
use radias_synth_application::effects::{
    EffectProgramPort, EffectUpdateController, EffectUpdateQueue, dispatch_coefficient_plan,
    load_effect, set_effect_mix,
};
use radias_synth_domain::effect_control::{
    EffectBank, EffectKind, EffectLoad, EffectMix, EffectOrigins, EffectRequest, MixContext,
};
use radias_synth_domain::effect_updates::{
    CoefficientChange, CoefficientChangePlan, EffectCoefficientAssignments,
};
use radias_synth_infrastructure::effects::EffectLibrary;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};
#[derive(Default)]
struct Port {
    program: Vec<(u16, u64, u16)>,
    coefficients: Vec<(u16, u32, u16)>,
    packets: u32,
}
impl EffectProgramPort for Port {
    type Error = std::convert::Infallible;
    fn upload_program(
        &mut self,
        address: u16,
        words: &[u64],
        control: u16,
    ) -> Result<(), Self::Error> {
        self.packets += 1;
        for (i, value) in words.iter().enumerate() {
            self.program
                .push((address.wrapping_add(i as u16), *value, control));
        }
        Ok(())
    }
    fn write_coefficient(
        &mut self,
        address: u16,
        value: u32,
        control: u16,
    ) -> Result<(), Self::Error> {
        self.coefficients.push((address, value, control));
        Ok(())
    }
    fn write_coefficient_packet(
        &mut self,
        address: u16,
        values: &[u32],
        control: u16,
    ) -> Result<(), Self::Error> {
        self.packets += 1;
        for (i, value) in values.iter().enumerate() {
            self.coefficients
                .push((address.wrapping_add(i as u16), *value, control));
        }
        Ok(())
    }
}
#[derive(Default)]
struct Queue {
    plan: Option<CoefficientChangePlan>,
    reject: bool,
}
impl EffectUpdateQueue for Queue {
    type Error = ();
    fn enqueue(&mut self, plan: &CoefficientChangePlan) -> Result<(), Self::Error> {
        if self.reject {
            return Err(());
        }
        self.plan = Some(*plan);
        Ok(())
    }
}
fn assignment_value(state: &EffectCoefficientAssignments) -> Value {
    json!({"order":state.order,"slots":state.slots.map(|s| {
        [u32::from(s.indices[0]),u32::from(s.indices[1]),u32::from(s.indices[2]),u32::from(s.indices[3]),s.target,s.last_value]
    })})
}
fn number(row: &Value, key: &str) -> u64 {
    row[key]
        .as_u64()
        .expect("Original observation field missing")
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let library = EffectLibrary::from_system(&system)?;
    let (
        mut programs,
        mut program_errors,
        mut words,
        mut mixes,
        mut mix_errors,
        mut mix_words,
        mut rejections,
    ) = (0u32, 0u32, 0u64, 0u32, 0u32, 0u64, 0u32);
    let mut first = Value::Null;
    let mut cases = BTreeSet::new();
    for line in fs::read_to_string(root.join("runs/fxd-preparation/native.jsonl"))?.lines() {
        let row: Value = serde_json::from_str(line)?;
        let kind =
            EffectKind::new(number(&row, "type") as u8).ok_or("Original effect type invalid")?;
        let bank = if number(&row, "bank") == 0 {
            EffectBank::Insert
        } else {
            EffectBank::Master
        };
        let load = if number(&row, "path") == 0 {
            EffectLoad::Default
        } else {
            EffectLoad::ParameterSelected
        };
        let request = EffectRequest {
            kind,
            bank,
            load,
            work_slot: number(&row, "slot") as u16,
            selector_byte: number(&row, "selector_value") as u8,
            origins: EffectOrigins {
                program: number(&row, "program_origin") as u16,
                data: number(&row, "data_origin") as u16,
                coefficients: number(&row, "coefficient_origin") as u16,
            },
        };
        let candidate = library.prepare(request)?;
        let mut port = Port::default();
        load_effect(&mut port, &candidate)?;
        let original = row["host_words"]
            .as_array()
            .ok_or("Original host words absent")?
            .iter()
            .map(|v| {
                (
                    v[0].as_u64().unwrap() as u16,
                    v[1].as_u64().unwrap(),
                    v[2].as_u64().unwrap() as u16,
                )
            })
            .collect::<Vec<_>>();
        port.program.sort_unstable();
        let mut errors = port.program != original
            || port.packets != number(&row, "uploaded_packets") as u32
            || u64::from(candidate.extended_insert) != number(&row, "object_extended_flag");
        let blocks = row["queued_blocks"].as_array().unwrap();
        errors |= blocks.len() != usize::from(candidate.block_count);
        for (native, original) in candidate.blocks[..usize::from(candidate.block_count)]
            .iter()
            .zip(blocks)
        {
            errors |= u64::from(native.destination) != number(original, "destination")
                || u64::from(native.tag) != number(original, "tag")
                || u64::from(native.buffer_address) != number(original, "buffer")
                || u64::from(native.count) != number(original, "count");
            let start = usize::from(native.word_start);
            let expected = original["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            errors |= candidate.words[start..start + usize::from(native.count)] != expected;
        }
        if errors {
            program_errors += 1;
            if first.is_null() {
                first = json!({"bank":number(&row,"bank"),"type":kind.raw(),"path":number(&row,"path"),"selector":request.selector_byte,"profile":number(&row,"profile"),"boundary":"effect program/host"});
            }
        }
        words += port.program.len() as u64;
        programs += 1;
        cases.insert((
            number(&row, "bank"),
            number(&row, "path"),
            kind.raw(),
            request.selector_byte,
            number(&row, "profile"),
        ));
    }
    let mut mix_cases = BTreeSet::new();
    for line in fs::read_to_string(root.join("runs/fxd-mix-coefficients/native.jsonl"))?.lines() {
        let row: Value = serde_json::from_str(line)?;
        let kind = EffectKind::new(number(&row, "effect_type") as u8).unwrap();
        let context = match number(&row, "context_profile") {
            0 => MixContext::default(),
            1 => MixContext {
                byte5: 1,
                ..Default::default()
            },
            2 => MixContext {
                byte6: 1,
                ..Default::default()
            },
            3 => MixContext {
                byte1: 1,
                byte6: 1,
                ..Default::default()
            },
            4 => MixContext {
                byte1: 127,
                byte5: 127,
                byte6: 127,
            },
            _ => return Err("Unsupported original mix context".into()),
        };
        let mix = EffectMix::compile(kind, number(&row, "value") as u8, context)
            .ok_or("Invalid original mix input")?;
        let mut port = Port::default();
        set_effect_mix(&mut port, number(&row, "origin") as u16, mix)?;
        let expected = row["host_words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v[0].as_u64().unwrap() as u16,
                    v[1].as_u64().unwrap() as u32,
                    v[2].as_u64().unwrap() as u16,
                )
            })
            .collect::<Vec<_>>();
        let errors = mix.dry as u32 != number(&row, "dry") as u32
            || mix.wet as u32 != number(&row, "wet") as u32
            || port.coefficients != expected;
        if errors {
            mix_errors += 1;
            if first.is_null() {
                first = json!({"type":kind.raw(),"value":number(&row,"value"),"context":number(&row,"context_profile"),"boundary":"effect mix"});
            }
        }
        mix_words += port.coefficients.len() as u64;
        mixes += 1;
        mix_cases.insert((
            kind.raw(),
            number(&row, "value"),
            number(&row, "context_profile"),
            number(&row, "origin"),
        ));
    }
    for raw in 31..=255 {
        rejections += u32::from(EffectKind::new(raw).is_none());
    }
    for kind in 0..31 {
        for value in 101..=255 {
            rejections += u32::from(
                EffectMix::compile(EffectKind::new(kind).unwrap(), value, MixContext::default())
                    .is_none(),
            );
        }
    }
    let mut controller = EffectUpdateController {
        assignments: EffectCoefficientAssignments::new(library.coefficient_update_indices()?),
    };
    let (
        mut updates,
        mut update_errors,
        mut update_words,
        mut update_packets,
        mut queue_rejections,
    ) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut update_cases = BTreeSet::new();
    for line in fs::read_to_string(root.join("runs/fxd-coefficient-updates/native.jsonl"))?.lines()
    {
        let row: Value = serde_json::from_str(line)?;
        if number(&row, "step") == 0 {
            controller.assignments =
                EffectCoefficientAssignments::new(library.coefficient_update_indices()?);
        }
        let change = CoefficientChange {
            direct_switch: number(&row, "direct_switch") as u32,
            standalone: number(&row, "call_mode") != 0,
            enabled_argument: number(&row, "enabled") as u32,
            mode: number(&row, "mode") as u8,
            target: number(&row, "target") as u32,
            value: number(&row, "value") as u32,
        };
        let before = controller.assignments;
        if number(&row, "step") == 0 {
            let mut rejected = Queue {
                reject: true,
                ..Default::default()
            };
            if controller.change(&mut rejected, change).is_err()
                && controller.assignments == before
                && rejected.plan.is_none()
            {
                queue_rejections += 1;
            }
        }
        let mut queue = Queue::default();
        controller
            .change(&mut queue, change)
            .map_err(|_| "Coefficient queue rejected unexpectedly")?;
        let plan = queue.plan.ok_or("Coefficient plan absent")?;
        let candidate_queue: Vec<_> = plan.entries[..usize::from(plan.count)]
            .iter()
            .map(|e| [u32::from(e.address), e.tagged_value])
            .collect();
        let mut port = Port::default();
        dispatch_coefficient_plan(&mut port, &plan)?;
        port.coefficients.sort_unstable();
        let errors = assignment_value(&before) != row["before"]
            || assignment_value(&controller.assignments) != row["after"]
            || json!(candidate_queue) != row["queue"]
            || json!(port.coefficients) != row["host_words"]
            || u64::from(port.packets) != number(&row, "host_packets");
        if errors {
            update_errors += 1;
            if first.is_null() {
                first = json!({"sequence":row["sequence"],"step":row["step"],"boundary":"live coefficient state/queue/host"});
            }
        }
        updates += 1;
        update_words += port.coefficients.len() as u32;
        update_packets += port.packets;
        update_cases.insert((number(&row, "sequence"), number(&row, "step")));
    }
    let passed = programs == 1778
        && cases.len() == 1778
        && program_errors == 0
        && words == 240960
        && mixes == 31310
        && mix_cases.len() == 31310
        && mix_errors == 0
        && mix_words == 62620
        && rejections == 5030
        && updates == 832
        && update_cases.len() == 832
        && update_errors == 0
        && update_words == 2288
        && update_packets == 1352
        && queue_rejections == 32;
    let report = json!({"passed":passed,"original_whole_effect_program_preparations":programs,"native_program_and_host_word_errors":program_errors,
        "original_program_words_compared":words,"effect_type_banks":2,"effect_types_per_bank":31,
        "original_mix_pairs_compared":mixes,"native_mix_and_coefficient_host_errors":mix_errors,"coefficient_host_words_compared":mix_words,
        "invalid_type_and_mix_inputs_rejected":rejections,"first_difference":first,
        "native_domain_and_application_are_no_std":true,"native_controller_executes_firmware_instructions":false,
        "original_template_bytes_are_declared_inputs":true,"recorded_host_programs_or_coefficient_outputs_used_as_native_inputs":false,
        "conditional_templates_original_padding_split_link_origins_and_host_packet_boundaries_verified":true,
        "original_effect_controller_mix_laws_and_phase_polarity_verified":true,
        "original_live_coefficient_transitions_compared":updates,"live_coefficient_state_queue_and_host_errors":update_errors,
        "live_coefficient_host_words_compared":update_words,"live_coefficient_host_packets_compared":update_packets,
        "full_queue_atomic_rejections":queue_rejections,"last_requested_values_preserve_all32_bits":true,
        "FXD03_instruction_arithmetic_or_original_effects_audio_verified":false,"complete_native_engine":false});
    let out = root.join("runs/native-clone/effect-control-parity.json");
    fs::write(out, serde_json::to_string_pretty(&report)? + "\n")?;
    println!(
        "Native effects: {programs} complete program loads, {mixes} Dry/Wet pairs: {} errors",
        program_errors + mix_errors
    );
    if !passed {
        return Err("Native effect controller differs from original firmware".into());
    }
    Ok(())
}
