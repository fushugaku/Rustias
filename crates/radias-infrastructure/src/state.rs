use radias_application::Machine;
use radias_domain::dsp::c55::C55;
use serde_json::{Value, json};
pub fn dsp_state(d: &C55, name: &str) -> Value {
    json!({"name":name,"pc":d.pc,"steps":d.steps,"interrupts":d.interrupts,"dma_elements":d.dma_elements,"sleeping":d.sleeping,"started":d.started,"host_words":d.host_words,"fault":d.fault,"sp":d.sp,"ssp":d.ssp,"hpic":d.hpic,"hpia":d.hpia,"st":d.st,"xar":d.xar,"t":d.t,"ac":d.ac,
        "watch":d.watched_writes.iter().map(|v|json!({"address":v.address,"pc":v.pc,"value":v.value,"host":v.host,"dma":v.dma})).collect::<Vec<_>>(),
        "host_commands":d.host_commands.iter().map(|v|json!({"pc":v.pc,"words":v.words})).collect::<Vec<_>>(),
        "dma":d.dma.iter().enumerate().map(|(channel,v)|json!({"channel":channel,"active":v.active,"waiting":v.waiting,"source_byte_address":v.source,"target_byte_address":v.target,"source_element":v.source_element,"source_frame":v.source_frame,"target_element":v.element,"target_frame":v.frame,"fifo_elements":v.fifo.len(),"config":v.config})).collect::<Vec<_>>()})
}
pub fn machine_state(m: &Machine) -> Value {
    let cpu = &m.cpu;
    let b = &m.board;
    json!({"pc":cpu.pc,"sr":cpu.sr,"pr":cpu.pr,"gbr":cpu.gbr,"vbr":cpu.vbr,"steps":cpu.steps,"cycles":cpu.cycles,"interrupts":cpu.interrupts,"sleeping":cpu.sleeping,"mach":cpu.mach,"macl":cpu.macl,"ssr":cpu.ssr,"spc":cpu.spc,"delay_pending":cpu.delayed,"fault":m.fault,"registers":cpu.r,"bank":cpu.bank,
        "lcd":b.lcd.pixels(),"lcd_enabled":b.lcd.enabled,"lcd_writes":b.lcd.writes,"lcd_commands":b.lcd.commands,"usr_reads":b.usr_reads,"pcm_reads":b.pcm_reads,
        "unknown_reads":b.unknown_reads.iter().map(|(&address,&count)|json!({"address":address,"count":count})).collect::<Vec<_>>(),"panel_adc":b.panel_adc.iter().flatten().copied().collect::<Vec<_>>(),"pcm_skipped":b.pcm_skipped,"backup_global_bytes":b.backup_global_data.len(),
        "backup_scope":if b.backup_usr_data.len()==0x100000{"programs-drums-global-templates-formants"}else if b.backup_usr_data.len()==0x90000{"programs-drums-global-templates"}else if !b.backup_usr_data.is_empty(){"programs-drums-global"}else if b.backup_global_data.is_empty(){"none"}else{"global-only"},
        "backup_template_counts":b.backup_librarian.templates.iter().map(Vec::len).collect::<Vec<_>>(),"pcm_placeholder_words":b.pcm_placeholder_words,"backup_formant_records":b.backup_librarian.formants.len(),"backup_formant_frames":b.backup_librarian.formants.iter().map(|r|radias_domain::backup::formant_frames(r).unwrap_or(0)).collect::<Vec<_>>(),
        "captured_formant_import_implemented":true,"captured_formant_playback_verified":false,"captured_formant_recording_verified":false,"captured_formant_verification_scope":"Rust formant replay pending","captured_formant_hardware_conformance_verified":false,
        "audio_frame_clocks":b.audio_frames,"dsp_serial_frame_clocks":b.dsp_serial_frames,"serial_words":b.serial_words,"adc_idle_noise":b.adc_idle_noise,"fxd03_mode":if b.fxd_return_zero{"zero-return-diagnostic"}else{"transport-passthrough"},"dsp_uploaded_bytes":b.dsp_upload.len(),"dsp_reads":b.dsp_reads,"dsp_mode":"partial-c55x","dsp":[dsp_state(&b.dsp[0],"Master"),dsp_state(&b.dsp[1],"Slave")],
        "scif_tx_fifo":b.scif.tx.len(),"scif_tx_active":b.scif.transmit_shift.is_some(),"scif_tx_ignored":b.scif.transmit_ignored,
        "fxd_upload":{"execution_implemented":false,"words48":b.fxd_upload.words48.len(),"words32":b.fxd_upload.words32.len(),"uploaded48":b.fxd_upload.uploaded48,"uploaded32":b.fxd_upload.uploaded32,"packets48":b.fxd_upload.packets48,"packets32":b.fxd_upload.packets32,"pending48":b.fxd_upload.p48.active,"pending32":b.fxd_upload.p32.active,"error":b.fxd_upload.error},"io":b.io_log,"midi_out":b.midi_out})
}
pub fn clock_info() -> Value {
    json!({"cpu_hz":144000000,"peripheral_hz":24000000,"dsp_hz":300000000,"codec_frame_hz":48000,"dsp_bus_word_hz":768000,"cpu_frequency_changes_modeled":false,"instruction_pipeline_timing_modeled":false})
}
