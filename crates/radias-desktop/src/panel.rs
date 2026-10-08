use super::{
    controls,
    engine::{Command, Engine},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use serde_json::Value;
const SILVER: Color32 = Color32::from_rgb(203, 208, 213);
const INK: Color32 = Color32::from_rgb(38, 43, 48);
const RED: Color32 = Color32::from_rgb(218, 83, 92);
const BLUE: Color32 = Color32::from_rgb(199, 223, 235);
pub const PRIMARY_MODE_NAMES: [&str; 4] = ["Waveform", "Cross", "Unison", "VPM"];
pub const PRIMARY_WAVEFORM_NAMES: [&str; 6] =
    ["Saw", "Pulse", "Triangle", "Sine", "Noise", "Formant"];
pub fn primary_selection_supported(waveform: usize, mode: u8) -> bool {
    waveform < PRIMARY_WAVEFORM_NAMES.len()
        && (mode as usize) < PRIMARY_MODE_NAMES.len()
        && (waveform < 4 || mode == 0)
}
pub const FILTER_ROUTING_NAMES: [&str; 4] = ["Single", "Serial", "Parallel", "Individual"];
pub const FILTER2_NAMES: [&str; 4] = ["LPF", "HPF", "BPF", "COMB"];
pub const SHAPER_NAMES: [&str; 13] = [
    "Off",
    "Drive",
    "Hard Clip",
    "Decimator",
    "OctSaw",
    "MultiTri",
    "MultiSin",
    "SubOSC Saw",
    "SubOSC Square",
    "SubOSC Triangle",
    "SubOSC Sine",
    "Pickup",
    "Lvl Boost",
];
const SHAPER_LCD_NAMES: [&str; 13] = [
    "Off",
    "Drive",
    "HardClip",
    "Decim",
    "OctSaw",
    "MultiTri",
    "MultiSin",
    "SubSaw",
    "SubSquare",
    "SubTri",
    "SubSine",
    "Pickup",
    "LvlBoost",
];
pub const SHAPER_POSITION_NAMES: [&str; 2] = ["Перед Filter 1", "Перед AMP"];
#[derive(Clone, Copy)]
pub struct NativePanelState {
    pub voice_mode: radias_synth_domain::mono_notes::VoiceMode,
    pub voice_group: radias_synth_domain::voice_group::VoiceGroupProgram,
    pub selected: usize,
    pub enabled: [bool; 4],
    pub waveform: usize,
    pub primary_mode: u8,
    pub filter_route: u8,
    pub filter2_type: u8,
    pub filter2: [u8; 2],
    pub filter2_link: bool,
    pub shaper_mode: u8,
    pub shaper_ws_mode: u8,
    pub shaper_position: u8,
    pub shaper_depth: u8,
    pub secondary: u8,
    pub lfo_tempo: [bool; 2],
}
impl NativePanelState {
    fn primary_controls_supported(self) -> bool {
        primary_selection_supported(self.waveform, self.primary_mode)
    }
    fn filter2_supported(self) -> bool {
        (self.filter_route as usize) < FILTER_ROUTING_NAMES.len()
            && (self.filter2_type as usize) < FILTER2_NAMES.len()
    }
    fn filter2_link_supported(self) -> bool {
        self.filter2_supported()
    }
    fn shaper_supported(self) -> bool {
        (self.shaper_mode as usize) < SHAPER_NAMES.len()
            && (2..SHAPER_NAMES.len() as u8).contains(&self.shaper_ws_mode)
            && (self.shaper_position as usize) < SHAPER_POSITION_NAMES.len()
            && self.shaper_depth <= 127
    }
}
#[derive(Debug, PartialEq)]
pub enum PanelAction {
    SelectTimbre(usize),
    ToggleTimbre(usize),
    StepWaveform(i32),
    StepPrimaryModulation,
    ToggleVoiceGroup,
    StepFilterRouting,
    StepFilter2Type,
    SetFilter2Link(bool),
    StepShaper,
    SetShaperMode(u8),
    SetShaperPosition(u8),
    StepSecondaryWaveform,
    StepSecondaryModulation,
}
#[derive(Default)]
pub struct PanelEvents {
    pub pots: Vec<(usize, usize, u16)>,
    pub actions: Vec<PanelAction>,
    pub notes: Vec<u8>,
}
pub struct Panel {
    data: Value,
    full_size: bool,
    pots: [[u16; 8]; 5],
    section: usize,
    native_waveform: &'static str,
    encoder_remainder: f32,
    native_pad_held: [bool; 16],
    native_pad_previous: [bool; 16],
    native_pad_frame: Option<u64>,
    #[cfg(test)]
    hit_targets: Vec<(String, Rect)>,
    #[cfg(test)]
    target_ids: Vec<(String, egui::Id)>,
    #[cfg(test)]
    active_leds: Vec<(f32, usize)>,
    #[cfg(test)]
    shaper_position_targets: Vec<(u8, Rect)>,
    #[cfg(test)]
    shaper_mode_targets: Vec<(u8, Rect)>,
    #[cfg(test)]
    shaper_mode_viewport: Option<Rect>,
    #[cfg(test)]
    filter2_link_target: Option<Rect>,
}
impl Panel {
    pub fn native_primary_controls(&mut self, values: [u8; 2]) {
        for (mux, value) in [7, 6].into_iter().zip(values) {
            self.pots[0][mux] = (value as u16) << 3;
        }
    }
    pub fn native_secondary_pitch(&mut self, values: [u8; 2]) {
        for (mux, value) in [5, 4].into_iter().zip(values) {
            self.pots[0][mux] = (value as u16) << 3;
        }
    }
    pub fn native_mixer_levels(&mut self, levels: [u8; 3]) {
        for (mux, level) in [3, 2, 1].into_iter().zip(levels) {
            self.pots[0][mux] = (level as u16) << 3;
        }
    }
    pub fn native_amplifier_level(&mut self, level: u8) {
        self.pots[1][2] = (level as u16) << 3;
    }
    pub fn native_pan_position(&mut self, position: u8) {
        self.pots[1][1] = (position as u16) << 3;
    }
    pub fn native_portamento_time(&mut self, value: u8) {
        self.pots[2][7] = (value as u16) << 3;
    }
    pub fn native_lfo_values(&mut self, values: [u8; 2]) {
        self.pots[3][5] = (values[0] as u16) << 3;
        self.pots[3][4] = (values[1] as u16) << 3;
    }
    pub fn native_envelope_values(&mut self, values: [u8; 4]) {
        for ((ch, mux), value) in [(2, 2), (2, 1), (2, 0), (3, 7)].into_iter().zip(values) {
            self.pots[ch][mux] = (value as u16) << 3;
        }
    }
    pub fn native_auxiliary_values(&mut self, values: [u8; 4], intensity: u8, key_tracking: u8) {
        for (mux, value) in [6, 5, 4, 3].into_iter().zip(values) {
            self.pots[2][mux] = (value as u16) << 3;
        }
        self.pots[1][7] = (intensity as u16) << 3;
        self.pots[1][4] = (key_tracking as u16) << 3;
    }
    pub fn native_filter_values(
        &mut self,
        cutoff: u8,
        resonance: u8,
        filter_type: u8,
        waveform: &'static str,
    ) {
        self.native_waveform = waveform;
        self.pots[0][0] = (cutoff as u16) << 3;
        self.pots[1][5] = (resonance as u16) << 3;
        self.pots[4][3] = (filter_type as u16) << 3;
    }
    pub fn new() -> Self {
        Self {
            data: serde_json::from_str(include_str!("panel.json")).unwrap(),
            full_size: false,
            pots: [[512; 8]; 5],
            section: 2,
            native_waveform: "Saw",
            encoder_remainder: 0.0,
            native_pad_held: [false; 16],
            native_pad_previous: [false; 16],
            native_pad_frame: None,
            #[cfg(test)]
            hit_targets: Vec::new(),
            #[cfg(test)]
            target_ids: Vec::new(),
            #[cfg(test)]
            active_leds: Vec::new(),
            #[cfg(test)]
            shaper_position_targets: Vec::new(),
            #[cfg(test)]
            shaper_mode_targets: Vec::new(),
            #[cfg(test)]
            shaper_mode_viewport: None,
            #[cfg(test)]
            filter2_link_target: None,
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        engine: &Engine,
        native_state: Option<NativePanelState>,
    ) -> PanelEvents {
        let native = native_state.is_some();
        let mut events = PanelEvents::default();
        if let Some(state) = native_state {
            self.pots[1][6] = (state.filter2[0] as u16) << 3;
            self.pots[1][3] = (state.filter2[1] as u16) << 3;
            self.pots[1][0] = (state.shaper_depth as u16) << 3;
        }
        if native {
            let frame = ui.ctx().cumulative_frame_nr();
            if self.native_pad_frame != Some(frame) {
                self.native_pad_previous = self.native_pad_held;
                self.native_pad_held = [false; 16];
                self.native_pad_frame = Some(frame);
            }
        } else {
            self.native_pad_held = [false; 16];
            self.native_pad_previous = [false; 16];
            self.native_pad_frame = None;
        }
        #[cfg(test)]
        self.hit_targets.clear();
        #[cfg(test)]
        self.target_ids.clear();
        #[cfg(test)]
        self.active_leds.clear();
        #[cfg(test)]
        self.shaper_position_targets.clear();
        #[cfg(test)]
        self.shaper_mode_targets.clear();
        #[cfg(test)]
        {
            self.shaper_mode_viewport = None;
            self.filter2_link_target = None;
        }
        let available = ui.available_width();
        let narrow = available < 760.0;
        let sections = [
            ("Вся панель", (0.0, 0.0, 1300.0, 580.0)),
            ("Oscillators", (155.0, 0.0, 285.0, 324.0)),
            ("Mixer / Filter / Amp", (365.0, 0.0, 442.0, 350.0)),
            ("Arp / Sequencer", (155.0, 318.0, 632.0, 86.0)),
            (
                if native {
                    "EG 2 (AMP)"
                } else {
                    "EG / LFO / Patch"
                },
                if native {
                    (500.0, 394.0, 283.0, 112.0)
                } else {
                    (155.0, 388.0, 848.0, 96.0)
                },
            ),
            ("Program / FX", (777.0, 0.0, 523.0, 484.0)),
            ("16 Keys", (0.0, 475.0, 1300.0, 100.0)),
        ];
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.full_size, "1:1");
            if narrow {
                egui::ComboBox::from_id_salt("panel-section")
                    .selected_text(sections[self.section].0)
                    .show_ui(ui, |ui| {
                        for (index, (name, _)) in sections.iter().enumerate() {
                            ui.selectable_value(&mut self.section, index, *name);
                        }
                    });
            }
        });
        let (left, top, width, height) = if narrow && !self.full_size {
            sections[self.section].1
        } else {
            sections[0].1
        };
        let scale = if self.full_size {
            1.0
        } else if narrow && self.section != 0 {
            (available / width).clamp(0.75, 1.1)
        } else {
            (available / width).min(1.1)
        };
        let touch_section = narrow && !self.full_size && self.section != 0;
        let (lcd, actual) = {
            let view = engine.view.lock().unwrap();
            (view.lcd.clone(), view.pots)
        };
        egui::ScrollArea::both()
            .max_height(640.0)
            .id_salt("faceplate")
            .show(ui, |ui| {
                let (rect, _) = ui
                    .allocate_exact_size(Vec2::new(width * scale, height * scale), Sense::hover());
                let origin = rect.min + Vec2::new(10.0 - left, 10.0 - top) * scale;
                let at = |x: f32, y: f32| origin + Vec2::new(x, y) * scale;
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, SILVER);
                painter.rect_stroke(
                    rect,
                    4.0,
                    Stroke::new(1.0, Color32::from_rgb(142, 151, 159)),
                    egui::StrokeKind::Inside,
                );
                for (x, y) in [(0.0, 0.0), (1280.0, 0.0), (0.0, 550.0), (1280.0, 550.0)] {
                    painter.circle_filled(at(x, y), 3.0 * scale, INK);
                }
                let path = self.data["path"].as_str().unwrap();
                let mut x = 0f32;
                let mut y = 0f32;
                let mut previous = Pos2::ZERO;
                let mut parts = Vec::new();
                let mut buffer = String::new();
                for c in path.chars() {
                    if c.is_ascii_alphabetic() || c == ' ' {
                        if !buffer.is_empty() {
                            parts.push(buffer.clone());
                            buffer.clear();
                        }
                        if c != ' ' {
                            parts.push(c.to_string());
                        }
                    } else {
                        buffer.push(c);
                    }
                }
                if !buffer.is_empty() {
                    parts.push(buffer);
                }
                let mut i = 0;
                while i < parts.len() {
                    match parts[i].as_str() {
                        "M" => {
                            x = parts[i + 1].parse().unwrap();
                            y = parts[i + 2].parse().unwrap();
                            previous = at(x, y);
                            i += 3;
                        }
                        "V" => {
                            y = parts[i + 1].parse().unwrap();
                            let next = at(x, y);
                            painter.line_segment(
                                [previous, next],
                                Stroke::new(0.8 * scale, Color32::from_gray(152)),
                            );
                            previous = next;
                            i += 2;
                        }
                        "H" => {
                            x = parts[i + 1].parse().unwrap();
                            let next = at(x, y);
                            painter.line_segment(
                                [previous, next],
                                Stroke::new(0.8 * scale, Color32::from_gray(152)),
                            );
                            previous = next;
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
                for group in self.data["groups"].as_array().unwrap() {
                    let group_x = group[1].as_f64().unwrap() as f32;
                    if touch_section && !(left..=left + width).contains(&group_x) {
                        continue;
                    }
                    let y = group[2].as_f64().unwrap() as f32;
                    let y = if y == 390.0 { 399.0 } else { y };
                    if narrow && self.section == 2 && y >= 318.0 {
                        continue;
                    }
                    painter.text(
                        at(group[1].as_f64().unwrap() as f32, y),
                        Align2::CENTER_TOP,
                        group[0].as_str().unwrap(),
                        FontId::proportional(10.0 * scale),
                        INK,
                    );
                }
                for ledlist in self.data["leds"].as_array().unwrap() {
                    let x = ledlist[0].as_f64().unwrap() as f32;
                    let y = ledlist[1].as_f64().unwrap() as f32;
                    for (index, label) in ledlist[2].as_array().unwrap().iter().enumerate() {
                        let p = at(x, y + index as f32 * 14.0);
                        let selected_wave = native_state.is_some_and(|state| match x {
                            168.0 => state.waveform == index,
                            246.0 => state.primary_controls_supported()
                                && ((index == 0 && state.waveform == 5) || (index == 1 && state.waveform == 4)),
                            243.0 => {
                                state.primary_controls_supported()
                                    && state.primary_mode as usize == index
                            }
                            308.0 => (state.secondary & 3) as usize == index,
                            377.0 => state.secondary & (16 << index) != 0,
                            440.0 => state.filter2_supported() && state.filter_route as usize == index,
                            704.0 => state.filter2_supported() && state.filter2_type as usize == index,
                            _ => false,
                        });
                        #[cfg(test)]
                        if selected_wave {
                            self.active_leds.push((x, index));
                        }
                        painter.circle_filled(
                            p,
                            if selected_wave { 2.8 } else { 2.0 } * scale,
                            if selected_wave {
                                RED
                            } else {
                                Color32::from_rgb(119, 109, 116)
                            },
                        );
                        let label = label.as_str().unwrap();
                        if label.chars().any(|c| matches!(c, '╱' | '△' | '∿' | '┌')) {
                            wave_icons(&painter, p + Vec2::new(6.0, -4.0) * scale, label, scale);
                        } else {
                            painter.text(
                                p + Vec2::new(6.0, 0.0) * scale,
                                Align2::LEFT_CENTER,
                                label,
                                FontId::proportional(7.0 * scale),
                                INK,
                            );
                        }
                    }
                }
                let screen = Rect::from_min_size(at(858.0, 38.0), Vec2::new(268.0, 142.0) * scale);
                painter.rect_filled(screen, 3.0, Color32::from_rgb(50, 59, 65));
                let inner = screen.shrink(10.0 * scale);
                painter.rect_filled(inner, 1.0, BLUE);
                let cell = inner.size() / Vec2::new(128.0, 64.0);
                if native {
                    let shaper_title = native_state
                        .filter(|state| state.shaper_supported() && state.shaper_mode != 0)
                        .map(|state| format!(" · {} {}", SHAPER_LCD_NAMES[state.shaper_mode as usize], state.shaper_depth))
                        .unwrap_or_default();
                    let mode_prefix = native_state
                        .filter(|state| state.primary_mode != 0)
                        .and_then(|state| PRIMARY_MODE_NAMES.get(state.primary_mode as usize))
                        .map(|name| format!("{name} · "))
                        .unwrap_or_default();
                    let link_title = native_state
                        .filter(|state| state.filter2_link_supported())
                        .map(|state| format!(" · LNK {}", if state.filter2_link { "ON" } else { "OFF" }))
                        .unwrap_or_default();
                    painter.text(
                        inner.min + Vec2::new(12.0, 14.0) * scale,
                        Align2::LEFT_TOP,
                        format!(
                            "Native VA{}\n{}{} · {}\n{} · {}{}\nF1 Cut {} Res {}\nF2 Cut {} Res {}\nType {}",
                            shaper_title,
                            mode_prefix,
                            self.native_waveform,
                            if native_state.is_none_or(|s| s.voice_mode.polyphonic) { "Poly" } else { "Mono" },
                            native_state.and_then(|s| FILTER_ROUTING_NAMES.get(s.filter_route as usize)).unwrap_or(&"?"),
                            native_state.and_then(|s| FILTER2_NAMES.get(s.filter2_type as usize)).unwrap_or(&"?"),
                            link_title,
                            self.pots[0][0] >> 3,
                            self.pots[1][5] >> 3,
                            self.pots[1][6] >> 3,
                            self.pots[1][3] >> 3,
                            self.pots[4][3] >> 3
                        ),
                        FontId::monospace(13.0 * scale),
                        Color32::from_rgb(24, 51, 78),
                    );
                }
                for y in 0..if native { 0 } else { 64 } {
                    for x in 0..128 {
                        if lcd[y * 128 + x] != 0 {
                            painter.rect_filled(
                                Rect::from_min_size(
                                    inner.min + Vec2::new(x as f32 * cell.x, y as f32 * cell.y),
                                    cell,
                                ),
                                0.0,
                                Color32::from_rgb(24, 51, 78),
                            );
                        }
                    }
                }
                painter.text(
                    at(1232.0, 35.0),
                    Align2::CENTER_TOP,
                    "KORG",
                    FontId::proportional(20.0 * scale),
                    INK,
                );
                painter.text(
                    at(1160.0, 103.0),
                    Align2::CENTER_CENTER,
                    "PROGRAM\nVALUE",
                    FontId::proportional(9.0 * scale),
                    INK,
                );
                for knob in self.data["knobs"].as_array().unwrap() {
                    let x = knob["x"].as_f64().unwrap() as f32;
                    let y = knob["y"].as_f64().unwrap() as f32;
                    let center = at(x, y);
                    let native_supported = native_state
                        .is_some_and(NativePanelState::primary_controls_supported)
                        && matches!(knob["id"].as_str(), Some("osc1-control1" | "osc1-control2"))
                        || native_state.is_some_and(NativePanelState::filter2_supported)
                            && matches!(knob["id"].as_str(), Some("filter2-cutoff" | "filter2-resonance"))
                        || native_state.is_some_and(NativePanelState::shaper_supported)
                            && knob["id"].as_str() == Some("amp-depth")
                        || matches!(
                            knob["id"].as_str(),
                            Some(
                                "filter1-cutoff"
                                    | "filter1-resonance"
                                    | "filter1-type"
                                    | "eg2-attack"
                                    | "eg2-decay"
                                    | "eg2-sustain"
                                    | "eg2-release"
                                    | "eg1-attack"
                                    | "eg1-decay"
                                    | "eg1-sustain"
                                    | "eg1-release"
                                    | "filter-eg1"
                                    | "filter-keytrack"
                                    | "amp-level"
                                    | "amp-pan"
                                    | "mixer-osc1"
                                    | "mixer-osc2"
                                    | "mixer-noise"
                                    | "osc2-semitone"
                                    | "osc2-tune"
                                    | "lfo1-frequency"
                                    | "lfo2-frequency"
                                    | "portamento"
                            )
                        );
                    let analog =
                        knob["analog"].as_bool().unwrap_or(false) || (native && !native_supported);
                    if narrow && native && self.section == 4 && !native_supported {
                        continue;
                    }
                    let hit = Rect::from_center_size(center, Vec2::splat(if touch_section { (48.0 * scale).max(44.0) } else { 48.0 * scale }));
                    if !rect.intersects(hit) {
                        continue;
                    }
                    #[cfg(test)]
                    {
                        let id = knob["id"].as_str().unwrap();
                        self.hit_targets.push((id.to_owned(), hit));
                        self.target_ids.push((id.to_owned(), ui.id().with(id)));
                    }
                    let control_name = format!(
                        "{} {}",
                        knob["section"].as_str().unwrap_or(""),
                        knob["label"].as_str().unwrap_or("")
                    );
                    let (ch, mux) = if analog {
                        (0, 0)
                    } else {
                        (
                            knob["channel"].as_u64().unwrap() as usize,
                            knob["mux"].as_u64().unwrap() as usize,
                        )
                    };
                    if !native
                        && !analog
                        && !ui
                            .ctx()
                            .is_being_dragged(ui.id().with(knob["id"].as_str().unwrap()))
                    {
                        self.pots[ch][mux] = actual[ch][mux];
                    }
                    let tempo_lfo = native_state.is_some_and(|state| match (ch, mux) {
                        (3, 5) => state.lfo_tempo[0],
                        (3, 4) => state.lfo_tempo[1],
                        _ => false,
                    });
                    let maximum = if tempo_lfo {
                        128
                    } else if native {
                        1016
                    } else {
                        1023
                    };
                    let default = if tempo_lfo {
                        8 << 3
                    } else {
                        match knob["id"].as_str().unwrap() {
                            "filter1-resonance" => 48 << 3,
                            "filter1-type" => 32 << 3,
                            "filter2-cutoff" => 127 << 3,
                            "filter2-resonance" => 0,
                            "amp-depth" => 0,
                            "mixer-noise" => 0,
                            "eg2-attack" | "eg2-decay" => 0,
                            "eg2-sustain" => 127 << 3,
                            "eg2-release" => 10 << 3,
                            "amp-level" => 100 << 3,
                            "mixer-osc1" => 127 << 3,
                            "mixer-osc2" => 0,
                            "osc1-control1" | "osc1-control2" => 0,
                            "lfo1-frequency" | "lfo2-frequency" => 45 << 3,
                            _ => 512,
                        }
                    };
                    let knob_change = controls::knob(
                        ui,
                        ui.id().with(knob["id"].as_str().unwrap()),
                        hit,
                        controls::KnobSettings {
                            incoming: if analog { 512 } else { self.pots[ch][mux] },
                            default,
                            native,
                            enabled: !analog,
                            label: &control_name,
                            maximum: Some(maximum),
                        },
                    );
                    let response = knob_change.response;
                    let value = knob_change.value;
                    if knob_change.changed {
                        self.pots[ch][mux] = value;
                        if !native {
                            engine.send(Command::Pot(ch, mux, value));
                        }
                        events.pots.push((ch, mux, value));
                    }
                    for tick in 0..17 {
                        let angle = (-135.0 + 270.0 * tick as f32 / 16.0).to_radians();
                        let direction = Vec2::new(angle.sin(), -angle.cos());
                        painter.line_segment(
                            [
                                center + direction * 27.0 * scale,
                                center + direction * 31.0 * scale,
                            ],
                            Stroke::new(0.7 * scale, INK),
                        );
                    }
                    painter.circle_filled(
                        center + Vec2::new(0.0, 2.0) * scale,
                        23.0 * scale,
                        Color32::from_gray(125),
                    );
                    painter.circle_filled(center, 22.0 * scale, Color32::from_rgb(36, 39, 43));
                    painter.circle_filled(center, 16.0 * scale, Color32::from_rgb(55, 59, 64));
                    let angle = (-135.0 + 270.0 * value as f32 / maximum as f32).to_radians();
                    painter.line_segment(
                        [
                            center + Vec2::new(angle.sin(), -angle.cos()) * 7.0 * scale,
                            center + Vec2::new(angle.sin(), -angle.cos()) * 18.0 * scale,
                        ],
                        Stroke::new(
                            2.0 * scale,
                            if analog {
                                Color32::from_gray(125)
                            } else {
                                Color32::from_gray(238)
                            },
                        ),
                    );
                    if !analog {
                        painter.rect_filled(
                            Rect::from_center_size(
                                center + Vec2::new(0.0, 6.0) * scale,
                                Vec2::new(23.0, 12.0) * scale,
                            ),
                            2.0 * scale,
                            Color32::from_rgb(36, 39, 43),
                        );
                        painter.text(
                            center + Vec2::new(0.0, 6.0) * scale,
                            Align2::CENTER_CENTER,
                            if native {
                                if tempo_lfo {
                                    crate::native::TEMPO_DIVISION_LABELS
                                        [((value >> 3) as usize).min(16)]
                                    .to_owned()
                                } else {
                                    (value >> 3).to_string()
                                }
                            } else {
                                value.to_string()
                            },
                            FontId::monospace(9.0 * scale),
                            Color32::from_gray(238),
                        );
                    }
                    if response.hovered() || response.has_focus() {
                        painter.circle_stroke(
                            center,
                            24.0 * scale,
                            Stroke::new(1.5 * scale, Color32::from_rgb(62, 99, 128)),
                        );
                    }
                    let label_y = if y == 443.0 {
                        y + 27.0
                    } else if y < 80.0 {
                        y - 34.0
                    } else {
                        y - 37.0
                    };
                    painter.text(
                        at(x, label_y),
                        if y == 443.0 {
                            Align2::CENTER_TOP
                        } else {
                            Align2::CENTER_BOTTOM
                        },
                        knob["label"].as_str().unwrap(),
                        FontId::proportional(8.0 * scale),
                        INK,
                    );
                }
                for (step, collection) in [(false, "switches"), (true, "stepKeys")] {
                    if narrow && native && self.section == 4 {
                        continue;
                    }
                    for control in self.data[collection].as_array().unwrap() {
                        let x = control["x"].as_f64().unwrap() as f32;
                        let y = control["y"].as_f64().unwrap() as f32;
                        let center = at(x, y);
                        let mut hit_size = Vec2::new(62.0, 44.0) * scale;
                        if touch_section { hit_size = hit_size.max(Vec2::splat(44.0)); }
                        let hit = Rect::from_center_size(center, hit_size);
                        if !rect.intersects(hit) {
                            continue;
                        }
                        let id = control["id"].as_str().unwrap();
                        let timbre_index = id
                            .strip_prefix("timbre")
                            .and_then(|n| n.parse::<usize>().ok())
                            .map(|n| n - 1);
                        let waveform = matches!(id, "osc1-wave-up" | "osc1-wave-down");
                        let primary_mod = id == "osc1-mod"
                            && native_state
                                .is_some_and(NativePanelState::primary_controls_supported);
                        let secondary = matches!(id, "osc2-wave" | "osc2-mod");
                        let voice_group = id == "unison" && native_state.is_some();
                        let routing = id == "filter-routing" && native_state.is_some_and(NativePanelState::filter2_supported);
                        let filter2_type = id == "filter2-type" && native_state.is_some_and(NativePanelState::filter2_supported);
                        let shaper = id == "amp-drive" && native_state.is_some_and(NativePanelState::shaper_supported);
                        let native_pad = native && step;
                        let supported = !native
                            || timbre_index.is_some()
                            || waveform
                            || primary_mod
                            || routing
                            || filter2_type
                            || shaper
                            || secondary
                            || voice_group
                            || native_pad;
                        let button_hit = if native && timbre_index.is_some() {
                            Rect::from_center_size(
                                center - Vec2::new(0.0, 3.0) * scale,
                                Vec2::new(62.0, 27.0) * scale,
                            )
                        } else {
                            hit
                        };
                        let response = ui.interact(
                            button_hit,
                            ui.id().with(id),
                            if native_pad {
                                Sense::click_and_drag()
                            } else if supported {
                                Sense::click()
                            } else {
                                Sense::hover()
                            },
                        );
                        #[cfg(test)]
                        self.hit_targets.push((id.to_owned(), button_hit));
                        #[cfg(test)]
                        self.target_ids.push((id.to_owned(), response.id));
                        let pad_index =
                            native_pad.then(|| control["number"].as_u64().unwrap() as usize - 1);
                        let pad_active = pad_index.is_some_and(|index| {
                            controls::note_key_active(&response, self.native_pad_previous[index])
                        });
                        if let Some(index) = pad_index {
                            self.native_pad_held[index] = pad_active;
                            if pad_active {
                                events.notes.push(60 + index as u8);
                            }
                        }
                        let selected = pad_active
                            || native_state.is_some_and(|state| {
                                timbre_index == Some(state.selected)
                                    || (primary_mod && state.primary_mode != 0)
                                    || (voice_group && state.voice_group.raw & 128 != 0)
                                    || (routing && state.filter_route != 0)
                                    || (filter2_type && state.filter2_type != 0)
                                    || (shaper && state.shaper_mode != 0)
                            });
                        let label = control["label"]
                            .as_str()
                            .unwrap()
                            .replace('▲', "UP")
                            .replace('▼', "DOWN")
                            .replace('◀', "LEFT")
                            .replace('▶', "RIGHT");
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                ui.is_enabled() && supported,
                                selected,
                                if primary_mod {
                                    format!(
                                        "OSC1 MOD {}",
                                        PRIMARY_MODE_NAMES
                                            [native_state.unwrap().primary_mode as usize]
                                            .to_uppercase()
                                    )
                                } else if routing {
                                    format!("FILTER ROUTING {}", FILTER_ROUTING_NAMES[native_state.unwrap().filter_route as usize].to_uppercase())
                                } else if filter2_type {
                                    format!("FILTER2 TYPE {}", FILTER2_NAMES[native_state.unwrap().filter2_type as usize])
                                } else if shaper {
                                    format!("DRIVE / WS {}", SHAPER_NAMES[native_state.unwrap().shaper_mode as usize].to_uppercase())
                                } else {
                                    id.replace('-', " ").to_uppercase()
                                },
                            )
                        });
                        let face = Rect::from_center_size(
                            center,
                            Vec2::new(
                                if step { 43.0 } else { 38.0 },
                                if step { 30.0 } else { 20.0 },
                            ) * scale,
                        );
                        let inside_label = if shaper {
                            Some(match native_state.unwrap().shaper_mode { 0 => "OFF", 1 => "DRIVE", 2 => "CLIP", _ => "WS" })
                        } else if timbre_index.is_some() || label == "ON" {
                            Some(label.as_str())
                        } else {
                            None
                        };
                        controls::switch_face(
                            &painter,
                            face,
                            &response,
                            scale,
                            controls::SwitchStyle {
                                enabled: supported,
                                selected,
                                step,
                                label: inside_label,
                            },
                        );
                        if response.clicked() {
                            if native {
                                if let Some(index) = timbre_index {
                                    events.actions.push(PanelAction::SelectTimbre(index));
                                } else if waveform {
                                    events.actions.push(PanelAction::StepWaveform(
                                        if id.ends_with("up") { 1 } else { -1 },
                                    ));
                                } else if id == "osc2-wave" {
                                    events.actions.push(PanelAction::StepSecondaryWaveform);
                                } else if id == "osc2-mod" {
                                    events.actions.push(PanelAction::StepSecondaryModulation);
                                } else if primary_mod {
                                    events.actions.push(PanelAction::StepPrimaryModulation);
                                } else if voice_group {
                                    events.actions.push(PanelAction::ToggleVoiceGroup);
                                } else if routing {
                                    events.actions.push(PanelAction::StepFilterRouting);
                                } else if filter2_type {
                                    events.actions.push(PanelAction::StepFilter2Type);
                                } else if shaper {
                                    events.actions.push(PanelAction::StepShaper);
                                }
                            } else {
                                engine.send(Command::Press(
                                    control["row"].as_u64().unwrap() as usize,
                                    control["column"].as_u64().unwrap() as u8,
                                ));
                            }
                        }
                        if shaper {
                            let state = native_state.unwrap();
                            response.clone().on_hover_text(format!("{} · {}\nWS: {}\nПравый клик: тип и положение", SHAPER_NAMES[state.shaper_mode as usize], SHAPER_POSITION_NAMES[state.shaper_position as usize], SHAPER_NAMES[state.shaper_ws_mode as usize]));
                            response.context_menu(|ui| {
                                let state = native_state.unwrap();
                                for (position, label) in SHAPER_POSITION_NAMES.iter().enumerate() {
                                    let option = ui.add_sized([144.0, if narrow { 44.0 } else { 28.0 }], egui::Button::selectable(state.shaper_position as usize == position, *label));
                                    #[cfg(test)]
                                    self.shaper_position_targets.push((position as u8, option.rect));
                                    if option.clicked() {
                                        events.actions.push(PanelAction::SetShaperPosition(position as u8));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                let ws_menu = egui::ScrollArea::vertical().id_salt(("shaper-modes", state.selected)).max_height(330.0).show(ui, |ui| {
                                    for (mode, label) in SHAPER_NAMES.iter().enumerate().skip(2) {
                                        let option = ui.add_sized([144.0, if narrow { 44.0 } else { 28.0 }], egui::Button::selectable(state.shaper_ws_mode as usize == mode, *label));
                                        #[cfg(test)]
                                        self.shaper_mode_targets.push((mode as u8, option.rect));
                                        if option.clicked() {
                                            events.actions.push(PanelAction::SetShaperMode(mode as u8));
                                            ui.close();
                                        }
                                    }
                                });
                                #[cfg(test)]
                                { self.shaper_mode_viewport = Some(ws_menu.inner_rect); }
                                #[cfg(not(test))]
                                let _ = ws_menu;
                            });
                        }
                        if filter2_type {
                            let state = native_state.unwrap();
                            response.clone().on_hover_text("Правый клик: LINK");
                            response.context_menu(|ui| {
                                let menu_width = (ui.ctx().content_rect().right() - ui.cursor().left() - 6.0).clamp(44.0, 144.0);
                                ui.set_min_width(menu_width);
                                ui.set_max_width(menu_width);
                                let enabled = state.filter2_link_supported();
                                let active = enabled && state.filter2_link;
                                let option = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized([menu_width, if narrow { 44.0 } else { 28.0 }],
                                        egui::Button::new(if active { "LINK ON" } else { "LINK OFF" })
                                            .selected(active)
                                            .fill(if active { RED } else { Color32::from_gray(38) }))
                                }).inner;
                                #[cfg(test)]
                                { self.filter2_link_target = Some(option.rect); }
                                option.clone().on_hover_text("LINK использует Cutoff и Resonance Filter 1 для Comb");
                                if option.clicked() {
                                    events.actions.push(PanelAction::SetFilter2Link(!state.filter2_link));
                                    ui.close();
                                }
                            });
                        }
                        if !supported {
                            response
                                .clone()
                                .on_hover_cursor(egui::CursorIcon::NotAllowed)
                                .on_hover_text(format!(
                                    "{} · функция пока не перенесена",
                                    id.replace('-', " ")
                                ));
                        }
                        if let (Some(state), Some(index)) = (native_state, timbre_index) {
                            let power = Rect::from_center_size(
                                center + Vec2::new(0.0, 17.0) * scale,
                                Vec2::new(38.0, 14.0) * scale,
                            );
                            let power_response =
                                ui.interact(power, ui.id().with((id, "power")), Sense::click());
                            #[cfg(test)]
                            self.hit_targets.push((format!("{id}-power"), power));
                            power_response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Checkbox,
                                    true,
                                    state.enabled[index],
                                    format!("Timbre {} power", index + 1),
                                )
                            });
                            painter.rect_filled(
                                power,
                                2.0 * scale,
                                if state.enabled[index] {
                                    RED
                                } else {
                                    Color32::from_gray(79)
                                },
                            );
                            painter.text(
                                power.center(),
                                Align2::CENTER_CENTER,
                                if state.enabled[index] { "ON" } else { "OFF" },
                                FontId::proportional(8.0 * scale),
                                Color32::WHITE,
                            );
                            if power_response.clicked() {
                                events.actions.push(PanelAction::ToggleTimbre(index));
                            }
                        } else if (inside_label.is_none() || shaper) && !native_pad {
                            painter.text(
                                center + Vec2::new(0.0, if step { 27.0 } else { -13.0 }) * scale,
                                if step {
                                    Align2::CENTER_TOP
                                } else {
                                    Align2::CENTER_BOTTOM
                                },
                                label,
                                FontId::proportional(if step { 6.5 * scale } else { 8.0 * scale }),
                                if supported {
                                    INK
                                } else {
                                    Color32::from_rgb(91, 99, 107)
                                },
                            );
                        }
                        if step {
                            painter.text(
                                center,
                                Align2::CENTER_CENTER,
                                control["number"].as_u64().unwrap().to_string(),
                                FontId::monospace(12.0 * scale),
                                Color32::WHITE,
                            );
                        }
                    }
                }
                let center = at(1232.0, 127.0);
                let response = ui.interact(
                    Rect::from_center_size(center, Vec2::splat(90.0 * scale)),
                    ui.id().with("program-wheel"),
                    if native {
                        Sense::hover()
                    } else {
                        Sense::drag()
                    },
                );
                painter.circle_filled(center, 42.0 * scale, Color32::from_gray(45));
                painter.circle_stroke(
                    center,
                    31.0 * scale,
                    Stroke::new(2.0 * scale, Color32::from_gray(76)),
                );
                if !native {
                    if response.dragged() {
                        self.encoder_remainder -= response.drag_delta().y / 8.0;
                    }
                    if response.hovered() {
                        self.encoder_remainder += ui.input_mut(|i| {
                            let delta = i.smooth_scroll_delta.y;
                            i.smooth_scroll_delta.y = 0.0;
                            delta
                        }) / 8.0;
                    }
                    let steps = self.encoder_remainder.trunc() as i32;
                    if steps != 0 {
                        let emitted = steps.clamp(-8, 8);
                        engine.send(Command::Encoder(emitted));
                        self.encoder_remainder -= emitted as f32;
                    }
                }
            });
        events
    }
}
fn wave_icons(painter: &egui::Painter, origin: Pos2, text: &str, scale: f32) {
    let mut cursor = origin;
    for token in text.split_whitespace() {
        let points: Vec<(f32, f32)> = if token.contains('╱') {
            vec![(0.0, 7.0), (11.0, 0.0), (11.0, 7.0)]
        } else if token.contains('△') {
            vec![(0.0, 7.0), (5.5, 0.0), (11.0, 7.0)]
        } else if token.contains('┌') {
            vec![
                (0.0, 7.0),
                (3.0, 7.0),
                (3.0, 0.0),
                (8.0, 0.0),
                (8.0, 7.0),
                (11.0, 7.0),
            ]
        } else if token.contains('∿') {
            (0..=12)
                .map(|i| {
                    (
                        i as f32,
                        3.5 - (i as f32 / 12.0 * std::f32::consts::TAU).sin() * 3.5,
                    )
                })
                .collect()
        } else {
            painter.text(
                cursor,
                Align2::LEFT_TOP,
                token,
                FontId::proportional(7.0 * scale),
                INK,
            );
            cursor.x += 17.0 * scale;
            continue;
        };
        painter.add(egui::Shape::line(
            points
                .into_iter()
                .map(|(x, y)| cursor + Vec2::new(x, y) * scale)
                .collect(),
            Stroke::new(0.8 * scale, INK),
        ));
        cursor.x += 17.0 * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::{NativePanelState, Panel, PanelAction, PanelEvents};
    use crate::engine::Engine;
    use eframe::egui;

    fn frame(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        input: Vec<egui::Event>,
    ) -> PanelEvents {
        frame_focused(ctx, panel, engine, input, true)
    }
    fn frame_focused(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        input: Vec<egui::Event>,
        focused: bool,
    ) -> PanelEvents {
        frame_native(
            ctx,
            panel,
            engine,
            egui::RawInput {
                events: input,
                focused,
                ..Default::default()
            },
            NativePanelState {
                voice_mode: Default::default(),
                voice_group: Default::default(),
                selected: 0,
                enabled: [true, false, false, false],
                waveform: 0,
                primary_mode: 0,
                filter_route: 0,
                filter2_type: 0,
                filter2: [127, 0],
                filter2_link: false,
                shaper_mode: 0,
                shaper_ws_mode: 2,
                shaper_position: 0,
                shaper_depth: 0,
                secondary: 0,
                lfo_tempo: [false, false],
            },
        )
    }
    fn frame_native(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        mut input: egui::RawInput,
        native: NativePanelState,
    ) -> PanelEvents {
        let mut result = PanelEvents::default();
        input.screen_rect.get_or_insert_with(|| {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 1000.0))
        });
        let output = ctx.run_ui(input, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("application")
                .show(ui, |ui| {
                    result = panel.show(ui, engine, Some(native));
                });
        });
        output.drop_without_applying_deltas();
        result
    }

    fn click(ctx: &egui::Context, panel: &mut Panel, engine: &Engine, id: &str) -> PanelEvents {
        frame(ctx, panel, engine, vec![]);
        let center = panel
            .hit_targets
            .iter()
            .find(|(name, _)| name == id)
            .unwrap()
            .1
            .center();
        frame(ctx, panel, engine, vec![egui::Event::PointerMoved(center)]);
        frame(
            ctx,
            panel,
            engine,
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }],
        );
        frame(
            ctx,
            panel,
            engine,
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        )
    }

    fn isolated_engine() -> Engine {
        // Native panel routing is independent of the firmware reference worker.
        // Missing assets stop that worker before it can write flash or artifacts.
        Engine::new(
            std::env::temp_dir().join(format!(
                "radias-ui-no-firmware-assets-{}",
                std::process::id()
            )),
            None,
        )
    }

    fn press_at(center: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn primary_frame(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        values: &mut [u8; 2],
        input: egui::RawInput,
        native: NativePanelState,
    ) -> PanelEvents {
        // Reproduce main.rs -> NativePanel.pot -> next-frame native feedback.
        panel.native_primary_controls(*values);
        let events = frame_native(ctx, panel, engine, input, native);
        for &(channel, mux, value) in &events.pots {
            if channel == 0 && matches!(mux, 7 | 6) {
                values[7 - mux] = (value >> 3) as u8;
            }
        }
        events
    }

    fn primary_state(waveform: usize, selected: usize) -> NativePanelState {
        NativePanelState {
            voice_mode: Default::default(),
            voice_group: Default::default(),
            selected,
            enabled: [true, true, false, false],
            waveform,
            primary_mode: 0,
            filter_route: 0,
            filter2_type: 0,
            filter2: [127, 0],
            filter2_link: false,
            shaper_mode: 0,
            shaper_ws_mode: 2,
            shaper_position: 0,
            shaper_depth: 0,
            secondary: 0,
            lfo_tempo: [false, false],
        }
    }

    fn primary_mode_state(waveform: usize, selected: usize, primary_mode: u8) -> NativePanelState {
        NativePanelState {
            primary_mode,
            ..primary_state(waveform, selected)
        }
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            events,
            ..Default::default()
        }
    }

    fn set_knob(panel: &Panel, name: &str, value: f64) -> egui::Event {
        let id = panel.target_ids.iter().find(|(n, _)| n == name).unwrap().1;
        egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::SetValue,
            target_node: id.accesskit_id(),
            target_tree: egui::accesskit::TreeId::ROOT,
            data: Some(egui::accesskit::ActionData::NumericValue(value)),
        })
    }

    #[test]
    fn primary_control_accessibility_routes_both_pots_and_switches_timbre_feedback() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut first = [0, 0];
        primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut first,
            input(vec![]),
            primary_state(0, 0),
        );
        let request = vec![
            set_knob(&panel, "osc1-control1", 37.0),
            set_knob(&panel, "osc1-control2", 91.0),
        ];
        let changed = primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut first,
            input(request),
            primary_state(0, 0),
        );
        assert_eq!(first, [37, 91]);
        assert_eq!(changed.pots, vec![(0, 7, 37 << 3), (0, 6, 91 << 3)]);
        assert!(
            primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut first,
                input(vec![]),
                primary_state(0, 0),
            )
            .pots
            .is_empty()
        );

        let mut second = [12, 88];
        assert!(
            primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut second,
                input(vec![]),
                primary_state(1, 1),
            )
            .pots
            .is_empty(),
            "selecting a timbre must not replay the old UI value"
        );
        assert_eq!([panel.pots[0][7], panel.pots[0][6]], [12 << 3, 88 << 3]);
        let request = vec![set_knob(&panel, "osc1-control2", 90.0)];
        let changed = primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut second,
            input(request),
            primary_state(1, 1),
        );
        assert_eq!(changed.pots, vec![(0, 6, 90 << 3)]);
        assert_eq!(second, [12, 90]);

        for waveform in [2, 3] {
            assert!(
                primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut second,
                    input(vec![]),
                    primary_state(waveform, 1),
                )
                .pots
                .is_empty(),
                "changing waveform must not replay an old control value"
            );
            let values = [13 + waveform as u8, 89 - waveform as u8];
            let request = vec![
                set_knob(&panel, "osc1-control1", values[0] as f64),
                set_knob(&panel, "osc1-control2", values[1] as f64),
            ];
            let changed = primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut second,
                input(request),
                primary_state(waveform, 1),
            );
            assert_eq!(
                changed.pots,
                vec![
                    (0, 7, (values[0] as u16) << 3),
                    (0, 6, (values[1] as u16) << 3)
                ]
            );
            assert_eq!(second, values);
        }
        for waveform in [6, 7, 16] {
            primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut second,
                input(vec![]),
                primary_state(waveform, 1),
            );
            let request = vec![
                set_knob(&panel, "osc1-control1", 127.0),
                set_knob(&panel, "osc1-control2", 127.0),
            ];
            assert!(
                primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut second,
                    input(request),
                    primary_state(waveform, 1),
                )
                .pots
                .is_empty(),
                "unsupported waveform {waveform} must remain disabled"
            );
            assert_eq!(second, [16, 86]);
        }
        assert!(
            primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut first,
                input(vec![]),
                primary_state(0, 0),
            )
            .pots
            .is_empty()
        );
        assert_eq!([panel.pots[0][7], panel.pots[0][6]], [37 << 3, 91 << 3]);
    }

    #[test]
    fn primary_controls_capture_fine_drag_with_quantized_feedback() {
        let engine = isolated_engine();
        for (name, index) in [("osc1-control1", 0), ("osc1-control2", 1)] {
            for waveform in 0..4 {
                for mode in 0..4 {
                    for (shift, engaged, small, outside) in
                        [(false, 70, 84, 120), (true, 65, 66, 70)]
                    {
                        let ctx = egui::Context::default();
                        let mut panel = Panel::new();
                        let mut values = [64, 64];
                        primary_frame(
                            &ctx,
                            &mut panel,
                            &engine,
                            &mut values,
                            input(vec![]),
                            primary_mode_state(waveform, 0, mode),
                        );
                        let center = panel
                            .hit_targets
                            .iter()
                            .find(|(n, _)| n == name)
                            .unwrap()
                            .1
                            .center();
                        let mut render = |events: Vec<egui::Event>| {
                            let mut raw = input(events);
                            raw.events.insert(
                                0,
                                egui::Event::ModifiersChanged(egui::Modifiers {
                                    shift,
                                    ..Default::default()
                                }),
                            );
                            primary_frame(
                                &ctx,
                                &mut panel,
                                &engine,
                                &mut values,
                                raw,
                                primary_mode_state(waveform, 0, mode),
                            );
                            values[index]
                        };
                        assert_eq!(
                            render(vec![
                                egui::Event::PointerMoved(center),
                                press_at(center, true)
                            ]),
                            64
                        );
                        assert_eq!(
                            render(vec![egui::Event::PointerMoved(
                                center - egui::vec2(0.0, 8.0)
                            )]),
                            engaged
                        );
                        for step in 1..=80 {
                            render(vec![egui::Event::PointerMoved(
                                center - egui::vec2(0.0, 8.0 + step as f32 * 0.25),
                            )]);
                        }
                        assert_eq!(
                            render(vec![]),
                            small,
                            "small motion must survive every native feedback frame"
                        );
                        let released = center - egui::vec2(0.0, 80.0);
                        assert_eq!(render(vec![egui::Event::PointerMoved(released)]), outside);
                        assert_eq!(render(vec![press_at(released, false)]), outside);
                        assert_eq!(
                            render(vec![egui::Event::PointerMoved(
                                center - egui::vec2(0.0, 100.0)
                            )]),
                            outside
                        );
                        assert_eq!(
                            values[1 - index],
                            64,
                            "drag must only edit the targeted CONTROL"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn primary_controls_consume_wheel_before_the_panel_scrolls() {
        let engine = isolated_engine();
        for (name, index) in [("osc1-control1", 0), ("osc1-control2", 1)] {
            for waveform in 0..4 {
                for mode in 0..4 {
                    let ctx = egui::Context::default();
                    let mut panel = Panel::new();
                    let mut values = [64, 64];
                    primary_frame(
                        &ctx,
                        &mut panel,
                        &engine,
                        &mut values,
                        input(vec![]),
                        primary_mode_state(waveform, 0, mode),
                    );
                    let center = panel
                        .hit_targets
                        .iter()
                        .find(|(n, _)| n == name)
                        .unwrap()
                        .1
                        .center();
                    let mut render = |events| {
                        primary_frame(
                            &ctx,
                            &mut panel,
                            &engine,
                            &mut values,
                            input(events),
                            primary_mode_state(waveform, 0, mode),
                        );
                    };
                    render(vec![egui::Event::PointerMoved(center)]);
                    render(vec![egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, 32.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::default(),
                    }]);
                    for _ in 0..32 {
                        render(vec![]);
                    }
                    assert_eq!(values[index], 68);
                    assert_eq!(values[1 - index], 64);
                    assert_eq!(ctx.input(|i| i.smooth_scroll_delta.y), 0.0);
                    assert_eq!(
                        panel
                            .hit_targets
                            .iter()
                            .find(|(n, _)| n == name)
                            .unwrap()
                            .1
                            .center(),
                        center
                    );
                }
            }
        }
    }

    fn route_primary_frame(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        events: Vec<egui::Event>,
    ) -> PanelEvents {
        route_primary_input(ctx, panel, engine, native, input(events))
    }

    fn route_primary_input(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        input: egui::RawInput,
    ) -> PanelEvents {
        panel.native_primary_controls(native.primary_controls());
        panel.native_mixer_levels(native.mixer_levels());
        let (auxiliary, intensity, tracking) = native.auxiliary_values();
        panel.native_auxiliary_values(auxiliary, intensity, tracking);
        let (cutoff, resonance, filter_type) = native.filter_values();
        panel.native_filter_values(cutoff, resonance, filter_type, native.waveform_name());
        let result = frame_native(ctx, panel, engine, input, native.panel_state());
        for &(ch, mux, value) in &result.pots {
            native.pot(ch, mux, value);
        }
        for action in &result.actions {
            match *action {
                PanelAction::StepPrimaryModulation => native.step_primary_modulation(),
                PanelAction::ToggleVoiceGroup => native.toggle_voice_group(),
                PanelAction::StepWaveform(direction) => native.step_waveform(direction),
                PanelAction::SelectTimbre(index) => native.select_timbre(index),
                PanelAction::StepFilterRouting => native.step_filter_routing(),
                PanelAction::StepFilter2Type => native.step_filter2_type(),
                PanelAction::SetFilter2Link(linked) => native.set_filter2_link(linked),
                PanelAction::StepShaper => native.step_shaper(),
                PanelAction::SetShaperMode(mode) => native.set_shaper_mode(mode),
                PanelAction::SetShaperPosition(position) => native.set_shaper_position(position),
                _ => panic!("unexpected primary interaction {action:?}"),
            }
        }
        result
    }

    fn click_primary(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        name: &str,
    ) -> PanelEvents {
        route_primary_frame(ctx, panel, engine, native, vec![]);
        let center = panel
            .hit_targets
            .iter()
            .find(|(n, _)| n == name)
            .unwrap()
            .1
            .center();
        route_primary_frame(
            ctx,
            panel,
            engine,
            native,
            vec![egui::Event::PointerMoved(center), press_at(center, true)],
        );
        route_primary_frame(ctx, panel, engine, native, vec![press_at(center, false)])
    }

    fn assert_primary_led(panel: &Panel, mode: usize) {
        assert_eq!(
            panel
                .active_leds
                .iter()
                .filter(|(x, _)| *x == 243.0)
                .copied()
                .collect::<Vec<_>>(),
            vec![(243.0, mode)]
        );
    }

    #[test]
    fn filter_routes_types_and_knobs_preserve_timbre_feedback() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().filter2, [127, 0]);
            for route in 0..4 {
                assert_eq!(native.panel_state().filter_route, route);
                for output in 0..super::FILTER2_NAMES.len() as u8 {
                    assert_eq!(native.panel_state().filter2_type, output);
                    let values = [31 + timbre as u8 * 8 + route * 3 + output, 17 + output];
                    let previous = native.panel_state().filter2;
                    let requested = vec![
                        set_knob(&panel, "filter2-cutoff", values[0] as f64),
                        set_knob(&panel, "filter2-resonance", values[1] as f64),
                    ];
                    let changed =
                        route_primary_frame(&ctx, &mut panel, &engine, &mut native, requested);
                    assert_eq!(
                        changed.pots,
                        [6, 3]
                            .into_iter()
                            .zip(values)
                            .zip(previous)
                            .filter(|((_, next), old)| next != old)
                            .map(|((mux, value), _)| (1, mux, (value as u16) << 3))
                            .collect::<Vec<_>>()
                    );
                    let stable =
                        route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                    assert!(
                        stable.pots.is_empty(),
                        "native feedback must not replay parameters"
                    );
                    assert_eq!(native.panel_state().filter2, values);
                    assert_eq!(
                        [panel.pots[1][6] >> 3, panel.pots[1][3] >> 3],
                        [values[0] as u16, values[1] as u16]
                    );
                    assert_eq!(
                        panel
                            .active_leds
                            .iter()
                            .filter(|(x, _)| *x == 440.0)
                            .copied()
                            .collect::<Vec<_>>(),
                        vec![(440.0, route as usize)]
                    );
                    assert_eq!(
                        panel
                            .active_leds
                            .iter()
                            .filter(|(x, _)| *x == 704.0)
                            .copied()
                            .collect::<Vec<_>>(),
                        vec![(704.0, output as usize)]
                    );
                    assert_eq!(
                        click_primary(&ctx, &mut panel, &engine, &mut native, "filter2-type")
                            .actions,
                        vec![PanelAction::StepFilter2Type]
                    );
                }
                assert_eq!(
                    native.panel_state().filter2_type,
                    0,
                    "type cycle includes Comb and returns to LPF"
                );
                assert_eq!(
                    click_primary(&ctx, &mut panel, &engine, &mut native, "filter-routing").actions,
                    vec![PanelAction::StepFilterRouting]
                );
            }
            assert_eq!(native.panel_state().filter_route, 0);
            assert_eq!(
                native.filter_values(),
                (64, 48, 32),
                "F2 controls must not edit F1"
            );
            native.step_filter_routing();
            native.step_filter2_type();
            assert!(
                click_primary(&ctx, &mut panel, &engine, &mut native, "filter-select")
                    .actions
                    .is_empty(),
                "Physical SELECT is still unavailable"
            );
        }
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            let feedback = route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert!(feedback.pots.is_empty());
            assert_eq!(native.panel_state().filter_route, 1);
            assert_eq!(native.panel_state().filter2_type, 1);
            assert_eq!(native.panel_state().filter2, [43 + timbre as u8 * 8, 20]);
        }
    }

    #[test]
    fn filter2_pointer_drag_and_wheel_keep_fractional_feedback_on_all_routes() {
        let engine = isolated_engine();
        for (name, index, mux) in [("filter2-cutoff", 0, 6), ("filter2-resonance", 1, 3)] {
            for route in 0..4 {
                let ctx = egui::Context::default();
                let mut panel = Panel::new();
                let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
                native.enabled = true;
                for _ in 0..route {
                    native.step_filter_routing();
                }
                native.pot(1, 6, 64 << 3);
                native.pot(1, 3, 64 << 3);
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                let center = panel
                    .hit_targets
                    .iter()
                    .find(|(n, _)| n == name)
                    .unwrap()
                    .1
                    .center();
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![egui::Event::PointerMoved(center), press_at(center, true)],
                );
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![egui::Event::PointerMoved(center - egui::vec2(0.0, 8.0))],
                );
                assert_eq!(native.panel_state().filter2[index], 70);
                for step in 1..=80 {
                    route_primary_frame(
                        &ctx,
                        &mut panel,
                        &engine,
                        &mut native,
                        vec![egui::Event::PointerMoved(
                            center - egui::vec2(0.0, 8.0 + step as f32 * 0.25),
                        )],
                    );
                }
                assert_eq!(native.panel_state().filter2[index], 84);
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![press_at(center - egui::vec2(0.0, 28.0), false)],
                );
                native.pot(1, mux, 64 << 3);
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![egui::Event::PointerMoved(center)],
                );
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, 32.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::default(),
                    }],
                );
                for _ in 0..32 {
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                }
                assert_eq!(native.panel_state().filter2[index], 68);
                assert_eq!(ctx.input(|i| i.smooth_scroll_delta.y), 0.0);
                assert_eq!(
                    panel
                        .hit_targets
                        .iter()
                        .find(|(n, _)| n == name)
                        .unwrap()
                        .1
                        .center(),
                    center
                );
                assert_eq!(native.panel_state().filter2[1 - index], 64);
            }
        }
    }

    #[test]
    fn narrow_filter_section_keeps_routing_and_filter2_targets_reachable() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(390.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                panel.show(ui, &engine, Some(primary_state(0, 0)));
            },
        );
        output.drop_without_applying_deltas();
        for name in [
            "filter-routing",
            "filter2-type",
            "filter2-cutoff",
            "filter2-resonance",
            "amp-drive",
            "amp-depth",
        ] {
            let rect = panel.hit_targets.iter().find(|(n, _)| n == name).unwrap().1;
            assert!(
                rect.left() >= 0.0 && rect.right() <= 390.0,
                "{name} must be entirely inside the section"
            );
            assert!(
                rect.width() >= 44.0 && rect.height() >= 44.0,
                "{name} retains a usable touch target"
            );
        }
    }

    #[test]
    fn shaper_button_depth_and_real_position_menu_keep_timbre_state() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().shaper_mode, 0);
            for (mode, depth) in [(1, 35 + timbre as u8 * 8), (2, 71 + timbre as u8 * 8)] {
                assert_eq!(
                    click_primary(&ctx, &mut panel, &engine, &mut native, "amp-drive").actions,
                    vec![PanelAction::StepShaper]
                );
                assert_eq!(native.panel_state().shaper_mode, mode);
                let request = set_knob(&panel, "amp-depth", depth as f64);
                let changed =
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![request]);
                assert_eq!(changed.pots, vec![(1, 0, (depth as u16) << 3)]);
                let stable = route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                assert!(stable.pots.is_empty());
                assert_eq!(panel.pots[1][0] >> 3, depth as u16);
            }
            let center = panel
                .hit_targets
                .iter()
                .find(|(n, _)| n == "amp-drive")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                route_primary_frame(
                    &ctx,
                    &mut panel,
                    &engine,
                    &mut native,
                    vec![
                        egui::Event::PointerMoved(center),
                        egui::Event::PointerButton {
                            pos: center,
                            button: egui::PointerButton::Secondary,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
            }
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            let position = panel
                .shaper_position_targets
                .iter()
                .find(|(p, _)| *p == 1)
                .unwrap()
                .1
                .center();
            route_primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                vec![egui::Event::PointerMoved(position)],
            );
            route_primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                vec![
                    egui::Event::PointerMoved(position),
                    press_at(position, true),
                ],
            );
            let selected = route_primary_frame(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                vec![press_at(position, false)],
            );
            assert_eq!(selected.actions, vec![PanelAction::SetShaperPosition(1)]);
            assert_eq!(native.panel_state().shaper_position, 1);
            assert_eq!(native.panel_state().shaper_mode, 2);
        }
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().shaper_depth, 71 + timbre as u8 * 8);
            assert_eq!(native.panel_state().shaper_position, 1);
            assert_eq!(native.panel_state().shaper_mode, 2);
            click_primary(&ctx, &mut panel, &engine, &mut native, "amp-drive");
            assert_eq!(native.panel_state().shaper_mode, 0);
        }
    }

    fn shaper_menu_frame(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        width: f32,
        events: Vec<egui::Event>,
    ) -> PanelEvents {
        route_primary_input(
            ctx,
            panel,
            engine,
            native,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 844.0),
                )),
                events,
                ..Default::default()
            },
        )
    }

    fn select_real_ws_menu(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        width: f32,
        mode: u8,
    ) -> PanelEvents {
        shaper_menu_frame(ctx, panel, engine, native, width, vec![]);
        let center = panel
            .hit_targets
            .iter()
            .find(|(name, _)| name == "amp-drive")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            shaper_menu_frame(
                ctx,
                panel,
                engine,
                native,
                width,
                vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        shaper_menu_frame(ctx, panel, engine, native, width, vec![]);
        assert_eq!(panel.shaper_mode_targets.len(), 11);
        let rect = panel
            .shaper_mode_targets
            .iter()
            .find(|(value, _)| *value == mode)
            .unwrap()
            .1;
        let viewport = panel.shaper_mode_viewport.unwrap();
        if !viewport.contains(rect.center()) {
            shaper_menu_frame(
                ctx,
                panel,
                engine,
                native,
                width,
                vec![egui::Event::PointerMoved(viewport.center())],
            );
            shaper_menu_frame(
                ctx,
                panel,
                engine,
                native,
                width,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, viewport.center().y - rect.center().y),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::default(),
                }],
            );
            for _ in 0..32 {
                shaper_menu_frame(ctx, panel, engine, native, width, vec![]);
            }
        }
        let rect = panel
            .shaper_mode_targets
            .iter()
            .find(|(value, _)| *value == mode)
            .unwrap()
            .1;
        assert!(
            panel.shaper_mode_viewport.unwrap().contains(rect.center()),
            "WS {mode} must be visible before click"
        );
        if width < 760.0 {
            assert!(rect.height() >= 44.0 && rect.width() >= 44.0);
            assert!(
                rect.left() >= 0.0 && rect.right() <= width,
                "WS {mode}: {rect:?}, viewport {:?}, content {:?}",
                panel.shaper_mode_viewport,
                ctx.content_rect()
            );
            for (_, position) in &panel.shaper_position_targets {
                assert!(position.height() >= 44.0);
            }
        }
        let choice = rect.center();
        shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![egui::Event::PointerMoved(choice)],
        );
        shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![press_at(choice, true)],
        );
        shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![press_at(choice, false)],
        )
    }

    #[test]
    fn all_ws_real_menu_choices_preserve_timbre_and_button_cycle_feedback() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            native.set_shaper_position((timbre & 1) as u8);
            for mode in 2..super::SHAPER_NAMES.len() as u8 {
                let depth = 13 + mode * 7;
                let choice =
                    select_real_ws_menu(&ctx, &mut panel, &engine, &mut native, 1440.0, mode);
                assert_eq!(choice.actions, vec![PanelAction::SetShaperMode(mode)]);
                assert_eq!(native.panel_state().shaper_mode, mode);
                assert_eq!(native.panel_state().shaper_ws_mode, mode);
                let request = set_knob(&panel, "amp-depth", depth as f64);
                let changed =
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![request]);
                assert_eq!(changed.pots, vec![(1, 0, (depth as u16) << 3)]);
                for expected in [0, 1, mode] {
                    click_primary(&ctx, &mut panel, &engine, &mut native, "amp-drive");
                    assert_eq!(native.panel_state().shaper_mode, expected);
                    assert_eq!(native.panel_state().shaper_ws_mode, mode);
                    assert_eq!(native.panel_state().shaper_depth, depth);
                    assert_eq!(native.panel_state().shaper_position, (timbre & 1) as u8);
                }
            }
            select_real_ws_menu(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                1440.0,
                3 + timbre as u8 * 3,
            );
            click_primary(&ctx, &mut panel, &engine, &mut native, "amp-drive");
        }
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            for expected in [1, 3 + timbre as u8 * 3] {
                click_primary(&ctx, &mut panel, &engine, &mut native, "amp-drive");
                assert_eq!(native.panel_state().shaper_mode, expected);
            }
            let stable = route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert!(stable.pots.is_empty());
            assert_eq!(native.panel_state().shaper_ws_mode, 3 + timbre as u8 * 3);
            assert_eq!(native.panel_state().shaper_depth, 97);
            assert_eq!(panel.pots[1][0] >> 3, 97);
            assert_eq!(native.panel_state().shaper_position, (timbre & 1) as u8);
        }
    }

    #[test]
    fn narrow_ws_menu_keeps_44pt_choices_and_scrolls_to_last_type() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for mode in [2, 12, 3, 11] {
            let choice = select_real_ws_menu(&ctx, &mut panel, &engine, &mut native, 390.0, mode);
            assert_eq!(choice.actions, vec![PanelAction::SetShaperMode(mode)]);
            assert_eq!(native.panel_state().shaper_mode, mode);
        }
    }

    fn click_real_filter2_link(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        native: &mut crate::native::NativePanel,
        width: f32,
    ) -> PanelEvents {
        shaper_menu_frame(ctx, panel, engine, native, width, vec![]);
        let center = panel
            .hit_targets
            .iter()
            .find(|(name, _)| name == "filter2-type")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            shaper_menu_frame(
                ctx,
                panel,
                engine,
                native,
                width,
                vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        shaper_menu_frame(ctx, panel, engine, native, width, vec![]);
        let rect = panel.filter2_link_target.unwrap();
        assert!(
            rect.left() >= 0.0 && rect.right() <= width,
            "LINK {rect:?} in {width}pt"
        );
        if width < 760.0 {
            assert!(rect.height() >= 44.0);
        }
        let choice = rect.center();
        shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![egui::Event::PointerMoved(choice)],
        );
        shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![press_at(choice, true)],
        );
        let selected = shaper_menu_frame(
            ctx,
            panel,
            engine,
            native,
            width,
            vec![press_at(choice, false)],
        );
        if selected.actions.is_empty() {
            for pressed in [true, false] {
                shaper_menu_frame(
                    ctx,
                    panel,
                    engine,
                    native,
                    width,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::default(),
                    }],
                );
            }
        }
        selected
    }

    #[test]
    fn comb_link_menu_and_linked_knobs_forward_timbre_route_and_f1_controls() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            assert!(
                click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 1440.0)
                    .actions
                    .contains(&PanelAction::SetFilter2Link(true))
            );
            click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 1440.0);
            let own = [27 + timbre as u8 * 8, 35 + timbre as u8 * 8];
            for (name, value) in [("filter2-cutoff", own[0]), ("filter2-resonance", own[1])] {
                let event = set_knob(&panel, name, value as f64);
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![event]);
            }
            for _ in 0..3 {
                click_primary(&ctx, &mut panel, &engine, &mut native, "filter2-type");
            }
            assert_eq!(native.panel_state().filter2_type, 3);
            assert_eq!(
                panel
                    .active_leds
                    .iter()
                    .filter(|(x, _)| *x == 704.0)
                    .copied()
                    .collect::<Vec<_>>(),
                vec![(704.0, 2)]
            );
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(
                panel
                    .active_leds
                    .iter()
                    .filter(|(x, _)| *x == 704.0)
                    .copied()
                    .collect::<Vec<_>>(),
                vec![(704.0, 3)]
            );
            for route in 0..4 {
                native.take_comb_updates();
                let changed =
                    click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 1440.0);
                assert_eq!(changed.actions, vec![PanelAction::SetFilter2Link(true)]);
                assert!(native.panel_state().filter2_link);
                for (name, value) in [
                    ("filter2-cutoff", 80 + route * 4),
                    ("filter2-resonance", 52 - route * 4),
                    ("filter-eg1", 91),
                    ("filter-keytrack", 43),
                ] {
                    let event = set_knob(&panel, name, value as f64);
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![event]);
                }
                let program = native.take_comb_updates().last().copied().unwrap();
                assert_eq!((program.0, program.1), (timbre as u8, route));
                assert_eq!(program.2.cutoff.cutoff, own[0]);
                assert_eq!(program.2.resonance.resonance, own[1]);
                assert_eq!(program.2.cutoff.linked_cutoff, 80 + route * 4);
                assert_eq!(program.2.resonance.linked_resonance, 52 - route * 4);
                assert_eq!(program.2.cutoff.linked_eg1_intensity, 91);
                assert_eq!(program.2.linked_key_tracking, 43);
                assert!(program.2.cutoff.link && program.2.resonance.link);
                assert_eq!(
                    native.panel_state().filter2,
                    [80 + route * 4, 52 - route * 4]
                );
                assert_eq!(native.filter_values(), (80 + route * 4, 52 - route * 4, 32));
                let changed =
                    click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 1440.0);
                assert_eq!(changed.actions, vec![PanelAction::SetFilter2Link(false)]);
                assert_eq!(native.panel_state().filter2, own);
                native.take_comb_updates();
                click_primary(&ctx, &mut panel, &engine, &mut native, "filter-routing");
                let forwarded = native.take_comb_updates();
                assert_eq!(
                    (forwarded.last().unwrap().0, forwarded.last().unwrap().1),
                    (timbre as u8, (route + 1) % 4)
                );
            }
            for (name, value) in [
                ("filter1-cutoff", 93 - timbre as u8 * 8),
                ("filter1-resonance", 51 + timbre as u8 * 8),
            ] {
                let event = set_knob(&panel, name, value as f64);
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![event]);
            }
            if timbre & 1 == 0 {
                click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 1440.0);
            }
        }
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().filter2_link, timbre & 1 == 0);
            let expected = if timbre & 1 == 0 {
                [93 - timbre as u8 * 8, 51 + timbre as u8 * 8]
            } else {
                [27 + timbre as u8 * 8, 35 + timbre as u8 * 8]
            };
            assert_eq!(native.panel_state().filter2, expected);
            assert_eq!(
                [panel.pots[1][6] >> 3, panel.pots[1][3] >> 3],
                [expected[0] as u16, expected[1] as u16]
            );
            assert!(
                click_primary(&ctx, &mut panel, &engine, &mut native, "filter-select")
                    .actions
                    .is_empty()
            );
        }
    }

    #[test]
    fn narrow_comb_link_menu_uses_44pt_targets_without_overflow() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for _ in 0..3 {
            native.step_filter2_type();
        }
        assert_eq!(
            click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 390.0).actions,
            vec![PanelAction::SetFilter2Link(true)]
        );
        assert_eq!(
            click_real_filter2_link(&ctx, &mut panel, &engine, &mut native, 390.0).actions,
            vec![PanelAction::SetFilter2Link(false)]
        );
    }

    #[test]
    fn shaper_depth_keeps_fractional_drag_shift_and_wheel_on_both_positions() {
        let engine = isolated_engine();
        for mode in 0..super::SHAPER_NAMES.len() as u8 {
            for position in 0..2 {
                for (shift, engaged, fine_end) in [(false, 70, 84), (true, 65, 66)] {
                    let ctx = egui::Context::default();
                    let mut panel = Panel::new();
                    let mut native =
                        crate::native::NativePanel::new(std::path::Path::new("."), true);
                    native.enabled = true;
                    native.set_shaper_mode(mode);
                    native.set_shaper_position(position);
                    native.pot(1, 0, 64 << 3);
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                    let center = panel
                        .hit_targets
                        .iter()
                        .find(|(n, _)| n == "amp-depth")
                        .unwrap()
                        .1
                        .center();
                    let mut render = |mut events: Vec<egui::Event>| {
                        events.insert(
                            0,
                            egui::Event::ModifiersChanged(egui::Modifiers {
                                shift,
                                ..Default::default()
                            }),
                        );
                        route_primary_frame(&ctx, &mut panel, &engine, &mut native, events);
                        native.panel_state().shaper_depth
                    };
                    assert_eq!(
                        render(vec![
                            egui::Event::PointerMoved(center),
                            press_at(center, true)
                        ]),
                        64
                    );
                    assert_eq!(
                        render(vec![egui::Event::PointerMoved(
                            center - egui::vec2(0.0, 8.0)
                        )]),
                        engaged
                    );
                    for step in 1..=80 {
                        render(vec![egui::Event::PointerMoved(
                            center - egui::vec2(0.0, 8.0 + step as f32 * 0.25),
                        )]);
                    }
                    assert_eq!(render(vec![]), fine_end);
                    render(vec![press_at(center - egui::vec2(0.0, 28.0), false)]);
                    native.pot(1, 0, 64 << 3);
                    route_primary_frame(
                        &ctx,
                        &mut panel,
                        &engine,
                        &mut native,
                        vec![
                            egui::Event::ModifiersChanged(egui::Modifiers::default()),
                            egui::Event::PointerMoved(center),
                        ],
                    );
                    route_primary_frame(
                        &ctx,
                        &mut panel,
                        &engine,
                        &mut native,
                        vec![egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, 32.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::default(),
                        }],
                    );
                    for _ in 0..32 {
                        route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                    }
                    assert_eq!(native.panel_state().shaper_depth, 68);
                    assert_eq!(ctx.input(|i| i.smooth_scroll_delta.y), 0.0);
                }
            }
        }
    }

    #[test]
    fn noise_formant_selection_and_both_controls_deliver_each_timbre_primary_packet() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            for waveform in [4, 5] {
                native.take_primary_updates();
                while native.panel_state().waveform != waveform {
                    assert_eq!(
                        click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-wave-up")
                            .actions,
                        vec![PanelAction::StepWaveform(1)]
                    );
                }
                let selected = native.take_primary_updates();
                assert_eq!(
                    (
                        selected.last().unwrap().0,
                        selected.last().unwrap().1.selection
                    ),
                    (timbre as u8, waveform as u8)
                );
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                assert!(panel.active_leds.iter().all(|(x, _)| *x != 168.0));
                assert_eq!(
                    panel
                        .active_leds
                        .iter()
                        .filter(|(x, _)| *x == 246.0)
                        .copied()
                        .collect::<Vec<_>>(),
                    vec![(246.0, if waveform == 4 { 1 } else { 0 })]
                );
                assert_primary_led(&panel, 0);
                let values = [
                    31 + timbre as u8 * 8 + waveform as u8,
                    98 - timbre as u8 * 8 - waveform as u8,
                ];
                let events = vec![
                    set_knob(&panel, "osc1-control1", values[0] as f64),
                    set_knob(&panel, "osc1-control2", values[1] as f64),
                ];
                let changed = route_primary_frame(&ctx, &mut panel, &engine, &mut native, events);
                assert_eq!(
                    changed.pots,
                    vec![
                        (0, 7, (values[0] as u16) << 3),
                        (0, 6, (values[1] as u16) << 3)
                    ]
                );
                let updates = native.take_primary_updates();
                assert_eq!(updates.len(), 2);
                let (index, program) = updates[1];
                assert_eq!((index, program.selection), (timbre as u8, waveform as u8));
                assert_eq!([program.control.control1, program.control.control2], values);
                native.take_mixer_updates();
                let event = set_knob(&panel, "mixer-noise", (43 + timbre * 16 + waveform) as f64);
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![event]);
                let updates = native.take_mixer_updates();
                assert_eq!(updates.len(), 1);
                assert_eq!(
                    (updates[0].0, updates[0].1.selections[0]),
                    (timbre as u8, waveform as u8)
                );
                assert_eq!(
                    updates[0].1.levels,
                    [127, 0, (43 + timbre * 16 + waveform) as u8]
                );
            }
            if timbre & 1 == 0 {
                click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-wave-down");
            }
        }
        for timbre in 0..4 {
            click_primary(
                &ctx,
                &mut panel,
                &engine,
                &mut native,
                &format!("timbre{}", timbre + 1),
            );
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().waveform, 4 + (timbre & 1));
            assert_eq!(
                native.primary_controls(),
                [(36 + timbre * 8) as u8, (93 - timbre * 8) as u8]
            );
            assert_eq!(
                [panel.pots[0][7] >> 3, panel.pots[0][6] >> 3],
                [(36 + timbre * 8) as u16, (93 - timbre * 8) as u16]
            );
            assert_eq!(panel.pots[0][1] >> 3, (48 + timbre * 16) as u16);
            click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-mod");
            assert_eq!(
                (
                    native.panel_state().waveform,
                    native.panel_state().primary_mode
                ),
                (0, 1)
            );
        }
    }

    #[test]
    fn osc1_mod_cycles_all_four_modes_with_control_and_timbre_feedback() {
        let engine = isolated_engine();
        for waveform in 0..4 {
            let ctx = egui::Context::default();
            let mut panel = Panel::new();
            let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
            native.enabled = true;
            native.step_waveform(waveform);
            native.pot(0, 7, 29 << 3);
            native.pot(0, 6, 77 << 3);
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_primary_led(&panel, 0);
            for (mode, control) in [(1, [43, 66]), (2, [51, 89]), (3, [29, 74])] {
                let clicked = click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-mod");
                assert_eq!(clicked.actions, vec![PanelAction::StepPrimaryModulation]);
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                assert_eq!(native.panel_state().primary_mode, mode);
                assert_primary_led(&panel, mode as usize);
                let requested = vec![
                    set_knob(&panel, "osc1-control1", control[0] as f64),
                    set_knob(&panel, "osc1-control2", control[1] as f64),
                ];
                let changed =
                    route_primary_frame(&ctx, &mut panel, &engine, &mut native, requested);
                assert_eq!(
                    changed.pots,
                    vec![
                        (0, 7, (control[0] as u16) << 3),
                        (0, 6, (control[1] as u16) << 3)
                    ]
                );
                assert_eq!(native.primary_controls(), control);
                assert_eq!(native.panel_state().primary_mode, mode);
                let previous_wave = native.panel_state().waveform;
                assert_eq!(
                    click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-wave-up").actions,
                    vec![PanelAction::StepWaveform(1)]
                );
                assert_eq!(native.panel_state().waveform, (previous_wave + 1) % 4);
                assert_eq!(native.panel_state().primary_mode, mode);
                click_primary(&ctx, &mut panel, &engine, &mut native, "timbre2");
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                assert_eq!(native.panel_state().primary_mode, 0);
                assert_eq!(native.primary_controls(), [0, 0]);
                assert_primary_led(&panel, 0);
                click_primary(&ctx, &mut panel, &engine, &mut native, "timbre1");
                route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
                assert_eq!(native.panel_state().primary_mode, mode);
                assert_eq!(
                    [panel.pots[0][7] >> 3, panel.pots[0][6] >> 3],
                    [control[0] as u16, control[1] as u16]
                );
                assert_primary_led(&panel, mode as usize);
                let previous_group = native.panel_state().voice_group;
                assert_eq!(
                    click_primary(&ctx, &mut panel, &engine, &mut native, "unison").actions,
                    vec![PanelAction::ToggleVoiceGroup]
                );
                assert_eq!(
                    native.panel_state().voice_group.raw,
                    previous_group.raw ^ 128
                );
            }
            click_primary(&ctx, &mut panel, &engine, &mut native, "osc1-mod");
            route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
            assert_eq!(native.panel_state().primary_mode, 0);
            assert_eq!(native.primary_controls(), [29, 74]);
            assert_primary_led(&panel, 0);
        }
    }

    #[test]
    fn unsupported_primary_modes_have_no_active_led_and_ignore_input() {
        let engine = isolated_engine();
        for mode in [4, 5] {
            let ctx = egui::Context::default();
            let mut panel = Panel::new();
            let mut state = primary_state(0, 0);
            state.primary_mode = mode;
            frame_native(&ctx, &mut panel, &engine, input(vec![]), state);
            assert!(!panel.active_leds.iter().any(|(x, _)| *x == 243.0));
            let center = panel
                .hit_targets
                .iter()
                .find(|(n, _)| n == "osc1-mod")
                .unwrap()
                .1
                .center();
            let requested = vec![
                egui::Event::PointerMoved(center),
                press_at(center, true),
                press_at(center, false),
                set_knob(&panel, "osc1-control1", 127.0),
                set_knob(&panel, "osc1-control2", 127.0),
            ];
            let result = frame_native(&ctx, &mut panel, &engine, input(requested), state);
            assert!(result.actions.is_empty());
            assert!(result.pots.is_empty());
            assert!(!panel.active_leds.iter().any(|(x, _)| *x == 243.0));
        }
    }

    #[test]
    fn unison_control2_edit_keeps_held_pad_and_is_retained_for_the_next_note() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let mut native = crate::native::NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        native.step_primary_modulation();
        native.step_primary_modulation();
        route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![]);
        let center = panel
            .hit_targets
            .iter()
            .find(|(n, _)| n == "step-1")
            .unwrap()
            .1
            .center();
        let on = route_primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut native,
            vec![egui::Event::PointerMoved(center), press_at(center, true)],
        );
        assert_eq!(
            crate::controls::note_transitions(&[], &on.notes).collect::<Vec<_>>(),
            vec![(60, true)]
        );
        let change = set_knob(&panel, "osc1-control2", 89.0);
        let held = route_primary_frame(&ctx, &mut panel, &engine, &mut native, vec![change]);
        assert_eq!(native.primary_controls()[1], 89);
        assert_eq!(native.panel_state().primary_mode, 2);
        assert_eq!(held.notes, vec![60]);
        assert!(
            crate::controls::note_transitions(&on.notes, &held.notes)
                .next()
                .is_none(),
            "phase-value edit must not manufacture a note retrigger"
        );
        let off = route_primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut native,
            vec![press_at(center, false)],
        );
        assert_eq!(
            crate::controls::note_transitions(&held.notes, &off.notes).collect::<Vec<_>>(),
            vec![(60, false)]
        );
        let next = route_primary_frame(
            &ctx,
            &mut panel,
            &engine,
            &mut native,
            vec![press_at(center, true)],
        );
        assert_eq!(
            crate::controls::note_transitions(&off.notes, &next.notes).collect::<Vec<_>>(),
            vec![(60, true)]
        );
        assert_eq!(
            native.primary_controls()[1],
            89,
            "INITIAL PHASE remains set for the next note-on"
        );
    }

    fn pad_center(
        ctx: &egui::Context,
        panel: &mut Panel,
        engine: &Engine,
        number: usize,
    ) -> egui::Pos2 {
        frame(ctx, panel, engine, vec![]);
        panel
            .hit_targets
            .iter()
            .find(|(name, _)| name == &format!("step-{number}"))
            .unwrap()
            .1
            .center()
    }

    #[test]
    fn all_sixteen_physical_pads_play_c4_through_d_sharp5() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        for number in 1..=16 {
            let center = pad_center(&ctx, &mut panel, &engine, number);
            let pressed = frame(
                &ctx,
                &mut panel,
                &engine,
                vec![egui::Event::PointerMoved(center), press_at(center, true)],
            );
            assert_eq!(pressed.notes, vec![59 + number as u8]);
            assert!(panel.native_pad_held[number - 1]);
            let released = frame(&ctx, &mut panel, &engine, vec![press_at(center, false)]);
            assert!(released.notes.is_empty());
            assert!(!panel.native_pad_held[number - 1]);
        }
    }

    #[test]
    fn pad_capture_survives_outside_motion_and_releases_without_retrigger() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let center = pad_center(&ctx, &mut panel, &engine, 1);
        let down = frame(
            &ctx,
            &mut panel,
            &engine,
            vec![egui::Event::PointerMoved(center), press_at(center, true)],
        );
        assert_eq!(
            crate::controls::note_transitions(&[], &down.notes).collect::<Vec<_>>(),
            vec![(60, true)]
        );
        let outside = center + egui::vec2(120.0, -100.0);
        let held = frame(
            &ctx,
            &mut panel,
            &engine,
            vec![egui::Event::PointerMoved(outside)],
        );
        assert_eq!(held.notes, vec![60]);
        assert!(
            crate::controls::note_transitions(&down.notes, &held.notes)
                .next()
                .is_none()
        );
        let released = frame(&ctx, &mut panel, &engine, vec![press_at(outside, false)]);
        assert_eq!(
            crate::controls::note_transitions(&held.notes, &released.notes).collect::<Vec<_>>(),
            vec![(60, false)]
        );
        assert!(released.notes.is_empty());
        assert!(frame(&ctx, &mut panel, &engine, vec![]).notes.is_empty());
    }

    #[test]
    fn quick_pad_click_sounds_one_frame_then_sends_ordered_note_off() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let center = pad_center(&ctx, &mut panel, &engine, 16);
        let clicked = frame(
            &ctx,
            &mut panel,
            &engine,
            vec![
                egui::Event::PointerMoved(center),
                press_at(center, true),
                press_at(center, false),
            ],
        );
        assert_eq!(clicked.notes, vec![75]);
        let mut midi = crate::controls::note_transitions(&[], &clicked.notes).collect::<Vec<_>>();
        let next = frame(&ctx, &mut panel, &engine, vec![]);
        assert!(next.notes.is_empty());
        midi.extend(crate::controls::note_transitions(
            &clicked.notes,
            &next.notes,
        ));
        assert_eq!(midi, vec![(75, true), (75, false)]);
    }

    #[test]
    fn background_accessibility_activation_sounds_then_releases() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        frame(&ctx, &mut panel, &engine, vec![]);
        let id = panel
            .target_ids
            .iter()
            .find(|(name, _)| name == "step-1")
            .unwrap()
            .1;
        let clicked = frame_focused(
            &ctx,
            &mut panel,
            &engine,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_node: id.accesskit_id(),
                    target_tree: egui::accesskit::TreeId::ROOT,
                    data: None,
                },
            )],
            false,
        );
        assert_eq!(clicked.notes, vec![60]);
        let released = frame_focused(&ctx, &mut panel, &engine, vec![], false);
        assert!(released.notes.is_empty());
    }

    #[test]
    fn losing_focus_releases_a_pointer_held_pad() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        let center = pad_center(&ctx, &mut panel, &engine, 1);
        let pressed = frame(
            &ctx,
            &mut panel,
            &engine,
            vec![egui::Event::PointerMoved(center), press_at(center, true)],
        );
        assert_eq!(pressed.notes, vec![60]);
        let blurred = frame_focused(&ctx, &mut panel, &engine, vec![], false);
        assert_eq!(
            crate::controls::note_transitions(&pressed.notes, &blurred.notes).collect::<Vec<_>>(),
            vec![(60, false)]
        );
    }

    #[test]
    fn supported_physical_buttons_emit_their_exact_native_actions() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        for index in 0..4 {
            assert_eq!(
                click(&ctx, &mut panel, &engine, &format!("timbre{}", index + 1)).actions,
                vec![PanelAction::SelectTimbre(index)]
            );
            assert_eq!(
                click(
                    &ctx,
                    &mut panel,
                    &engine,
                    &format!("timbre{}-power", index + 1)
                )
                .actions,
                vec![PanelAction::ToggleTimbre(index)]
            );
        }
        assert_eq!(
            click(&ctx, &mut panel, &engine, "osc1-wave-up").actions,
            vec![PanelAction::StepWaveform(1)]
        );
        assert_eq!(
            click(&ctx, &mut panel, &engine, "osc1-wave-down").actions,
            vec![PanelAction::StepWaveform(-1)]
        );
        assert_eq!(
            click(&ctx, &mut panel, &engine, "osc2-wave").actions,
            vec![PanelAction::StepSecondaryWaveform]
        );
        assert_eq!(
            click(&ctx, &mut panel, &engine, "osc2-mod").actions,
            vec![PanelAction::StepSecondaryModulation]
        );
    }

    #[test]
    fn unavailable_physical_buttons_do_not_emit_fake_native_actions() {
        let engine = isolated_engine();
        let ctx = egui::Context::default();
        let mut panel = Panel::new();
        for id in [
            "fx-on",
            "master-on",
            "arp-on",
            "lfo1-select",
            "lfo2-select",
            "write",
        ] {
            let events = click(&ctx, &mut panel, &engine, id);
            assert!(events.actions.is_empty(), "{id}");
            assert!(events.pots.is_empty(), "{id}");
            assert!(events.notes.is_empty(), "{id}");
        }
    }
}
