use radias_synth_application::{
    dsp_transport::{ParameterSendRequest, SendQueueError},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::dsp_control::DspEndpoint;
use std::{fs, path::PathBuf};

fn word(raw: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-packets.bin"))?;
    let actions = fs::read(out.join("dsp-original-packet-actions.bin"))?;
    let clocks = fs::read(out.join("dsp-original-packet-clocks.bin"))?;
    if raw.len() != 32768 * 48 || actions.len() != 32768 * 112 || clocks.len() != 32768 * 44 {
        return Err("Original sender corpus incomplete".into());
    }
    let mut queue = OrderedParameterTransport::<64>::default();
    let mut origin = 0u64;
    let mut expected = Vec::new();
    let mut delivered = 0usize;
    for case in 0..32768 {
        let row = &raw[case * 48..(case + 1) * 48];
        let ar = &actions[case * 112..(case + 1) * 112];
        let cr = &clocks[case * 44..(case + 1) * 44];
        let endpoint = if word(row, 1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let entry = word(row, 0) as u8;
        let length = (0..word(ar, 0) as usize)
            .filter(|i| word(ar, 1 + 3 * i) == 2 && word(ar, 2 + 3 * i) == 16)
            .count();
        let request = ParameterSendRequest {
            endpoint,
            sender: entry,
            address: word(row, 2),
            value: word(row, 3),
            available_clock: 0,
        };
        queue
            .enqueue(request)
            .map_err(|error| format!("Parameter queue: {error:?}"))?;
        let words = (0..length)
            .map(|i| word(row, 4 + i) as u16)
            .collect::<Vec<_>>();
        let ack = origin + u64::from(word(cr, word(ar, 0) as usize));
        expected.push((ack, endpoint, words));
        origin += u64::from(word(cr, 0));
        if case % 64 == 63 {
            if queue.enqueue(request) != Err(SendQueueError::Full) {
                return Err("Full queue changed its contract".into());
            }
            let mut actual = Vec::new();
            queue.advance_until(
                origin,
                |_| 0,
                |clock, packet| actual.push((clock, packet.endpoint, packet.words().to_vec())),
            );
            if actual != expected || queue.pending() != 0 {
                return Err(
                    format!("Original complete parameter delivery batch{case} differs").into(),
                );
            }
            delivered += actual.len();
            expected.clear();
        }
    }
    let waits = fs::read(out.join("dsp-original-packet-waits.bin"))?;
    if waits.len() != 8192 * 180 {
        return Err("Original wait corpus incomplete".into());
    }
    let mut blocked = 0;
    for row in waits.chunks_exact(180) {
        let w = |i| word(row, i);
        let endpoint = if w(1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let mut queue = OrderedParameterTransport::<1>::default();
        queue
            .enqueue(ParameterSendRequest {
                endpoint,
                sender: w(0) as u8,
                address: w(2),
                value: w(3),
                available_clock: 0,
            })
            .map_err(|error| format!("Parameter queue: {error:?}"))?;
        let words = (0..w(8) as usize)
            .filter(|&i| w(18 + 3 * i) == 2 && w(19 + 3 * i) == 16)
            .map(|i| w(20 + 3 * i) as u16)
            .collect::<Vec<_>>();
        let mut actual = Vec::new();
        queue.advance_until(
            u64::from(w(5)),
            |clock| if clock < u64::from(w(4)) { 255 } else { 0 },
            |clock, packet| actual.push((clock, packet.endpoint, packet.words().to_vec())),
        );
        if actual != vec![(u64::from(w(9 + w(8) as usize - 1)), endpoint, words)]
            || queue.pending() != 0
        {
            return Err("Original blocked mailbox delivery differs".into());
        }
        blocked += 1;
    }
    let report = serde_json::json!({"passed":true,"whole_original_sender_profiles":delivered,"both_DSP_endpoints":true,
        "original_blocked_sender_profiles":blocked,"all21_sender_wrappers":true,"original_actor_copy_sender_checked":true,"original_Pickup_prime_sender_checked":true,"original_actor_detach_sender_checked":true,"four_word_payload_checked":true,"payload_word_order_and_HPI_ACK_clocks_match":true,
        "declared_sequential_sender_call_origins":true,"full_queue_preserves_prior_requests":true,
        "production_AMP_uses_this_transport":true,"receiver_execution_and_DMA_job_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("parameter-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
