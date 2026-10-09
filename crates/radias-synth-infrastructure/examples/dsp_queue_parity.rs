use radias_synth_application::dsp_transport::{
    HpiAction, ParameterSendQueue, ParameterSendRequest, SendQueueError,
};
use radias_synth_domain::dsp_control::DspEndpoint;
use std::{fs, path::PathBuf};
fn action_words(action: HpiAction) -> (DspEndpoint, [u32; 3]) {
    match action {
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
        HpiAction::AcknowledgeHint { endpoint, value } => (endpoint, [0, 16, u32::from(value)]),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("dsp-original-packets.bin"))?;
    let actions = fs::read(out.join("dsp-original-packet-actions.bin"))?;
    let clocks = fs::read(out.join("dsp-original-packet-clocks.bin"))?;
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    let mut origin = 0;
    for case in 0..256 {
        let row = &raw[case * 48..(case + 1) * 48];
        let ar = &actions[case * 112..(case + 1) * 112];
        let cr = &clocks[case * 44..(case + 1) * 44];
        let w = |r: &[u8], i: usize| u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap());
        let endpoint = if w(row, 1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        requests.push(ParameterSendRequest {
            endpoint,
            sender: w(row, 0) as u8,
            address: w(row, 2),
            value: w(row, 3),
            available_clock: 0,
        });
        for i in 0..w(ar, 0) as usize {
            expected.push((
                origin + u64::from(w(cr, 1 + i)),
                endpoint,
                [w(ar, 1 + 3 * i), w(ar, 2 + 3 * i), w(ar, 3 + 3 * i)],
            ));
        }
        origin += u64::from(w(cr, 0));
    }
    let mut partitions = Vec::new();
    for chunk in [1, 31, 100, 3000, 8000, 65536] {
        let mut queue = ParameterSendQueue::<256>::default();
        for &request in &requests {
            queue.push(request).unwrap();
        }
        let full_rejected = queue.push(requests[0]) == Err(SendQueueError::Full);
        let mut native = Vec::new();
        let mut end = 0;
        while end < origin {
            end = (end + chunk).min(origin);
            queue.advance_until(
                end,
                |_| 0,
                |clock, action| {
                    let (endpoint, words) = action_words(action);
                    native.push((clock, endpoint, words));
                },
            );
        }
        if native != expected || queue.pending() != 0 || !full_rejected {
            return Err(format!("Native send queue partition {chunk} differed").into());
        }
        partitions.push(chunk);
    }
    let waits = fs::read(out.join("dsp-original-packet-waits.bin"))?;
    let mut waited_actions = 0;
    for row in waits.chunks_exact(180) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let endpoint = if w(1) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let mut queue = ParameterSendQueue::<1>::default();
        queue
            .push(ParameterSendRequest {
                endpoint,
                sender: w(0) as u8,
                address: w(2),
                value: w(3),
                available_clock: 0,
            })
            .unwrap();
        let mut native = Vec::new();
        queue.advance_until(
            u64::from(w(5)),
            |clock| if clock < u64::from(w(4)) { 255 } else { 0 },
            |clock, action| {
                let (endpoint, words) = action_words(action);
                native.push((clock, endpoint, words));
            },
        );
        let expected = (0..w(8) as usize)
            .map(|i| {
                (
                    u64::from(w(9 + i)),
                    endpoint,
                    [w(18 + 3 * i), w(19 + 3 * i), w(20 + 3 * i)],
                )
            })
            .collect::<Vec<_>>();
        if native != expected || queue.pending() != 0 {
            return Err("Queued original busy transfer differed".into());
        }
        waited_actions += native.len();
    }
    let report = serde_json::json!({"passed":true,"whole_original_sender_sequence":256,"ordered_writes":expected.len(),"original_busy_transfers":8192,"waited_writes":waited_actions,
        "CPU_clock_partitions":partitions,"both_processors_share_one_ordered_controller_queue":true,"overflow_preserves_queue":true,
        "source_instruction_execution_in_native_queue":false,"complete_production_HPI_or_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("dsp-queue-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
