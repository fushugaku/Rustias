//! Decode source library and all four timbres without firmware CPU dependencies.
use radias_synth_application::program::TimbreControls;
use radias_synth_infrastructure::rdl;
use std::{fs, io::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let source = fs::read(root.join("firmware/Radias-backup.rdl"))?;
    let programs = rdl::programs(&source)?;
    let mut bad_magic = source.clone();
    bad_magic[0] ^= 1;
    let mut bad_length = source.clone();
    bad_length[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let malformed = [
        &bad_magic[..],
        &bad_length[..],
        &source[..source.len() - 1],
        &source[..11],
    ];
    for input in malformed {
        if rdl::programs(input).is_ok() {
            return Err("Malformed RDL container was accepted".into());
        }
    }
    fs::write(
        root.join("runs/native-clone/native-rdl-validation.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"malformed_containers_rejected":malformed.len(),"source_bank_audio_qualified":false}),
        )?,
    )?;
    let mut raw = fs::File::create(root.join("runs/native-clone/native-rdl-programs.bin"))?;
    let mut output = Vec::new();
    for (slot, program) in programs.iter().enumerate() {
        raw.write_all(program.bytes())?;
        let mut timbres = Vec::new();
        for index in 0..4 {
            let timbre = program.timbre(index).unwrap();
            let controls = TimbreControls::from_timbre(timbre)
                .map_err(|e| format!("Program {slot}, timbre {index}, destination {e:?}"))?;
            timbres.push(serde_json::json!({
                "enabled":timbre.enabled(),"channel":timbre.channel(0),"key_window":timbre.key_window(),
                "oscillator_selection":controls.oscillator_selection,"oscillator_controls":controls.oscillator_controls,
                "secondary":[controls.secondary.selection,controls.secondary.pitch.semitone,controls.secondary.pitch.fine_tune],
                "mixer":controls.mixer.levels,"filter_route":controls.filter_route,"filter_type":controls.filter_type,
                "cutoff":controls.cutoff,"resonance":controls.resonance,"eg1_intensity":controls.eg1_intensity,
                "filter_key_tracking":controls.filter_key_tracking,"amplifier_level":controls.amplifier_level,"pan":controls.pan,
                "envelopes":controls.envelope.map(|e| [e.adsr[0],e.adsr[1],e.adsr[2],e.adsr[3],e.curve,e.velocity_level_sensitivity,e.velocity_time_sensitivity,e.key_tracking]),
                "lfo":core::array::from_fn::<_,2,_>(|i| {let l=controls.modulation.lfo[i];[l.waveform,l.shape,l.frequency,l.phase_sync,controls.modulation.tempo_divisions[i]]}),
                "patches":controls.modulation.routes.map(|p|[p.source,p.destination.index() as u8,p.intensity]),
            }));
        }
        output.push(serde_json::json!({"slot":slot,"name":String::from_utf8_lossy(program.name()),
            "tempo_tenths":program.tempo_tenths(),"drum_timbre":program.drum_timbre(),
            "arpeggiator_flags":program.arpeggiator_flags(),"vocoder_flags":program.vocoder_flags(),"timbres":timbres}));
    }
    fs::write(
        root.join("runs/native-clone/native-rdl-controls.json"),
        serde_json::to_vec_pretty(&output)?,
    )?;
    println!(
        "Decoded {} lossless programs and {} timbre control blocks",
        programs.len(),
        programs.len() * 4
    );
    Ok(())
}
