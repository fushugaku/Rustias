use radias_infrastructure::{audio, midi};
mod controls;
mod engine;
mod native;
mod panel;
use eframe::egui;
use engine::{Command, Engine};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct App {
    native: native::NativePanel,
    engine: Engine,
    panel: panel::Panel,
    player: Option<audio::Player>,
    audio_error: String,
    _midi: Option<midi::Connection>,
    native_midi: Arc<AtomicBool>,
    midi_error: String,
    preset: usize,
    gain: f32,
    played_generation: u64,
    held: Vec<u8>,
    held_drum_pads: Vec<u8>,
    working: Option<radias_application::Program>,
    pending_program: Option<radias_application::Program>,
    input_path: String,
    pcm_path: String,
}
impl App {
    fn new(root: PathBuf, no_audio: bool) -> Self {
        let (player, audio_error) = if no_audio {
            (None, "Аудиовыход отключён (--no-audio)".into())
        } else {
            match audio::Player::new() {
                Ok(p) => (Some(p), String::new()),
                Err(e) => (None, e),
            }
        };
        let mut native = native::NativePanel::new(&root, no_audio);
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_COMB").as_deref() == Ok("1")
        {
            for _ in 0..3 {
                native.step_filter2_type();
            }
            native.step_filter_routing();
            native.set_filter2_link(true);
        }
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_NOISE").as_deref() == Ok("1")
        {
            native.set_waveform(5);
            native.pot(0, 7, 80 * 8);
            native.pot(0, 6, 100 * 8);
            native.pot(0, 1, 24 * 8);
        }
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_EG").as_deref() == Ok("1")
        {
            native.set_eg2_program(
                radias_synth_application::voice_envelopes::ModEnvelopeProgram {
                    curve: 2,
                    velocity_level_sensitivity: 48,
                    velocity_time_sensitivity: 96,
                    key_tracking: 80,
                    ..Default::default()
                },
            );
        }
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_PITCH").as_deref() == Ok("1")
        {
            native.set_pitch_program(radias_synth_domain::note_pitch::PitchProgram {
                transpose: 76,
                fine_tune: 48,
                vibrato_intensity: 100,
                bend_range: 76,
                ..Default::default()
            });
        }
        let engine = Engine::new(root, player.as_ref().map(audio::Player::live_input));
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_PORTAMENTO").as_deref() == Ok("1")
        {
            native.set_portamento(radias_synth_domain::portamento::PortamentoProgram {
                time: 80,
                curve: 8,
                switch_required: false,
            });
        }
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_MONO").as_deref() == Ok("1")
        {
            native.set_voice_mode(radias_synth_domain::mono_notes::VoiceMode {
                polyphonic: false,
                multi_trigger: false,
                priority: radias_synth_domain::mono_notes::NotePriority::Last,
            });
        }
        engine.send(Command::RunReference(!native.enabled));
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_VOICE_GROUP").as_deref() == Ok("1")
        {
            native.set_voice_group(radias_synth_domain::voice_group::VoiceGroupProgram {
                raw: 134,
                detune: 64,
                spread: 90,
            });
        }
        if native.enabled
            && let Some(player) = &player
        {
            player.live_input().pause();
        }
        // Screenshot harness only: eframe captures its second UI pass, so
        // explicitly wait for real firmware state rather than drawing a mock.
        if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var_os("RADIAS_SCREENSHOT_READY").is_some()
        {
            let start = std::time::Instant::now();
            while {
                let v = engine.view.lock().unwrap();
                (!v.ready || v.steps < 60_000_000 || !v.lcd.iter().any(|&pixel| pixel != 0))
                    && v.fault.is_empty()
            } && start.elapsed() < std::time::Duration::from_secs(60)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                engine.view.lock().unwrap().ready,
                "Firmware did not reach screenshot-ready state"
            );
        }
        let native_midi = Arc::new(AtomicBool::new(native.enabled));
        let (midi, midi_error) = match midi::virtual_input({
            let commands = engine.commands.clone();
            let native_input = native.midi_input();
            let native_enabled = native_midi.clone();
            move |bytes| {
                if native_enabled.load(Ordering::Relaxed) {
                    if let Some(input) = &native_input {
                        let _ = input.midi(bytes);
                    }
                } else {
                    let _ = commands.send(Command::Midi(bytes.to_vec()));
                }
            }
        }) {
            Ok(port) => (Some(port), String::new()),
            Err(e) => (None, e),
        };
        Self {
            native,
            engine,
            panel: panel::Panel::new(),
            player,
            audio_error,
            _midi: midi,
            native_midi,
            midi_error,
            preset: 0,
            gain: 0.3,
            played_generation: 0,
            held: Vec::new(),
            held_drum_pads: Vec::new(),
            working: None,
            pending_program: None,
            input_path: String::new(),
            pcm_path: String::new(),
        }
    }
    fn note(&mut self, note: u8, down: bool) {
        if self.native.enabled {
            if down && !self.held.contains(&note) {
                self.held.push(note);
                self.native.note(note, true);
            }
            if !down && self.held.contains(&note) {
                self.held.retain(|&n| n != note);
                self.native.note(note, false);
            }
            return;
        }
        if down && !self.held.contains(&note) {
            if !self.engine.view.lock().unwrap().ready {
                return;
            }
            self.held.push(note);
            self.engine.send(Command::Note(note, true));
        }
        if !down && self.held.contains(&note) {
            self.held.retain(|&n| n != note);
            self.engine.send(Command::Note(note, false));
        }
    }
}
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        *ui.visuals_mut() = egui::Visuals::dark();
        ui.visuals_mut().panel_fill = egui::Color32::from_rgb(32, 35, 39);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
        let (ready, status, fault, preview_progress, current, preview_generation) = {
            let v = self.engine.view.lock().unwrap();
            (
                v.ready,
                v.status.clone(),
                v.fault.clone(),
                v.preview_progress,
                v.current.clone(),
                v.preview_generation,
            )
        };
        if preview_generation != self.played_generation {
            self.played_generation = preview_generation;
            if !self.native.enabled
                && let Some(player) = &self.player
            {
                player.play(self.engine.view.lock().unwrap().preview.clone());
            }
        }
        let playback = self.player.as_ref().map(audio::Player::status);
        if self.pending_program.as_ref().is_some_and(|pending| {
            current
                .as_ref()
                .is_some_and(|actual| actual.bytes() == pending.bytes())
        }) {
            self.pending_program = None;
        }
        if self.pending_program.is_none() {
            self.working = current.clone();
        }
        let mut desired_notes = Vec::new();
        let mut desired_drum_pads=Vec::new();
        egui::CentralPanel::default().show(ui,|ui|{
            egui::ScrollArea::vertical().id_salt("application").show(ui,|ui|{
            if ui.available_width()<760.0 {ui.spacing_mut().slider_width=80.0;}
            ui.horizontal_wrapped(|ui|{ui.label(egui::RichText::new("RADIAS").size(26.0).strong());ui.add_space(12.0);
                let label=if self.native.enabled {self.native.stored_program_name().unwrap_or_else(||"Native VA".into())}
                    else {self.engine.names.get(self.preset).cloned().unwrap_or_else(||"Патч".into())};
                let mut requested=None;
                egui::ComboBox::from_id_salt("patch").width(if ui.available_width()<760.0{200.0}else{250.0})
                    .selected_text(label).show_ui(ui,|ui|{for (i,name) in self.engine.names.iter().enumerate(){
                        if ui.selectable_label(i==self.preset,name).clicked(){requested=Some(i);}
                    }});
                if let Some(index)=requested {
                    if self.native.enabled {
                        if self.native.select_stored_program(index){self.preset=index;self.held.clear();self.working=None;}
                    } else {self.preset=index;self.engine.send(Command::Preset(index));self.working=None;}
                }
                if ui.add_enabled(ready&&preview_progress.is_none()&&!self.native.enabled,egui::Button::new("Прослушать")).clicked(){self.engine.send(Command::Preview);}
                if ui.button("Стоп").clicked(){if let Some(p)=&self.player{p.stop();}self.native.stop();self.engine.send(Command::Stop);}
                ui.add(egui::Slider::new(&mut self.gain,0.0..=1.0).text("Громкость").show_value(false));if let Some(p)=&self.player{p.gain(self.gain);}ui.label(if self.native.enabled{"Native VA"}else if playback.as_ref().is_some_and(|p|p.preview_playing){"Воспроизведение"}else{&status});
            });
            if !fault.is_empty(){ui.colored_label(egui::Color32::from_rgb(241,114,115),fault);if ui.button("Перезапуск").clicked(){self.engine.send(Command::Reset);}}
            if let Some(error)=self.engine.view.lock().unwrap().command_error.clone(){ui.colored_label(egui::Color32::from_rgb(241,114,115),error);}
            if let Some(p)=preview_progress{ui.add(egui::ProgressBar::new(p).show_percentage());}
            if self.native.show(ui,self.gain){
                self.held.clear();self.native_midi.store(self.native.enabled,Ordering::Relaxed);self.engine.send(Command::Stop);self.engine.send(Command::RunReference(!self.native.enabled));
                if let Some(player)=&self.player{if self.native.enabled{player.live_input().pause();}else{player.live_input().resume();}}
            }
            if self.native.enabled{let(cutoff,resonance,filter_type)=self.native.filter_values();self.panel.native_filter_values(cutoff,resonance,filter_type,self.native.waveform_name());self.panel.native_envelope_values(self.native.envelope_values());self.panel.native_amplifier_level(self.native.amplifier_level());self.panel.native_pan_position(self.native.pan_position());self.panel.native_mixer_levels(self.native.mixer_levels());self.panel.native_secondary_pitch(self.native.secondary_pitch());self.panel.native_primary_controls(self.native.primary_controls());let(auxiliary,intensity,key_tracking)=self.native.auxiliary_values();self.panel.native_auxiliary_values(auxiliary,intensity,key_tracking);self.panel.native_lfo_values(self.native.lfo_values());self.panel.native_portamento_time(self.native.portamento_time());}
            let native_state=self.native.enabled.then(||self.native.panel_state());
            let panel_events=self.panel.show(ui,&self.engine,native_state);
            if self.native.enabled && self.native.has_drum_program() {
                desired_drum_pads.extend(panel_events.notes.into_iter().filter_map(|n|n.checked_sub(60)).filter(|n|*n<16));
            } else {desired_notes.extend(panel_events.notes);}
            for(ch,mux,value)in panel_events.pots{self.native.pot(ch,mux,value);}
            for action in panel_events.actions{match action{
                panel::PanelAction::SelectTimbre(index)=>self.native.select_timbre(index),
                panel::PanelAction::ToggleTimbre(index)=>{let enabled=self.native.panel_state().enabled[index];self.native.set_timbre_enabled(index,!enabled);},
                panel::PanelAction::StepWaveform(direction)=>self.native.step_waveform(direction),
                panel::PanelAction::StepPrimaryModulation=>self.native.step_primary_modulation(),
                panel::PanelAction::ToggleVoiceGroup=>self.native.toggle_voice_group(),
                panel::PanelAction::StepFilterRouting=>self.native.step_filter_routing(),
                panel::PanelAction::StepFilter2Type=>self.native.step_filter2_type(),
                panel::PanelAction::SetFilter2Link(linked)=>self.native.set_filter2_link(linked),
                panel::PanelAction::StepShaper=>self.native.step_shaper(),
                panel::PanelAction::SetShaperMode(mode)=>self.native.set_shaper_mode(mode),
                panel::PanelAction::SetShaperPosition(position)=>self.native.set_shaper_position(position),
                panel::PanelAction::StepSecondaryWaveform=>self.native.step_secondary_waveform(),
                panel::PanelAction::StepSecondaryModulation=>self.native.step_secondary_modulation(),
            }}
            ui.add_space(8.0);if !self.native.enabled{ui.horizontal_wrapped(|ui|{ui.label("Выход до FXD03 · 48 кГц");let v=self.engine.view.lock().unwrap();ui.label(&v.pcm_description);if let Some(path)=v.wav.as_ref(){ui.label(path.file_name().unwrap().to_string_lossy());}ui.label("MIDI: RADIAS Rust");});
            ui.horizontal_wrapped(|ui|{
                if let Some(p)=&playback{
                    ui.label(format!("{} · {} Гц",p.device,p.sample_rate));
                    let db=if p.peak>0.0{format!("{:.0} dB",20.0*p.peak.log10())}else{"−∞ dB".into()};
                    ui.label(db);
                    if let Some(error)=&p.error{ui.colored_label(egui::Color32::from_rgb(241,114,115),error);}
                }
                if !self.audio_error.is_empty(){ui.colored_label(egui::Color32::from_rgb(241,114,115),&self.audio_error);}
                if ui.button("Переподключить звук").clicked(){
                    match audio::Player::new(){Ok(player)=>{player.gain(self.gain);self.engine.send(Command::Audio(Some(player.live_input())));self.player=Some(player);self.audio_error.clear();},Err(error)=>self.audio_error=error}
                }
            });
            if !self.midi_error.is_empty(){ui.label(&self.midi_error);}}
            ui.separator();ui.horizontal_wrapped(|ui|{for note in 48..73{let black=matches!(note%12,1|3|6|8|10);let pressed=self.held.contains(&note);let text=egui::RichText::new(note.to_string()).color(if black||pressed{egui::Color32::WHITE}else{egui::Color32::from_rgb(38,43,48)});let response=ui.add_enabled(ready||self.native.enabled,egui::Button::new(text).fill(if pressed{egui::Color32::from_rgb(200,75,89)}else if black{egui::Color32::from_rgb(45,48,52)}else{egui::Color32::from_rgb(196,201,207)}).min_size(egui::vec2(30.0,38.0)));if controls::note_key_active(&response,pressed){desired_notes.push(note);}}});
            ui.add_enabled_ui(!self.native.enabled,|ui|{egui::CollapsingHeader::new("Параметры").show(ui,|ui|{if let Some(p)=self.working.as_mut(){let mut changed=false;
                for timbre in 0..4{egui::CollapsingHeader::new(format!("Timbre {}",timbre+1)).show(ui,|ui|{let body=64+timbre*228;
                    for (label,offset,mask,shift,max) in [("OSC 1 wave",body+22,0x0f,0,8u8),("OSC 1 mod",body+22,0x30,4,3),("OSC 2 wave",body+27,0x03,0,3),("OSC 2 mod",body+27,0x30,4,3)]{ui.horizontal(|ui|{ui.label(label);let mut value=(p.bytes()[offset]&mask)>>shift;if ui.add(egui::Slider::new(&mut value,0..=max)).changed(){let _=p.set_bits(offset,mask,shift,value);changed=true;}});}
                    let rows=[("OSC 1 control 1",body+23,0,127),("OSC 1 control 2",body+24,0,127),("OSC 1 level",body+30,0,127),("OSC 2 level",body+31,0,127),("Noise",body+32,0,127),("Filter cutoff",body+35,0,127),("Resonance",body+36,0,127),("EG 1 depth",body+37,0,127),("AMP level",body+45,0,127),("Pan",body+49,0,127),("EG 1 attack",body+52,0,127),("EG 1 decay",body+53,0,127),("EG 1 sustain",body+54,0,127),("EG 1 release",body+55,0,127),("EG 2 attack",body+60,0,127),("EG 2 decay",body+61,0,127),("EG 2 sustain",body+62,0,127),("EG 2 release",body+63,0,127)];
                    egui::Grid::new(format!("timbre-{timbre}")).num_columns(2).show(ui,|ui|{for (label,offset,min,max) in rows{ui.label(label);let mut value=p.bytes()[offset] as u16;if ui.add(egui::Slider::new(&mut value,min..=max)).changed(){let _=p.set_byte(offset,value as u8);changed=true;}ui.end_row();}});});}
                egui::CollapsingHeader::new("Полный payload").show(ui,|ui|{egui::ScrollArea::vertical().max_height(240.0).show_rows(ui,22.0,1790,|ui,range|{for offset in range{ui.horizontal(|ui|{ui.monospace(format!("{offset:04}"));let mut value=p.bytes()[offset];if ui.add(egui::DragValue::new(&mut value).range(0..=255)).changed(){let _=p.set_byte(offset,value);changed=true;}});}});});
                if changed{self.pending_program=Some(p.clone());self.engine.send(Command::Program(p.clone()));}
            }});});
            egui::CollapsingHeader::new("Вход и PCM").show(ui,|ui|{ui.horizontal_wrapped(|ui|{ui.add(egui::TextEdit::singleline(&mut self.input_path).hint_text("WAV, mono/stereo 48 кГц").desired_width(280.0));if ui.button("Подключить WAV").clicked(){self.engine.send(Command::InputWave(self.input_path.clone().into()));}});ui.horizontal_wrapped(|ui|{ui.add(egui::TextEdit::singleline(&mut self.pcm_path).hint_text("KORG/X3160PCM bank").desired_width(280.0));if ui.button("Подключить PCM").clicked(){self.engine.send(Command::Pcm(self.pcm_path.clone().into()));}});});
            egui::CollapsingHeader::new("Состояние").show(ui,|ui|{let v=self.engine.view.lock().unwrap();ui.monospace(format!("SH steps: {}   Frames: {}   Скорость: {:.3}×",v.steps,v.native_frames,v.speed));if let Some(p)=&playback{ui.monospace(format!("Audio callbacks: {}   Ненулевых кадров: {}   Буфер: {}   Каналов: {}",p.callbacks,p.audible_frames,p.buffered_frames,p.channels));}ui.label("FXD03 и выход DAC ещё не восстановлены. WAV сохраняет исходные Q31-сэмплы выбранного промежуточного выхода. При скорости ниже 1× игра с клавиатуры звучит с паузами; «Прослушать» воспроизводит готовый рендер без пауз.");});
            });
        });
        if ui.input(|i| i.focused) && !ui.ctx().egui_wants_keyboard_input() {
            for (key, note) in [
                (egui::Key::A, 60),
                (egui::Key::W, 61),
                (egui::Key::S, 62),
                (egui::Key::E, 63),
                (egui::Key::D, 64),
                (egui::Key::F, 65),
                (egui::Key::T, 66),
                (egui::Key::G, 67),
                (egui::Key::Y, 68),
                (egui::Key::H, 69),
                (egui::Key::U, 70),
                (egui::Key::J, 71),
                (egui::Key::K, 72),
            ] {
                if ui.input(|i| i.key_down(key)) {
                    desired_notes.push(note);
                }
            }
        }
        desired_notes.sort_unstable();
        desired_notes.dedup();
        let previous = self.held.clone();
        for (note, down) in controls::note_transitions(&previous, &desired_notes) {
            self.note(note, down);
        }
        desired_drum_pads.sort_unstable();desired_drum_pads.dedup();
        for (instrument,down) in controls::note_transitions(&self.held_drum_pads,&desired_drum_pads) {
            self.native.drum_pad(instrument,down);
        }
        self.held_drum_pads=desired_drum_pads;
        self.native.flush_drum_edits();
    }
}
fn main() -> eframe::Result {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut no_audio = false;
    let mut size = [1440.0, 920.0];
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                if let Some(path) = args.next() {
                    root = path.into();
                }
            }
            "--no-audio" => no_audio = true,
            "--size" => {
                if let Some(value) = args.next() {
                    if let Some((w, h)) = value.split_once('x') {
                        if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                            size = [w, h];
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size([390.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RADIAS",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            let mut style = egui::Style::default();
            style.visuals = egui::Visuals::dark();
            style.visuals.panel_fill = egui::Color32::from_rgb(32, 35, 39);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.interact_size.y = 32.0;
            style.spacing.button_padding.y = 8.0;
            style.spacing.icon_width = 16.0;
            cc.egui_ctx.set_global_style(style);
            Ok(Box::new(App::new(root, no_audio)))
        }),
    )
}
