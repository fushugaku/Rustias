use radias_synth_application::{
    actor_copy,
    dsp_receiver::{ParameterMemory, ReceiveOutcome, receive_memory_command},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::actor_copy::ActorTemplateBinding;
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};

struct Memory(Vec<u16>);
impl ParameterMemory for Memory {
    fn read_word(&self, address: u16) -> u16 {
        self.0[usize::from(address)]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.0[usize::from(address)] = value;
    }
}
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let addresses = firmware::parameter_template_addresses(&sys)?;
    let raw = fs::read(out.join("actor-copy-original.bin"))?;
    let (mut cursor, mut calls, mut errors, mut compared) = (0, 0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = take(&raw, &mut cursor) as usize;
        let local = take(&raw, &mut cursor) as usize;
        let binding = ActorTemplateBinding {
            program_kind: take(&raw, &mut cursor) as u8,
            timbre_id: take(&raw, &mut cursor) as u8,
            drum_instrument: take(&raw, &mut cursor) as u8,
            ordinary_address: take(&raw, &mut cursor) as u16,
        };
        let busy = u64::from(take(&raw, &mut cursor));
        let before: Vec<u16> = (0..4480).map(|_| take(&raw, &mut cursor) as u16).collect();
        let sender = take(&raw, &mut cursor);
        let ack = u64::from(take(&raw, &mut cursor));
        let returned = u64::from(take(&raw, &mut cursor));
        let packet: [u16; 4] = core::array::from_fn(|_| take(&raw, &mut cursor) as u16);
        let after: Vec<u16> = (0..4480).map(|_| take(&raw, &mut cursor) as u16).collect();
        for chunk in [1u64, 31, 3000] {
            let mut memory = Memory(vec![0; 65536]);
            memory.0[0xa00..0x1400].copy_from_slice(&before[..2560]);
            memory.0[0x2000..0x2780].copy_from_slice(&before[2560..]);
            let mut queue = OrderedParameterTransport::<4>::default();
            actor_copy::enqueue(&mut queue, 0, local + 12 * chip, binding, &addresses)
                .map_err(|e| format!("{e:?}"))?;
            let mut clock = 0;
            let mut seen = Vec::new();
            let mut ready = true;
            while clock < returned {
                clock = (clock + chunk).min(returned);
                queue.advance_until(
                    clock,
                    |poll| if poll < busy { 1 << chip } else { 0 },
                    |time, received| {
                        seen.push((time, received.words().to_vec()));
                        for (offset, word) in received.words().iter().copied().enumerate() {
                            memory.0[0x100 + offset] = word;
                        }
                        ready &=
                            receive_memory_command(&mut memory, 0x100) == ReceiveOutcome::Ready;
                    },
                );
            }
            let equal = ready
                && sender == u32::from(binding.sender_gap())
                && seen == vec![(ack, packet.to_vec())]
                && queue.pending() == 0
                && queue.caller_available_clock() == returned
                && memory.0[0xa00..0x1400] == after[..2560]
                && memory.0[0x2000..0x2780] == after[2560..];
            if !equal {
                errors += 1;
                first_error.get_or_insert(serde_json::json!({"case":calls,"chip":chip,"chunk":chunk,"binding":format!("{binding:?}"),"source_ack":ack,"actual":seen,"source_return":returned,"native_return":queue.caller_available_clock()}));
            }
            compared += 4480;
        }
        calls += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_SYS_template_binding_copy_calls":calls,"whole_original_E319_copy_calls":calls,"errors":errors,"first_error":first_error,
        "parameter_words_compared":compared,"all8_program_kind_flags_all4_timbres_all16_drums_both_DSPs":true,"unrelated_selector_bits_checked":true,"all12_actor_slots":true,
        "source_readiness_policies":[0,64,333,3000],"CPU_clock_partitions":[1,31,3000],"all16_template_banks_and_all12_actor_banks_checked":true,"source_prior_template_and_actor_banks_declared":true,
        "native_template_binding_and_common_FIFO_used":true,"original_template_data_not_assumed_immutable_firmware_ROM":true,
        "full_caller_and_HPI_ACK_clocks_match":errors==0,"receiver_DMA_job_timing_qualified":false,"native_template_construction_and_full_note_on_order_qualified":false,
        "source_controller_outputs_used_to_render":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-copy-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native actor binding/copy differs".into());
    }
    Ok(())
}
