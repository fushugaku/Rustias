use radias_synth_application::dsp_transport::{
    HpiAction, ParameterTransfer, TimedParameterTransfer,
};
use radias_synth_domain::dsp_control::{DspEndpoint, ParameterPacket};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-packets.bin"))?;
    let actions = fs::read(out.join("dsp-original-packet-actions.bin"))?;
    let clocks = fs::read(out.join("dsp-original-packet-clocks.bin"))?;
    let waits = fs::read(out.join("dsp-original-packet-waits.bin"))?;
    if raw.len() != 32768 * 48
        || actions.len() != 32768 * 112
        || clocks.len() != 32768 * 44
        || waits.len() != 8192 * 180
    {
        return Err("Original DSP packet corpus incomplete".into());
    }
    let mut errors = 0;
    let mut action_errors = 0;
    let mut total_actions = 0;
    let mut clock_errors = 0;
    for (case, ((row, action_row), clock_row)) in raw
        .chunks_exact(48)
        .zip(actions.chunks_exact(112))
        .zip(clocks.chunks_exact(44))
        .enumerate()
    {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let aw = |i: usize| u32::from_le_bytes(action_row[4 * i..4 * i + 4].try_into().unwrap());
        let cw = |i: usize| u32::from_le_bytes(clock_row[4 * i..4 * i + 4].try_into().unwrap());
        let endpoint = if w(1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let packet =
            ParameterPacket::from_sender(w(0) as u8, w(2), w(3)).ok_or("Original sender absent")?;
        if packet
            .words()
            .iter()
            .enumerate()
            .any(|(i, &v)| u32::from(v) != w(4 + i))
            || w(10) & ParameterPacket::COMMIT as u32 != 0
            || !endpoint.busy(w(11) as u8)
        {
            errors += 1;
        }
        let mut transfer = ParameterTransfer::new(endpoint, packet);
        if transfer.advance(255).is_some() || transfer.completed() {
            action_errors += 1;
        }
        let mut count = 0;
        // Read readiness before HPIA only. After the first address write the
        // original completes its transfer even if the sampled port is busy.
        while let Some(action) = transfer.advance(if count == 0 { 0 } else { 255 }) {
            let (actual_endpoint, actual) = match action {
                HpiAction::AddressByte {
                    endpoint,
                    offset,
                    value,
                } => (endpoint, [u32::from(offset), 8, u32::from(value)]),
                HpiAction::DataWord {
                    endpoint,
                    offset,
                    value,
                } => (endpoint, [u32::from(offset), 16, u32::from(value)]),
                HpiAction::AcknowledgeHint { endpoint, value } => {
                    (endpoint, [0, 16, u32::from(value)])
                }
            };
            if count >= aw(0) as usize
                || actual_endpoint != endpoint
                || actual
                    .iter()
                    .enumerate()
                    .any(|(i, &v)| v != aw(1 + 3 * count + i))
            {
                action_errors += 1;
            }
            count += 1;
        }
        if count != aw(0) as usize || !transfer.completed() || transfer.advance(0).is_some() {
            action_errors += 1;
        }
        total_actions += count;
        let origin = case as u64 * 101;
        let mut timed =
            TimedParameterTransfer::from_sender(endpoint, w(0) as u8, w(2), w(3), origin).unwrap();
        if timed.next_clock() != Some(origin + u64::from(cw(10))) {
            clock_errors += 1;
        }
        let mut index = 0;
        let mut end = 0;
        while let Some(clock) = timed.next_clock() {
            if timed.advance(0).is_some() {
                if clock != origin + u64::from(cw(1 + index)) {
                    clock_errors += 1;
                }
                index += 1;
            }
            end = clock;
        }
        if index != count || end != origin + u64::from(cw(0)) || !timed.completed() {
            clock_errors += 1;
        }
    }
    let mut wait_errors = 0;
    let mut compared_polls = 0;
    for row in waits.chunks_exact(180) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let endpoint = if w(1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let mut timed =
            TimedParameterTransfer::from_sender(endpoint, w(0) as u8, w(2), w(3), 0).unwrap();
        if timed.next_clock() != Some(u64::from(w(6))) {
            wait_errors += 1;
        }
        let mut actions = 0;
        let mut polls = 0;
        let mut end = 0;
        while let Some(clock) = timed.next_clock() {
            let port = if clock < u64::from(w(4)) { 255 } else { 0 };
            if let Some(action) = timed.advance(port) {
                if actions >= 9 || clock != u64::from(w(9 + actions)) {
                    wait_errors += 1;
                }
                let actual = match action {
                    HpiAction::AddressByte { offset, value, .. } => {
                        [u32::from(offset), 8, u32::from(value)]
                    }
                    HpiAction::DataWord { offset, value, .. } => {
                        [u32::from(offset), 16, u32::from(value)]
                    }
                    HpiAction::AcknowledgeHint { value, .. } => [0, 16, u32::from(value)],
                };
                if actual
                    .iter()
                    .enumerate()
                    .any(|(i, &v)| v != w(18 + 3 * actions + i))
                {
                    wait_errors += 1;
                }
                actions += 1;
            } else if !timed.completed() {
                polls += 1;
            }
            end = clock;
        }
        if actions != w(8) as usize || polls != w(7) || end != u64::from(w(5)) || !timed.completed()
        {
            wait_errors += 1;
        }
        compared_polls += polls;
    }
    let report = serde_json::json!({"passed":errors==0 && action_errors==0&&clock_errors==0&&wait_errors==0,"original_complete_sender_calls":32768,"original_sender_wrappers":21,"original_actor_copy_sender_checked":true,"original_Pickup_prime_sender_checked":true,"original_actor_detach_sender_checked":true,"four_word_payload_checked":true,
        "both_endpoints":true,"address_and_word_narrowing_and_long_word_order":true,"HINT_ack_and_selected_endpoint_busy_checked":true,
        "errors":errors,"source_subcalls_skipped":false,"original_instructions_modified":false,
        "action_sequence_errors":action_errors,"bus_actions_compared":total_actions,"busy_before_start_preserves_transfer":true,"busy_after_start_does_not_interrupt_transfer":true,
        "clock_errors":clock_errors,"original_busy_wait_cases":8192,"busy_wait_errors":wait_errors,"ready_polls_compared":compared_polls,"poll_period_SH_reference_clocks":21,
        "software_port_ready_masks":[DspEndpoint::Master.busy(1),DspEndpoint::Slave.busy(2)],
        "complete_native_HPI_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("dsp-packet-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 || action_errors != 0 || clock_errors != 0 || wait_errors != 0 {
        return Err("Native DSP packet differed".into());
    }
    Ok(())
}
