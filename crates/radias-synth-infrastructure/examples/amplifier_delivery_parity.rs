use radias_synth_domain::{
    amplifier_delivery::{AmplifierDelivery, AmplifierPacket},
    dsp_control::ParameterPacket,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let tables = radias_synth_infrastructure::firmware::amplifier_rate_table(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let raw = fs::read(out.join("amplifier-original-delivery.bin"))?;
    if raw.len() != 65536 * 56 {
        return Err("Original AMP delivery corpus incomplete".into());
    }
    let mut errors = 0;
    let mut first_error = None;
    for (case, row) in raw.chunks_exact(56).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut delivery = AmplifierDelivery { mode: w(4) as u8 };
        let update = match w(0) {
            0 | 1 => delivery.binding(&tables, w(1) as u8, w(2) as i16, w(3) as i16, w(0) == 1),
            2 => delivery.service(&tables, w(3) as i16),
            3 => AmplifierDelivery::reset(&tables),
            _ => unreachable!(),
        };
        let (packet, target) = match update {
            AmplifierPacket::RateAndTarget { rate, target } => (
                ParameterPacket::long(
                    22,
                    0x207c,
                    (u32::from(rate) << 16) | u32::from(target as u16),
                ),
                target as u16,
            ),
            AmplifierPacket::Target(target) => (
                ParameterPacket::word(0, 0x207d, u32::from(target as u16)),
                target as u16,
            ),
        };
        if u32::from(delivery.mode) != w(5)
            || u32::from(target) != w(6)
            || packet
                .words()
                .iter()
                .enumerate()
                .any(|(i, &value)| u32::from(value) != w(8 + i))
        {
            errors += 1;
            first_error.get_or_insert(serde_json::json!({"case":case,"action":w(0),"attack":w(1),"modulation":w(2),"mode":w(4),"native_mode":delivery.mode,"original_mode":w(5),"native_words":packet.words(),"original_words":(8..14).map(w).collect::<Vec<_>>()}));
        }
    }
    let report = serde_json::json!({"passed":errors==0,"complete_original_controller_calls":65536,"errors":errors,"first_error":first_error,
        "source_subcalls_skipped":false,"original_instructions_modified":false,"rate_table_from_SYS":tables.attack_rates,
        "soft_binding_rate":tables.soft_binding_rate,"regular_rate":tables.regular_rate,"termination_rate":tables.termination_rate,"reset_rate":tables.reset_rate,
        "signed_mode_and_attack_modulation_checked":true,"complete_native_HPI_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("amplifier-delivery-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native AMP packet delivery differs".into());
    }
    Ok(())
}
