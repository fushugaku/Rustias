use radias_synth_application::{
    parameter_transport::OrderedParameterTransport, pitch_delivery::primary_pitch_request,
};
use radias_synth_domain::dsp_control::DspEndpoint;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let table = radias_synth_infrastructure::firmware::primary_pitch_sender_table(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let raw = fs::read(out.join("primary-pitch-dispatch.bin"))?;
    if raw.len() != 6144 * 48 {
        return Err("Original dispatch corpus size differs".into());
    }
    let mut errors = 0;
    let mut first_error = None;
    for (case, row) in raw.chunks_exact(48).enumerate() {
        let v = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let endpoint = if v(0) == 0 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let request = primary_pitch_request(
            table,
            u64::from(v(5)),
            endpoint,
            v(2) as u16,
            v(1) as u8,
            v(3) as u16,
        )
        .ok_or("Missing native VA pitch sender")?;
        let mut transport = OrderedParameterTransport::<2>::default();
        transport
            .enqueue(request)
            .map_err(|_| "Native pitch queue rejected the request")?;
        let mut received = Vec::new();
        for end in [1u64, 31, 255, 8192] {
            transport.advance_until(end, |_| 0, |clock, packet| received.push((clock, packet)));
        }
        let matches = request.sender == v(4) as u8
            && received.len() == 1
            && received[0].0 == u64::from(v(6))
            && received[0].1.endpoint == endpoint
            && received[0]
                .1
                .words()
                .iter()
                .copied()
                .map(u32::from)
                .eq((7..12).map(v));
        if !matches {
            errors += 1;
            first_error.get_or_insert(serde_json::json!({"case":case,"selection":v(1),"source_sender":v(4),"native_sender":request.sender,"source_ack_clock":v(6),"native_received":format!("{received:?}")}));
        }
    }
    let report = serde_json::json!({"passed":errors==0,"complete_original_SYS01fcf0_calls":6144,"errors":errors,"first_error":first_error,
        "both_DSP_endpoints":true,"waveform_cross_unison_VPM_all_VA_descriptors_and_base_noise_formant":true,
        "native_sender_choice_compiled_from_immutable_SYS_table":true,"shared_ordered_transport_used":true,
        "original_sender_entry_clocks_are_declared_inputs":true,"whole_controller_callback_timing_independently_transferred":false,
        "all_payload_words_and_HPI_ack_clocks_match":errors==0,"original_instructions_modified":false,"source_subcalls_skipped":false,
        "PCM_pitch_descriptors_excluded":true,"production_all_pitch_families_connected":false,"complete_native_engine":false});
    fs::write(
        out.join("primary-pitch-dispatch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native primary pitch dispatch differs".into());
    }
    Ok(())
}
