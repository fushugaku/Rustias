//! Complete original descriptor procedures and their full chained publication.
use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::actor_descriptors::{ActorControlCache, DescriptorPlan};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master)?;
    let slave = MasterTables::from_host_stream(&slave)?;
    let tables = firmware::parameter_template_tables(&sys, master.filter_mix()?)?;
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    // Reject missing coefficient ports and full FIFO before any publication.
    let mut unsupported_body = [0u8; 104];
    unsupported_body[22] = 32;
    let full_plan = DescriptorPlan::compile(12, &unsupported_body, Default::default(), &tables)
        .map_err(|e| format!("{e:?}"))?;
    let mut unconfigured = SynthesisParameterTransport::default();
    if unconfigured.publish_actor_descriptors(0, 0, &full_plan)
        != Err(radias_synth_application::dsp_transport::SendQueueError::MissingFilterContext)
        || unconfigured.pending() != 0
        || unconfigured.parameter_state(0) != [0; 160]
    {
        return Err("Missing mix port published a partial descriptor chain".into());
    }
    unconfigured.configure_constructor_filter_mix(master.filter_mix()?);
    if unconfigured.publish_actor_descriptors(0, 0, &full_plan)
        != Err(radias_synth_application::dsp_transport::SendQueueError::MissingPitchContext)
        || unconfigured.pending() != 0
    {
        return Err("Missing pitch port published a partial descriptor chain".into());
    }
    unsupported_body[22] = 20;
    let null_plan = DescriptorPlan::compile(11, &unsupported_body, Default::default(), &tables)
        .map_err(|e| format!("{e:?}"))?;
    for _ in 0..512 {
        unconfigured
            .publish_actor_descriptors(0, 0, &null_plan)
            .map_err(|e| format!("{e:?}"))?;
    }
    if unconfigured.publish_actor_descriptors(0, 0, &full_plan)
        != Err(radias_synth_application::dsp_transport::SendQueueError::Full)
        || unconfigured.pending() != 512
        || unconfigured.parameter_state(0) != [0; 160]
    {
        return Err("Full FIFO published a partial descriptor chain".into());
    }
    let raw = fs::read(out.join("actor-descriptors-original.bin"))?;
    let (mut cursor, mut calls, mut commands, mut errors, mut words, mut chains) =
        (0, 0, 0, 0, 0, 0);
    let mut first_error = None;
    let mut coverage = [0; 13];
    while cursor < raw.len() {
        let family = take(&raw, &mut cursor) as u8;
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let busy = u64::from(take(&raw, &mut cursor));
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let cache_words: [u32; 14] = core::array::from_fn(|_| take(&raw, &mut cursor));
        let cache = ActorControlCache {
            waveform: cache_words[0] as i16,
            cross: cache_words[1] as i16,
            colored_color: cache_words[2] as i16,
            formant_level: cache_words[3] as i16,
            formant_feedback: cache_words[4] as i16,
            vpm: cache_words[5] as i16,
            unison: cache_words[6] as i16,
            pitch: cache_words[7] as i16,
            control2_modulation: cache_words[8] as i16,
            control2_manual: cache_words[9] as i8,
            shaper_manual: cache_words[10] as i16,
            shaper_modulation: cache_words[11] as i16,
            filter_type_manual: cache_words[12] as i16,
            filter_type_modulation: cache_words[13] as i16,
        };
        let before: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let returned = u64::from(take(&raw, &mut cursor));
        let count = take(&raw, &mut cursor) as usize;
        let mut packets = Vec::new();
        for _ in 0..count {
            let sender = take(&raw, &mut cursor);
            let entry = take(&raw, &mut cursor);
            let ack = u64::from(take(&raw, &mut cursor));
            let length = take(&raw, &mut cursor);
            let payload: Vec<u16> = (0..length)
                .map(|_| take(&raw, &mut cursor) as u16)
                .collect();
            let after: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
            packets.push((sender, entry, ack, payload, after));
        }
        let plan =
            DescriptorPlan::compile(family, &body, cache, &tables).map_err(|e| format!("{e:?}"))?;
        for chunk in [1u64, 31, 3000] {
            let slot = local + 12 * chip;
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(master.filter_mix()?);
            transport.configure_pitch_receivers(rom.clone(), dispatch);
            transport.restore_parameters(slot, before);
            transport
                .publish_actor_descriptors(0, slot, &plan)
                .map_err(|e| format!("{e:?}"))?;
            if transport.parameter_state(slot) != before {
                return Err("Enqueue changed unpublished actor memory".into());
            }
            let (mut clock, mut received, mut ready_at, mut good) = (0, 0, busy, true);
            for (_, _, ack, payload, after) in &packets {
                let mut seen = None;
                while clock < *ack {
                    clock = (clock + chunk).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|if poll<ready_at{1<<chip}else{0},|time,owner,event|{
                        received+=1;
                        seen=Some((time,owner,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..} if opcode==payload[1])));
                    });
                }
                good &= seen == Some((*ack, slot, true));
                let actual = transport.parameter_state(slot);
                if actual != *after {
                    good = false;
                    if first_error.is_none() {
                        let offset = actual.iter().zip(after).position(|(a, b)| a != b).unwrap();
                        first_error = Some(
                            serde_json::json!({"call":calls,"family":family,"chunk":chunk,"chip":chip,"slot":slot,"packet":received,"payload":payload,"offset":offset,"source":after[offset],"native":actual[offset],"body":body.to_vec(),"cache":cache_words,"plan":format!("{plan:?}")}),
                        );
                    }
                }
                ready_at = *ack + busy;
                words += 160;
            }
            transport.advance_until_with_readiness(returned, |_| 0, |_, _, _| good = false);
            good &= received == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if count == 0 {
                good &= transport.parameter_state(slot) == before;
                words += 160;
            }
            if !good {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"call":calls,"family":family,"chunk":chunk,"busy":busy,"received":received,"expected":count,"source_return":returned,"native_return":transport.caller_available_clock(),"plan":format!("{plan:?}")}));
            }
        }
        calls += 1;
        commands += count;
        chains += usize::from(family == 12);
        coverage[usize::from(family)] += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_descriptor_calls":calls,"whole_original_SYS01e9e4_chains":chains,
        "whole_original_E319_descriptor_receives":commands,"parameter_words_compared":words,"errors":errors,"first_error":first_error,
        "procedure_coverage":coverage,"native_direct_descriptor_and_depth_noise_VPM_compilation_used":true,
        "common_FIFO_and_complete_actor_banks_used":true,"prior_controller_shadows_and_actor_banks_declared_inputs":true,
        "missing_ROM_mix_and_full_FIFO_preserve_prior_state":true,
        "source_readiness_policies":[0,64,333,3000],"clock_partitions":[1,31,3000],
        "full_dynamic_note_constructor_and_DMA_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-descriptor-parity.json"),
        format!("{report:#}\n"),
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Original actor descriptor publication differs".into());
    }
    Ok(())
}
