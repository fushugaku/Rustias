//! Presentation adapter for the currently qualified native VA voice.
use eframe::egui;
use radias_synth_application::amplifier::{AmplifierProgram, ControllerTables};
use radias_synth_application::comb::CombProgram;
use radias_synth_application::mixer::MixerProgram;
use radias_synth_application::modulation::{ModulationProgram, VoiceModulationTables};
use radias_synth_application::primary::PrimaryProgram;
use radias_synth_application::secondary::SecondaryProgram;
use radias_synth_application::voice_envelopes::{DynamicFilter, ModEnvelopeProgram};
use radias_synth_domain::filter::FilterCoefficients;
use radias_synth_domain::filter_routing::{Filter2Coefficients, Filter2Output};
use radias_synth_infrastructure::{
    audio::NativePlayer,
    firmware::{
        MasterTables, amplifier_tables, comb_control_tables, controller_filter_tables,
        envelope_curves, envelope_timing_tables, filter2_control_tables, fine_tune_table,
        formant_counter_seeds, lfo_tables, lfo_tempo_tables, mixer_scales, modulation_tables,
        pan_tables, voice_cost_tables,
    },
    prepared::{ControlMap, PreparedVoice},
};
use std::{fs, path::Path};

pub struct NativePanel {
    player: Option<NativePlayer>,
    controls: Option<ControlMap>,
    base: Option<FilterCoefficients>,
    mix_table: Option<radias_synth_domain::filter_control::FilterMixTable>,
    pub enabled: bool,
    timbres: [TimbreControls; 4],
    selected: usize,
    error: String,
    tempo: u16,
    comb_configured: bool,
    bank: Vec<radias_synth_domain::program::Program>,
    global_performance: radias_synth_domain::performance::GlobalPerformance,
    drum_kits: Vec<radias_synth_domain::drum::DrumKit>,
    drum_editor: Option<DrumEditor>,
    suppress_parameter_commands: bool,
    stored_program_selected: Option<usize>,
    #[cfg(test)]
    comb_updates: Vec<(u8, u8, CombProgram)>,
    #[cfg(test)]
    modulation_updates: Vec<(u8, ModulationProgram)>,
    #[cfg(test)]
    modulation_header: Option<egui::Rect>,
    #[cfg(test)]
    destination_targets: Vec<(usize, egui::Rect)>,
    #[cfg(test)]
    destination_choices: Vec<(usize, u8, egui::Rect)>,
    #[cfg(test)]
    destination_viewport: Option<egui::Rect>,
    #[cfg(test)]
    primary_updates: Vec<(u8, PrimaryProgram)>,
    #[cfg(test)]
    mixer_updates: Vec<(u8, MixerProgram)>,
    #[cfg(test)]
    amplifier_updates: Vec<(u8, AmplifierProgram)>,
    #[cfg(test)]
    envelope_header: Option<egui::Rect>,
    #[cfg(test)]
    envelope_targets: Vec<(usize, usize, egui::Id, egui::Rect)>,
}
struct DrumEditor {
    source: radias_synth_domain::program::Program,
    kit: radias_synth_domain::drum::DrumKit,
    owner: usize,
    selected: usize,
    views: [TimbreControls; 16],
}

pub const TEMPO_DIVISION_LABELS: [&str; 17] = [
    "8/1", "4/1", "2/1", "1/1", "3/4", "1/2", "3/8", "1/3", "1/4", "3/16", "1/6", "1/8", "1/12",
    "1/16", "1/24", "1/32", "1/64",
];

#[derive(Clone, Copy)]
struct TimbreControls {
    voice_mode: radias_synth_domain::mono_notes::VoiceMode,
    voice_group: radias_synth_domain::voice_group::VoiceGroupProgram,
    sustain: radias_synth_domain::sustain::SustainProgram,
    pitch: radias_synth_domain::note_pitch::PitchProgram,
    portamento: radias_synth_domain::portamento::PortamentoProgram,
    cutoff: u8,
    resonance: u8,
    filter_type: u8,
    filter_route: u8,
    filter2_cutoff: u8,
    filter2_resonance: u8,
    filter2_type: u8,
    filter2_link: bool,
    filter2_eg1_intensity: u8,
    filter2_key_tracking: u8,
    shaper_mode: u8,
    shaper_ws_mode: u8,
    shaper_position: u8,
    shaper_depth: u8,
    waveform: usize,
    primary_mode: u8,
    amplifier: AmplifierProgram,
    pan_position: u8,
    mixer_levels: [u8; 3],
    secondary: SecondaryProgram,
    primary_controls: [u8; 2],
    enabled: bool,
    channel: u8,
    modulation: ModulationProgram,
    auxiliary: [ModEnvelopeProgram; 2],
    eg1_intensity: u8,
    key_tracking: u8,
}

impl TimbreControls {
    fn write_drum_body(self, body: &mut [u8; 104]) {
        body[0x13] = self.pitch.transpose;
        body[0x14] = self.pitch.fine_tune;
        body[0x15] = self.pitch.vibrato_intensity;
        body[0x16] = (body[0x16] & 0xc0) | (self.primary_selection() & 63);
        body[0x17..0x19].copy_from_slice(&self.primary_controls);
        body[0x1b] = self.secondary.selection;
        body[0x1c] = self.secondary.pitch.semitone;
        body[0x1d] = self.secondary.pitch.fine_tune;
        body[0x1e..0x21].copy_from_slice(&self.mixer_levels);
        body[0x21] = (body[0x21] & 0x4c)
            | (self.filter_route & 3)
            | ((self.filter2_type & 3) << 4)
            | if self.filter2_link { 128 } else { 0 };
        body[0x22] = self.filter_type;
        body[0x23] = self.cutoff;
        body[0x24] = self.resonance;
        body[0x25] = self.eg1_intensity;
        body[0x26] = self.key_tracking;
        body[0x28] = self.filter2_cutoff;
        body[0x29] = self.filter2_resonance;
        body[0x2a] = self.filter2_eg1_intensity;
        body[0x2b] = self.filter2_key_tracking;
        body[0x2d] = self.amplifier.level;
        body[0x2e] = (body[0x2e] & !0x13)
            | self.shaper_mode.min(2)
            | if self.shaper_position != 0 { 16 } else { 0 };
        if self.shaper_mode >= 2 {
            body[0x2f] = (body[0x2f] & 0xf0) | (self.shaper_mode - 2);
        }
        body[0x30] = self.shaper_depth;
        body[0x31] = self.pan_position;
        body[0x32] = self.amplifier.key_tracking;
        for (i, eg) in [
            self.auxiliary[0],
            self.amplifier.envelope,
            self.auxiliary[1],
        ]
        .iter()
        .enumerate()
        {
            let b = 0x34 + 8 * i;
            body[b..b + 4].copy_from_slice(&eg.adsr);
            body[b + 4] = eg.curve;
            body[b + 5] = eg.velocity_level_sensitivity;
            body[b + 6] = eg.velocity_time_sensitivity;
            body[b + 7] = eg.key_tracking;
        }
        for (i, lfo) in self.modulation.lfo.iter().enumerate() {
            let b = 0x4c + 5 * i;
            body[b] = lfo.waveform;
            body[b + 1] = lfo.shape;
            body[b + 2] = lfo.frequency;
            body[b + 3] = lfo.phase_sync;
            body[b + 4] = self.modulation.tempo_divisions[i];
        }
        for (i, route) in self.modulation.routes.iter().enumerate() {
            let b = 0x56 + 3 * i;
            body[b] = route.source;
            body[b + 1] = route.destination.index() as u8;
            body[b + 2] = route.intensity;
        }
    }
    fn from_stored(source: radias_synth_application::program::StoredTimbre) -> Self {
        let c = source.controls;
        let shaper =
            match c.shaper.mode {
                radias_synth_application::shaper::ShaperMode::Off => 0,
                radias_synth_application::shaper::ShaperMode::Drive => 1,
                radias_synth_application::shaper::ShaperMode::HardClip => 2,
                radias_synth_application::shaper::ShaperMode::Waveshaper(kind) => {
                    if kind as u8 == 0 { 3 } else { kind as u8 + 2 }
                }
            };
        Self {
            voice_mode: c.voice_mode,
            voice_group: c.voice_group,
            sustain: c.sustain,
            pitch: c.pitch,
            portamento: c.portamento,
            cutoff: c.cutoff[0] & 127,
            resonance: c.resonance[0] & 127,
            filter_type: c.filter_type & 127,
            filter_route: c.filter_route & 3,
            filter2_cutoff: c.cutoff[1] & 127,
            filter2_resonance: c.resonance[1] & 127,
            filter2_type: (c.filter_route >> 4) & 3,
            filter2_link: c.filter_route & 128 != 0,
            filter2_eg1_intensity: c.filter2_eg_intensity,
            filter2_key_tracking: c.filter2_key_tracking,
            waveform: (c.oscillator_selection & 15) as usize,
            primary_mode: (c.oscillator_selection >> 4) & 3,
            shaper_mode: shaper,
            shaper_ws_mode: if shaper >= 2 { shaper } else { 2 },
            shaper_position: if c.shaper.position
                == radias_synth_domain::waveshaper::ShaperPosition::PreAmp
            {
                1
            } else {
                0
            },
            shaper_depth: c.shaper.control.depth,
            amplifier: c.amplifier(0x7f00, None, 0),
            pan_position: c.pan,
            mixer_levels: c.mixer.levels,
            secondary: c.secondary,
            primary_controls: c.oscillator_controls,
            enabled: source.enabled,
            channel: source.channel,
            modulation: c.modulation,
            auxiliary: [c.envelope[0], c.envelope[2]],
            eg1_intensity: c.eg1_intensity,
            key_tracking: c.filter_key_tracking,
        }
    }
    fn filter2_values(self) -> [u8; 2] {
        if self.filter2_link {
            [self.cutoff, self.resonance]
        } else {
            [self.filter2_cutoff, self.filter2_resonance]
        }
    }
    fn comb_program(self) -> Option<CombProgram> {
        if self.filter2_type != 3
            || self.filter_route as usize >= crate::panel::FILTER_ROUTING_NAMES.len()
        {
            return None;
        }
        Some(self.filter2_controls())
    }
    fn filter2_controls(self) -> CombProgram {
        let default = CombProgram::default();
        CombProgram {
            cutoff: radias_synth_domain::controller_comb::CombCutoffControl {
                cutoff: self.filter2_cutoff,
                linked_cutoff: self.cutoff,
                link: self.filter2_link,
                linked_eg1_intensity: self.eg1_intensity,
                eg1_intensity: self.filter2_eg1_intensity,
                key_offset: 0,
                ..default.cutoff
            },
            resonance: radias_synth_domain::controller_comb::CombResonanceControl {
                resonance: self.filter2_resonance,
                linked_resonance: self.resonance,
                link: self.filter2_link,
                ..default.resonance
            },
            linked_key_tracking: self.key_tracking,
            key_tracking: self.filter2_key_tracking,
            ..default
        }
    }
    fn shaper_parameters(self) -> Option<(u8, u8, u8)> {
        ((self.shaper_mode as usize) < crate::panel::SHAPER_NAMES.len()
            && (2..crate::panel::SHAPER_NAMES.len() as u8).contains(&self.shaper_ws_mode)
            && (self.shaper_position as usize) < crate::panel::SHAPER_POSITION_NAMES.len()
            && self.shaper_depth <= 127)
            .then_some((self.shaper_mode, self.shaper_position, self.shaper_depth))
    }
    fn filter2_coefficients(
        self,
        controls: &ControlMap,
        base: FilterCoefficients,
    ) -> Result<Filter2Coefficients, &'static str> {
        let output = [
            Filter2Output::LowPass,
            Filter2Output::HighPass,
            Filter2Output::BandPass,
        ]
        .get(self.filter2_type as usize)
        .copied()
        .ok_or("Filter 2 output not supported")?;
        let compiled = controls.filter(base, self.filter2_cutoff, self.filter2_resonance)?;
        Ok(Filter2Coefficients {
            input_gain: compiled.input_gain,
            feedback: compiled.feedback,
            integrator_gain: compiled.integrator_gain,
            output,
        })
    }
    fn primary_selection(self) -> u8 {
        self.waveform as u8 | (self.primary_mode << 4)
    }
    fn primary_program(self) -> Option<PrimaryProgram> {
        crate::panel::primary_selection_supported(self.waveform, self.primary_mode).then_some(
            PrimaryProgram {
                selection: self.primary_selection(),
                control: radias_synth_domain::controller_primary::PrimaryControl {
                    control1: self.primary_controls[0],
                    control2: self.primary_controls[1],
                    ..Default::default()
                },
            },
        )
    }
    fn mixer_program(self) -> MixerProgram {
        MixerProgram {
            selections: [self.primary_selection(), self.secondary.selection],
            levels: self.mixer_levels,
            manual_offsets: [0; 3],
        }
    }
}

impl NativePanel {
    pub fn panel_state(&self) -> crate::panel::NativePanelState {
        crate::panel::NativePanelState {
            voice_mode: self.timbres[self.selected].voice_mode,
            voice_group: self.timbres[self.selected].voice_group,
            selected: self.selected,
            enabled: self.timbres.map(|t| t.enabled),
            waveform: self.timbres[self.selected].waveform,
            filter_route: self.timbres[self.selected].filter_route,
            filter2_type: self.timbres[self.selected].filter2_type,
            filter2: self.timbres[self.selected].filter2_values(),
            filter2_link: self.timbres[self.selected].filter2_link,
            primary_mode: self.timbres[self.selected].primary_mode,
            shaper_mode: self.timbres[self.selected].shaper_mode,
            shaper_ws_mode: self.timbres[self.selected].shaper_ws_mode,
            shaper_position: self.timbres[self.selected].shaper_position,
            shaper_depth: self.timbres[self.selected].shaper_depth,
            secondary: self.timbres[self.selected].secondary.selection,
            lfo_tempo: self.timbres[self.selected]
                .modulation
                .lfo
                .map(|p| p.phase_sync & 128 != 0),
        }
    }
    pub fn select_timbre(&mut self, index: usize) {
        if index < self.timbres.len() {
            self.flush_drum_edits();
            self.selected = index;
            self.suppress_parameter_commands =
                self.drum_editor.as_ref().is_some_and(|e| e.owner == index);
        }
    }
    pub fn has_drum_program(&self) -> bool {
        self.drum_editor.is_some()
    }
    pub fn select_drum_instrument(&mut self, index: usize) {
        if index >= 16 {
            return;
        }
        self.flush_drum_edits();
        let Some(editor) = &mut self.drum_editor else {
            return;
        };
        let (enabled, channel) = (
            self.timbres[editor.owner].enabled,
            self.timbres[editor.owner].channel,
        );
        editor.selected = index;
        self.selected = editor.owner;
        self.timbres[editor.owner] = editor.views[index];
        self.timbres[editor.owner].enabled = enabled;
        self.timbres[editor.owner].channel = channel;
        self.suppress_parameter_commands = true;
    }
    pub fn drum_pad(&mut self, index: u8, on: bool) {
        if on {
            self.select_drum_instrument(index as usize);
        }
        if let Some(player) = &self.player
            && let Err(error) = player.drum_pad(index, if on { 100 } else { 0 })
        {
            self.error = error;
        }
    }
    pub fn flush_drum_edits(&mut self) {
        if !self.suppress_parameter_commands {
            return;
        }
        let Some(editor) = &mut self.drum_editor else {
            return;
        };
        let current = self.timbres[editor.owner];
        let mut body = *editor.kit.instrument(editor.selected).unwrap();
        current.write_drum_body(&mut body);
        if editor.kit.instrument(editor.selected) == Some(&body) {
            return;
        }
        let mut changed = editor.kit.clone();
        changed.replace_instrument(editor.selected, &body).unwrap();
        let (Some(map), Some(mix), Some(base)) = (&self.controls, &self.mix_table, self.base)
        else {
            return;
        };
        match radias_synth_infrastructure::stored_program::compile_drum_kit(
            &editor.source,
            changed.clone(),
            map,
            mix,
            base,
        ) {
            Ok(compiled) => {
                if let Some(player) = &self.player
                    && let Err(error) = player.edit_drum_instrument(
                        editor.selected as u8,
                        compiled.instruments[editor.selected],
                    )
                {
                    self.error = error;
                    return;
                }
                editor.kit = changed;
                editor.views[editor.selected] = current;
            }
            Err(error) => self.error = error,
        }
    }
    pub fn set_timbre_enabled(&mut self, index: usize, enabled: bool) {
        if let Some(timbre) = self.timbres.get_mut(index) {
            timbre.enabled = enabled;
            if let Some(player) = &self.player
                && let Err(error) = player.timbre(index as u8, enabled, timbre.channel)
            {
                self.error = error;
            }
        }
    }
    pub fn step_waveform(&mut self, direction: i32) {
        let t = self.timbres[self.selected];
        let count = if t.primary_mode == 0 {
            crate::panel::PRIMARY_WAVEFORM_NAMES.len()
        } else {
            4
        };
        self.set_waveform((t.waveform as i32 + direction).rem_euclid(count as i32) as usize);
    }
    pub fn set_waveform(&mut self, waveform: usize) {
        if waveform >= crate::panel::PRIMARY_WAVEFORM_NAMES.len() {
            return;
        }
        let t = &mut self.timbres[self.selected];
        t.waveform = waveform;
        if waveform >= 4 {
            t.primary_mode = 0;
        }
        self.update_primary();
    }
    pub fn step_primary_modulation(&mut self) {
        let t = &mut self.timbres[self.selected];
        if t.primary_program().is_none() {
            return;
        }
        t.primary_mode = (t.primary_mode + 1) % crate::panel::PRIMARY_MODE_NAMES.len() as u8;
        if t.primary_mode != 0 && t.waveform >= 4 {
            t.waveform = 0;
        }
        self.update_primary();
    }
    pub fn step_filter_routing(&mut self) {
        let t = &mut self.timbres[self.selected];
        if t.filter_route as usize >= crate::panel::FILTER_ROUTING_NAMES.len()
            || t.filter2_type as usize >= crate::panel::FILTER2_NAMES.len()
        {
            return;
        }
        t.filter_route = (t.filter_route + 1) % crate::panel::FILTER_ROUTING_NAMES.len() as u8;
        self.update_filter_routing();
    }
    pub fn step_filter2_type(&mut self) {
        let t = &mut self.timbres[self.selected];
        if t.filter_route as usize >= crate::panel::FILTER_ROUTING_NAMES.len()
            || t.filter2_type as usize >= crate::panel::FILTER2_NAMES.len()
        {
            return;
        }
        t.filter2_type = (t.filter2_type + 1) % crate::panel::FILTER2_NAMES.len() as u8;
        self.update_filter_routing();
    }
    pub fn set_filter2_link(&mut self, linked: bool) {
        let t = &mut self.timbres[self.selected];
        if t.filter2_type >= 4 || t.filter_route >= 4 {
            return;
        }
        t.filter2_link = linked;
        self.update_filter_routing();
    }
    #[cfg(test)]
    pub(crate) fn take_comb_updates(&mut self) -> Vec<(u8, u8, CombProgram)> {
        core::mem::take(&mut self.comb_updates)
    }
    fn update_filter_routing(&mut self) {
        let t = self.timbres[self.selected];
        if let Some(program) = t.comb_program() {
            #[cfg(test)]
            self.comb_updates
                .push((self.selected as u8, t.filter_route, program));
            if !self.suppress_parameter_commands
                && !self.suppress_parameter_commands
                && let Some(player) = &self.player
                && let Err(error) = player.timbre_comb(self.selected as u8, t.filter_route, program)
            {
                self.error = error;
            }
            return;
        }
        if let (Some(player), Some(controls), Some(base)) =
            (&self.player, &self.controls, self.base)
        {
            let t = self.timbres[self.selected];
            match t.filter2_coefficients(controls, base) {
                Ok(second) => {
                    if let Err(error) =
                        player.timbre_filter_routing(self.selected as u8, t.filter_route, second)
                    {
                        self.error = error;
                    }
                    let program = radias_synth_application::filter2::Filter2Program {
                        route: t.filter_route
                            | t.filter2_type << 4
                            | if t.filter2_link { 128 } else { 0 },
                        controls: t.filter2_controls(),
                        normalization: controls.normalization,
                    };
                    if let Err(error) = player.timbre_filter2_program(self.selected as u8, program)
                    {
                        self.error = error;
                    }
                }
                Err(error) => self.error = error.into(),
            }
        }
    }
    fn update_primary(&mut self) {
        if let Some(program) = self.timbres[self.selected].primary_program() {
            #[cfg(test)]
            self.primary_updates.push((self.selected as u8, program));
            if !self.suppress_parameter_commands
                && !self.suppress_parameter_commands
                && let Some(player) = &self.player
                && let Err(error) = player.timbre_primary_control(self.selected as u8, program)
            {
                self.error = error;
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn take_primary_updates(&mut self) -> Vec<(u8, PrimaryProgram)> {
        core::mem::take(&mut self.primary_updates)
    }
    #[cfg(test)]
    pub(crate) fn take_mixer_updates(&mut self) -> Vec<(u8, MixerProgram)> {
        core::mem::take(&mut self.mixer_updates)
    }
    pub fn step_shaper(&mut self) {
        let t = &mut self.timbres[self.selected];
        if t.shaper_parameters().is_none() {
            return;
        }
        t.shaper_mode = match t.shaper_mode {
            0 => 1,
            1 => t.shaper_ws_mode,
            _ => 0,
        };
        self.update_shaper();
    }
    pub fn set_shaper_mode(&mut self, mode: u8) {
        let t = &mut self.timbres[self.selected];
        if mode as usize >= crate::panel::SHAPER_NAMES.len() || t.shaper_parameters().is_none() {
            return;
        }
        t.shaper_mode = mode;
        if mode >= 2 {
            t.shaper_ws_mode = mode;
        }
        self.update_shaper();
    }
    pub fn set_shaper_position(&mut self, position: u8) {
        let t = &mut self.timbres[self.selected];
        if position as usize >= crate::panel::SHAPER_POSITION_NAMES.len()
            || t.shaper_parameters().is_none()
        {
            return;
        }
        t.shaper_position = position;
        self.update_shaper();
    }
    fn update_shaper(&mut self) {
        if let Some((mode, position, depth)) = self.timbres[self.selected].shaper_parameters()
            && !self.suppress_parameter_commands
            && let Some(player) = &self.player
            && let Err(error) = player.timbre_shaper(self.selected as u8, mode, position, depth)
        {
            self.error = error;
        }
    }
    pub fn step_secondary_waveform(&mut self) {
        let t = &mut self.timbres[self.selected];
        t.secondary.selection = (t.secondary.selection & !3) | ((t.secondary.selection + 1) & 3);
        self.update_secondary();
    }
    pub fn step_secondary_modulation(&mut self) {
        let t = &mut self.timbres[self.selected];
        t.secondary.selection = (t.secondary.selection & 3) | ((t.secondary.selection + 16) & 48);
        self.update_secondary();
    }
    fn update_secondary(&mut self) {
        if !self.suppress_parameter_commands
            && let Some(player) = &self.player
            && let Err(error) =
                player.timbre_secondary(self.selected as u8, self.timbres[self.selected].secondary)
        {
            self.error = error;
        }
    }
    pub fn midi_input(&self) -> Option<radias_synth_infrastructure::audio::NativeInput> {
        self.player.as_ref().map(NativePlayer::input)
    }
    pub fn waveform_name(&self) -> &'static str {
        crate::panel::PRIMARY_WAVEFORM_NAMES
            .get(self.timbres[self.selected].waveform)
            .copied()
            .unwrap_or("?")
    }
    pub fn filter_values(&self) -> (u8, u8, u8) {
        let t = self.timbres[self.selected];
        (t.cutoff, t.resonance, t.filter_type)
    }
    pub fn new(root: &Path, no_audio: bool) -> Self {
        let mut panel = Self {
            player: None,
            controls: None,
            base: None,
            mix_table: None,
            enabled: false,
            timbres: core::array::from_fn(|i| TimbreControls {
                pitch: Default::default(),
                portamento: Default::default(),
                voice_mode: Default::default(),
                voice_group: Default::default(),
                sustain: Default::default(),
                cutoff: 64,
                resonance: 48,
                filter_type: 32,
                filter_route: 0,
                filter2_cutoff: 127,
                filter2_resonance: 0,
                filter2_type: 0,
                filter2_link: false,
                filter2_eg1_intensity: 64,
                filter2_key_tracking: 64,
                shaper_mode: 0,
                shaper_ws_mode: 2,
                shaper_position: 0,
                shaper_depth: 0,
                waveform: 0,
                primary_mode: 0,
                amplifier: AmplifierProgram::default(),
                pan_position: 64,
                mixer_levels: [127, 0, 0],
                secondary: SecondaryProgram::default(),
                primary_controls: [0; 2],
                auxiliary: [ModEnvelopeProgram::default(); 2],
                eg1_intensity: 64,
                key_tracking: 64,
                enabled: i == 0,
                channel: 0,
                modulation: ModulationProgram {
                    lfo: [radias_synth_application::lfo::LfoParameters {
                        frequency: 45,
                        phase_sync: 0x40,
                        ..Default::default()
                    }; 2],
                    ..Default::default()
                },
            }),
            selected: 0,
            error: String::new(),
            tempo: 1200,
            comb_configured: false,
            bank: Vec::new(),
            global_performance: Default::default(),
            drum_kits: Vec::new(),
            drum_editor: None,
            suppress_parameter_commands: false,
            stored_program_selected: None,
            #[cfg(test)]
            comb_updates: Vec::new(),
            #[cfg(test)]
            modulation_updates: Vec::new(),
            #[cfg(test)]
            modulation_header: None,
            #[cfg(test)]
            destination_targets: Vec::new(),
            #[cfg(test)]
            destination_choices: Vec::new(),
            #[cfg(test)]
            destination_viewport: None,
            #[cfg(test)]
            primary_updates: Vec::new(),
            #[cfg(test)]
            mixer_updates: Vec::new(),
            #[cfg(test)]
            amplifier_updates: Vec::new(),
            #[cfg(test)]
            envelope_header: None,
            #[cfg(test)]
            envelope_targets: Vec::new(),
        };
        if !no_audio && let Err(error) = panel.load(root) {
            panel.error = error;
        }
        panel.enabled =
            panel.player.is_some() && std::env::var_os("RADIAS_SCREENSHOT_READY").is_none();
        panel
    }
    fn load(&mut self, root: &Path) -> Result<(), String> {
        let backup =
            fs::read(root.join("firmware/Radias-backup.rdl")).map_err(|e| e.to_string())?;
        self.global_performance =
            radias_synth_infrastructure::rdl::global_performance(&backup).map_err(str::to_owned)?;
        self.drum_kits =
            radias_synth_infrastructure::rdl::drum_kits(&backup).map_err(str::to_owned)?;
        self.bank = radias_synth_infrastructure::rdl::programs(&backup).map_err(str::to_owned)?;
        for timbre in &mut self.timbres {
            timbre.channel = self.global_performance.channel;
        }
        let bytes = fs::read(root.join("assets/native-va/saw.json")).map_err(|e| e.to_string())?;
        let plan = PreparedVoice::from_program_json(&bytes)?;
        self.base = Some(plan.parameters.filter);
        let mut plans = vec![plan];
        for name in ["pulse", "triangle", "sine"] {
            let raw = fs::read(root.join(format!("assets/native-va/{name}.json")))
                .map_err(|e| e.to_string())?;
            plans.push(PreparedVoice::from_program_json(&raw)?);
        }
        let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))
            .map_err(|e| e.to_string())?;
        let tables = MasterTables::from_host_stream(&source).map_err(str::to_owned)?;
        let controls = ControlMap::from_json(
            &fs::read(root.join("assets/native-va/filter-controls.json"))
                .map_err(|e| e.to_string())?,
        )?;
        let system =
            fs::read(root.join("firmware/RADIAS_SYS_0200.bin")).map_err(|e| e.to_string())?;
        self.player = Some(NativePlayer::with_clock(
            plans,
            tables.waveform().map_err(str::to_owned)?,
            Some((
                tables.pitch().map_err(str::to_owned)?,
                tables.bandwidth().map_err(str::to_owned)?,
            )),
            Some(ControllerTables {
                curves: envelope_curves(&system).map_err(str::to_owned)?,
                timing: envelope_timing_tables(&system).map_err(str::to_owned)?,
                amplifier: amplifier_tables(&system).map_err(str::to_owned)?,
            }),
            Some(voice_cost_tables(&system).map_err(str::to_owned)?),
            Some(VoiceModulationTables {
                lfo: lfo_tables(&system).map_err(str::to_owned)?,
                matrix: modulation_tables(&system).map_err(str::to_owned)?,
                pitch: tables.pitch().map_err(str::to_owned)?,
                bandwidth: tables.bandwidth().map_err(str::to_owned)?,
            }),
            Some(lfo_tempo_tables(&system).map_err(str::to_owned)?),
        )?);
        self.mix_table = Some(tables.filter_mix().map_err(str::to_owned)?);
        self.player
            .as_ref()
            .unwrap()
            .configure_performance(self.global_performance)?;
        self.player.as_ref().unwrap().configure_noise(
            tables.pitch().map_err(str::to_owned)?,
            tables.noise_pitch().map_err(str::to_owned)?,
            formant_counter_seeds(&system).map_err(str::to_owned)?,
        )?;
        self.controls = Some(controls);
        self.player
            .as_ref()
            .unwrap()
            .controller_filter_tables(controller_filter_tables(&system).map_err(str::to_owned)?)?;
        self.player
            .as_ref()
            .unwrap()
            .configure_filter2(filter2_control_tables(&system).map_err(str::to_owned)?)?;
        self.player
            .as_ref()
            .unwrap()
            .configure_comb(comb_control_tables(&system).map_err(str::to_owned)?)?;
        self.comb_configured = true;
        self.player
            .as_ref()
            .unwrap()
            .configure_mixer(mixer_scales(&system).map_err(str::to_owned)?)?;
        self.player
            .as_ref()
            .unwrap()
            .configure_secondary(fine_tune_table(&system).map_err(str::to_owned)?)?;
        self.player.as_ref().unwrap().configure_note_pitch(
            radias_synth_infrastructure::firmware::note_pitch_tables(&system)
                .map_err(str::to_owned)?,
            Default::default(),
            0,
        )?;
        self.player.as_ref().unwrap().configure_portamento(
            radias_synth_application::portamento::PortamentoTables {
                rates: radias_synth_infrastructure::firmware::portamento_rates(&system)
                    .map_err(str::to_owned)?,
                curves: radias_synth_infrastructure::firmware::portamento_curves(&system)
                    .map_err(str::to_owned)?,
            },
        )?;
        self.player.as_ref().unwrap().configure_voice_groups(
            radias_synth_infrastructure::firmware::voice_group_tables(&system)
                .map_err(str::to_owned)?,
        )?;
        for i in 0..4 {
            self.selected = i;
            let t = self.timbres[i];
            self.player
                .as_ref()
                .unwrap()
                .timbre_voice_group(i as u8, t.voice_group)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre(i as u8, t.enabled, t.channel)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_pitch(i as u8, t.pitch)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_portamento(i as u8, t.portamento)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_voice_mode(i as u8, t.voice_mode)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_sustain_program(i as u8, t.sustain)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_modulation(i as u8, t.modulation)?;
            self.player
                .as_ref()
                .unwrap()
                .timbre_auxiliary(i as u8, t.auxiliary)?;
            self.update_amplifier();
            self.update_filter();
            self.update_filter_routing();
            self.update_shaper();
        }
        self.player.as_ref().unwrap().configure_pan(
            pan_tables(&system).map_err(str::to_owned)?,
            radias_synth_domain::control_slew::SlewWeights {
                target: tables.word(0x4026).map_err(str::to_owned)? as i16,
                memory: tables.word(0x4027).map_err(str::to_owned)? as i16,
            },
        )?;
        self.selected = 0;
        Ok(())
    }
    pub fn note(&mut self, note: u8, on: bool) {
        if let Some(player) = &self.player
            && let Err(error) = player.input().midi(&[
                0x90 | self.global_performance.channel,
                note,
                if on { 100 } else { 0 },
            ])
        {
            self.error = error;
        }
    }
    pub fn load_stored_program(&mut self, index: usize) -> Result<(), String> {
        let program = self.bank.get(index).ok_or("Stored program absent")?;
        let (controls, mix, base) = match (&self.controls, &self.mix_table, self.base) {
            (Some(c), Some(m), Some(b)) => (c, m, b),
            _ => return Err("Native program tables absent".into()),
        };
        let compiled = radias_synth_infrastructure::stored_program::compile_program(
            program,
            self.global_performance.channel,
            controls,
            mix,
            base,
        )?;
        let player = self.player.as_ref().ok_or("Native output absent")?;
        let drums = if compiled.stored.drum.timbre.is_some() {
            Some(
                radias_synth_infrastructure::stored_program::compile_drum_kit(
                    program,
                    self.drum_kits
                        .get(compiled.stored.drum.kit as usize)
                        .ok_or("Stored drum kit is missing")?
                        .clone(),
                    controls,
                    mix,
                    base,
                )?,
            )
        } else {
            None
        };
        let editor = drums.as_ref().map(|d| {
            let owner = d.program.timbre.unwrap() as usize;
            DrumEditor {
                source: program.clone(),
                kit: d.kit.clone(),
                owner,
                selected: 0,
                views: core::array::from_fn(|i| {
                    TimbreControls::from_stored(radias_synth_application::program::StoredTimbre {
                        controls: d.instruments[i].controls,
                        ..compiled.stored.timbres[owner]
                    })
                }),
            }
        });
        player.load_program_with_drums(compiled, drums)?;
        self.timbres = compiled.stored.timbres.map(TimbreControls::from_stored);
        self.drum_editor = editor;
        if let Some(editor) = &self.drum_editor {
            self.timbres[editor.owner] = editor.views[0];
        }
        self.tempo = compiled.stored.tempo_tenths;
        self.selected = 0;
        self.suppress_parameter_commands = self
            .drum_editor
            .as_ref()
            .is_some_and(|e| e.owner == self.selected);
        self.stored_program_selected = Some(index);
        Ok(())
    }
    pub fn select_stored_program(&mut self, index: usize) -> bool {
        match self.load_stored_program(index) {
            Ok(()) => {
                self.error.clear();
                true
            }
            Err(error) => {
                self.error = error;
                false
            }
        }
    }
    pub fn stored_program_name(&self) -> Option<String> {
        self.stored_program_selected.and_then(|index| {
            self.bank.get(index).map(|p| {
                format!(
                    "{}{:02}  {}",
                    char::from(b'A' + (index / 16) as u8),
                    index % 16 + 1,
                    String::from_utf8_lossy(p.name()).trim_end()
                )
            })
        })
    }
    pub fn stop(&mut self) {
        if let Some(player) = &self.player
            && let Err(error) = player.stop()
        {
            self.error = error;
        }
    }
    pub fn pot(&mut self, ch: usize, mux: usize, value: u16) {
        if !self.enabled {
            return;
        }
        let settings = &mut self.timbres[self.selected];
        match (ch, mux) {
            (2, 7) => {
                settings.portamento.time = (value >> 3).min(127) as u8;
                self.update_portamento();
                return;
            }
            (1, 0) => {
                if settings.shaper_parameters().is_none() {
                    return;
                }
                settings.shaper_depth = (value >> 3).min(127) as u8;
                self.update_shaper();
                return;
            }
            (1, 6) | (1, 3) => {
                if settings.filter_route as usize >= crate::panel::FILTER_ROUTING_NAMES.len()
                    || settings.filter2_type as usize >= crate::panel::FILTER2_NAMES.len()
                {
                    return;
                }
                if settings.filter2_link {
                    if mux == 6 {
                        settings.cutoff = (value >> 3).min(127) as u8;
                    } else {
                        settings.resonance = (value >> 3).min(127) as u8;
                    }
                    self.update_filter();
                    return;
                } else if mux == 6 {
                    settings.filter2_cutoff = (value >> 3).min(127) as u8;
                } else {
                    settings.filter2_resonance = (value >> 3).min(127) as u8;
                }
                self.update_filter_routing();
                return;
            }
            (0, 7) | (0, 6) => {
                if settings.primary_program().is_none() {
                    return;
                }
                settings.primary_controls[7 - mux] = (value >> 3).min(127) as u8;
                self.update_primary();
                return;
            }
            (0, 3) | (0, 2) | (0, 1) => {
                let index = 3 - mux;
                settings.mixer_levels[index] = (value >> 3).min(127) as u8;
                let program = settings.mixer_program();
                #[cfg(test)]
                self.mixer_updates.push((self.selected as u8, program));
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) = player.timbre_mixer(self.selected as u8, program)
                {
                    self.error = error;
                }
                return;
            }
            (0, 5) | (0, 4) => {
                let v = (value >> 3).min(127) as u8;
                if mux == 5 {
                    settings.secondary.pitch.semitone = v;
                } else {
                    settings.secondary.pitch.fine_tune = v;
                }
                self.update_secondary();
                return;
            }
            (1, 1) => {
                settings.pan_position = (value >> 3).min(127) as u8;
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) = player.timbre_pan(
                        self.selected as u8,
                        radias_synth_domain::controller_pan::PanControl {
                            position: settings.pan_position,
                            ..Default::default()
                        },
                    )
                {
                    self.error = error;
                }
                return;
            }
            (1, 2) => {
                settings.amplifier.level = (value >> 3).min(127) as u8;
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) =
                        player.timbre_amplifier_level(self.selected as u8, settings.amplifier.level)
                {
                    self.error = error;
                }
                return;
            }
            (0, 0) => settings.cutoff = (value >> 3).min(127) as u8,
            (1, 5) => settings.resonance = (value >> 3).min(127) as u8,
            (4, 3) => settings.filter_type = (value >> 3).min(127) as u8,
            (1, 7) => settings.eg1_intensity = (value >> 3).min(127) as u8,
            (1, 4) => settings.key_tracking = (value >> 3).min(127) as u8,
            (2, 6) | (2, 5) | (2, 4) | (2, 3) => {
                settings.auxiliary[0].adsr[6 - mux] = (value >> 3).min(127) as u8;
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) =
                        player.timbre_auxiliary(self.selected as u8, settings.auxiliary)
                {
                    self.error = error;
                }
                return;
            }
            (3, 5) | (3, 4) => {
                let index = if mux == 5 { 0 } else { 1 };
                if settings.modulation.lfo[index].phase_sync & 128 != 0 {
                    settings.modulation.tempo_divisions[index] = (value >> 3).min(16) as u8;
                } else {
                    settings.modulation.lfo[index].frequency = (value >> 3).min(127) as u8;
                }
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) =
                        player.timbre_modulation(self.selected as u8, settings.modulation)
                {
                    self.error = error;
                }
                return;
            }
            (2, 2) | (2, 1) | (2, 0) | (3, 7) => {
                let index = match (ch, mux) {
                    (2, 2) => 0,
                    (2, 1) => 1,
                    (2, 0) => 2,
                    _ => 3,
                };
                settings.amplifier.envelope.adsr[index] = (value >> 3).min(127) as u8;
                if !self.suppress_parameter_commands
                    && !self.suppress_parameter_commands
                    && let Some(player) = &self.player
                    && let Err(error) =
                        player.timbre_adsr(self.selected as u8, settings.amplifier.envelope.adsr)
                {
                    self.error = error;
                }
                return;
            }
            _ => return,
        }
        self.update_filter();
    }
    pub fn envelope_values(&self) -> [u8; 4] {
        self.timbres[self.selected].amplifier.envelope.adsr
    }
    pub fn amplifier_level(&self) -> u8 {
        self.timbres[self.selected].amplifier.level
    }
    pub fn pan_position(&self) -> u8 {
        self.timbres[self.selected].pan_position
    }
    pub fn mixer_levels(&self) -> [u8; 3] {
        self.timbres[self.selected].mixer_levels
    }
    pub fn portamento_time(&self) -> u8 {
        self.timbres[self.selected].portamento.time
    }
    pub fn set_voice_mode(&mut self, mode: radias_synth_domain::mono_notes::VoiceMode) {
        self.timbres[self.selected].voice_mode = mode;
        if let Some(player) = &self.player
            && let Err(error) = player.timbre_voice_mode(self.selected as u8, mode)
        {
            self.error = error;
        }
    }
    fn show_voice_mode(&mut self, ui: &mut egui::Ui) {
        use radias_synth_domain::mono_notes::NotePriority;
        let mut mode = self.timbres[self.selected].voice_mode;
        let previous = mode;
        let mut sustain = self.timbres[self.selected].sustain;
        let prior_sustain = sustain;
        let mut pedal = self
            .player
            .as_ref()
            .is_some_and(|p| p.sustain_flags()[self.selected] & 15 != 0);
        let prior_pedal = pedal;
        let mut group = self.timbres[self.selected].voice_group;
        let previous_group = group;
        egui::CollapsingHeader::new("Voice / Mono")
            .id_salt(("native_voice_mode", self.selected))
            .default_open(std::env::var("RADIAS_QA_MONO").as_deref() == Ok("1"))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (value, label) in [(true, "Poly"), (false, "Mono")] {
                        if ui
                            .add_sized(
                                [64.0, 44.0],
                                egui::Button::selectable(mode.polyphonic == value, label),
                            )
                            .clicked()
                        {
                            mode.polyphonic = value;
                        }
                    }
                    ui.add_enabled_ui(!mode.polyphonic, |ui| {
                        ui.add_sized(
                            [144.0, 44.0],
                            egui::Checkbox::new(&mut mode.multi_trigger, "Multi Trigger"),
                        );
                    });
                });
                ui.add_enabled_ui(!mode.polyphonic, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (value, label) in [
                            (NotePriority::Last, "Last"),
                            (NotePriority::Lowest, "Low"),
                            (NotePriority::Highest, "High"),
                        ] {
                            if ui
                                .add_sized(
                                    [64.0, 44.0],
                                    egui::Button::selectable(mode.priority == value, label),
                                )
                                .clicked()
                            {
                                mode.priority = value;
                            }
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(
                        [112.0, 44.0],
                        egui::Checkbox::new(&mut sustain.enabled, "Damper"),
                    );
                    ui.add_sized([96.0, 44.0], egui::Checkbox::new(&mut pedal, "CC64"));
                });
                ui.horizontal_wrapped(|ui| {
                    let mut enabled = group.raw & 128 != 0;
                    let mut count = (group.raw & 15).min(6) + 2;
                    ui.add_sized([112.0, 44.0], egui::Checkbox::new(&mut enabled, "Unison"));
                    ui.add_sized(
                        [112.0, 44.0],
                        egui::DragValue::new(&mut count)
                            .speed(0.04)
                            .range(2..=8)
                            .prefix("Voices "),
                    );
                    group.raw = (group.raw & 0x70) | (count - 2) | if enabled { 128 } else { 0 };
                    ui.add_sized(
                        [112.0, 44.0],
                        egui::DragValue::new(&mut group.detune)
                            .speed(0.15)
                            .range(0..=127)
                            .prefix("Detune "),
                    );
                    ui.add_sized(
                        [112.0, 44.0],
                        egui::DragValue::new(&mut group.spread)
                            .speed(0.15)
                            .range(0..=127)
                            .prefix("Spread "),
                    );
                });
            });
        if mode != previous {
            self.set_voice_mode(mode);
        }
        if group != previous_group {
            self.set_voice_group(group);
        }
        if sustain != prior_sustain {
            self.set_sustain_program(sustain);
        }
        if pedal != prior_pedal {
            if let Some(player) = &self.player {
                if let Err(error) = player.input().midi(&[
                    0xb0 | self.timbres[self.selected].channel,
                    64,
                    if pedal { 127 } else { 0 },
                ]) {
                    self.error = error;
                }
            }
        }
    }
    pub fn set_voice_group(
        &mut self,
        program: radias_synth_domain::voice_group::VoiceGroupProgram,
    ) {
        self.timbres[self.selected].voice_group = program;
        if let Some(player) = &self.player
            && let Err(error) = player.timbre_voice_group(self.selected as u8, program)
        {
            self.error = error;
        }
    }
    pub fn toggle_voice_group(&mut self) {
        let mut program = self.timbres[self.selected].voice_group;
        program.raw ^= 128;
        self.set_voice_group(program);
    }
    pub fn set_sustain_program(&mut self, program: radias_synth_domain::sustain::SustainProgram) {
        self.timbres[self.selected].sustain = program;
        if let Some(player) = &self.player
            && let Err(error) = player.timbre_sustain_program(self.selected as u8, program)
        {
            self.error = error;
        }
    }
    fn update_portamento(&mut self) {
        if let Some(player) = &self.player
            && let Err(error) = player
                .timbre_portamento(self.selected as u8, self.timbres[self.selected].portamento)
        {
            self.error = error;
        }
    }
    pub fn set_portamento(&mut self, program: radias_synth_domain::portamento::PortamentoProgram) {
        self.timbres[self.selected].portamento = program;
        self.update_portamento();
    }
    fn show_portamento(&mut self, ui: &mut egui::Ui) {
        let t = &mut self.timbres[self.selected];
        let previous = t.portamento;
        let screenshot_open = std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_PORTAMENTO").as_deref() == Ok("1");
        egui::CollapsingHeader::new("Portamento")
            .default_open(screenshot_open)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut t.portamento.time)
                            .range(0..=127)
                            .prefix("Time "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut t.portamento.curve)
                            .range(0..=15)
                            .prefix("Curve "),
                    );
                    ui.checkbox(&mut t.portamento.switch_required, "CC65");
                });
            });
        if t.portamento != previous {
            self.update_portamento();
        }
    }
    pub fn secondary_pitch(&self) -> [u8; 2] {
        let p = self.timbres[self.selected].secondary.pitch;
        [p.semitone, p.fine_tune]
    }
    pub fn primary_controls(&self) -> [u8; 2] {
        self.timbres[self.selected].primary_controls
    }
    pub fn auxiliary_values(&self) -> ([u8; 4], u8, u8) {
        let t = self.timbres[self.selected];
        (t.auxiliary[0].adsr, t.eg1_intensity, t.key_tracking)
    }
    pub fn lfo_values(&self) -> [u8; 2] {
        let p = self.timbres[self.selected].modulation;
        core::array::from_fn(|i| {
            if p.lfo[i].phase_sync & 128 != 0 {
                p.tempo_divisions[i].min(16)
            } else {
                p.lfo[i].frequency
            }
        })
    }
    fn show_modulation(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        {
            self.destination_targets.clear();
            self.destination_choices.clear();
            self.destination_viewport = None;
        }
        let t = &mut self.timbres[self.selected];
        let filter2_destinations = self.comb_configured && t.filter2_type < 4;
        let previous = t.modulation;
        let previous_auxiliary = t.auxiliary;
        let section = ui.collapsing("LFO / Virtual Patch", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("EG 3");
                for (value, label) in t.auxiliary[1]
                    .adsr
                    .iter_mut()
                    .zip(["Attack", "Decay", "Sustain", "Release"])
                {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(0..=127)
                            .prefix(format!("{label} ")),
                    );
                }
            });
            for i in 0..2 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("LFO {}", i + 1));
                    let names = if i == 0 {
                        ["Saw", "Square", "Triangle", "S&H"]
                    } else {
                        ["Saw", "Square", "Sine", "S&H"]
                    };
                    let p = &mut t.modulation.lfo[i];
                    let mut synced = p.phase_sync & 128 != 0;
                    if crate::controls::power_toggle(ui, &mut synced, "Tempo Sync").changed() {
                        p.phase_sync = (p.phase_sync & !128) | if synced { 128 } else { 0 };
                    }
                    egui::ComboBox::from_id_salt(("native-lfo-wave", i))
                        .selected_text(names[(p.waveform & 3) as usize])
                        .show_ui(ui, |ui| {
                            for (wave, label) in names.into_iter().enumerate() {
                                ui.selectable_value(&mut p.waveform, wave as u8, label);
                            }
                        });
                    if synced {
                        let division = &mut t.modulation.tempo_divisions[i];
                        egui::ComboBox::from_id_salt(("native-lfo-division", i))
                            .selected_text(TEMPO_DIVISION_LABELS[(*division as usize).min(16)])
                            .show_ui(ui, |ui| {
                                for (value, label) in TEMPO_DIVISION_LABELS.iter().enumerate() {
                                    ui.selectable_value(division, value as u8, *label);
                                }
                            });
                    } else {
                        ui.add(egui::Slider::new(&mut p.frequency, 0..=127).text("Freq"));
                    }
                    ui.add(egui::Slider::new(&mut p.shape, 0..=127).text("Shape"));
                });
            }
            for (i, p) in t.modulation.routes.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Patch {}", i + 1));
                    egui::ComboBox::from_id_salt(("native-patch-source", i))
                        .selected_text(match p.source {
                            0 => "EG 1",
                            1 => "EG 2",
                            2 => "EG 3",
                            3 => "LFO 1",
                            4 => "LFO 2",
                            5 => "Velocity",
                            6 => "Bend",
                            7 => "Wheel",
                            8 => "Key Track",
                            _ => "Source",
                        })
                        .show_ui(ui, |ui| {
                            for (value, label) in [
                                (0, "EG 1"),
                                (1, "EG 2"),
                                (2, "EG 3"),
                                (3, "LFO 1"),
                                (4, "LFO 2"),
                                (5, "Velocity"),
                                (6, "Bend"),
                                (7, "Wheel"),
                                (8, "Key Track"),
                            ] {
                                ui.selectable_value(&mut p.source, value, label);
                            }
                        });
                    let mut destination = p.destination.index() as u8;
                    let destination_combo =
                        egui::ComboBox::from_id_salt(("native-patch-destination", i))
                            .selected_text(match destination {
                                0 => "Pitch",
                                1 => "OSC 2 Pitch",
                                2 => "OSC 1 Control 1",
                                16 => "OSC 1 Control 2",
                                3 => "OSC 1 Level",
                                4 => "OSC 2 Level",
                                5 => "Noise Level",
                                7 => "Cutoff",
                                9 => "Filter 2 Cutoff (Comb)",
                                10 => "DRIVE / WS Depth",
                                11 => "AMP",
                                12 => "Pan",
                                13 => "LFO 1 Rate",
                                14 => "LFO 2 Rate",
                                15 => "Portamento Time",
                                17 => "EG 1 INT",
                                18 => "Key Track",
                                19 => "Filter 2 Resonance (Comb)",
                                20 => "Filter 2 EG 1 Depth (Comb)",
                                21 => "Filter 2 Key Track (Comb)",
                                _ => "Destination",
                            })
                            .show_ui(ui, |ui| {
                                #[cfg(test)]
                                {
                                    self.destination_viewport = Some(ui.clip_rect());
                                }
                                for (value, label) in [
                                    (0, "Pitch"),
                                    (1, "OSC 2 Pitch"),
                                    (2, "OSC 1 Control 1"),
                                    (16, "OSC 1 Control 2"),
                                    (3, "OSC 1 Level"),
                                    (4, "OSC 2 Level"),
                                    (5, "Noise Level"),
                                    (7, "Cutoff"),
                                    (9, "Filter 2 Cutoff (Comb)"),
                                    (10, "DRIVE / WS Depth"),
                                    (11, "AMP"),
                                    (12, "Pan"),
                                    (13, "LFO 1 Rate"),
                                    (14, "LFO 2 Rate"),
                                    (15, "Portamento Time"),
                                    (17, "EG 1 INT"),
                                    (18, "Key Track"),
                                    (19, "Filter 2 Resonance (Comb)"),
                                    (20, "Filter 2 EG 1 Depth (Comb)"),
                                    (21, "Filter 2 Key Track (Comb)"),
                                ] {
                                    if matches!(value, 9 | 19 | 20 | 21) && !filter2_destinations {
                                        continue;
                                    }
                                    let option =
                                        ui.selectable_value(&mut destination, value, label);
                                    #[cfg(test)]
                                    self.destination_choices.push((i, value, option.rect));
                                    #[cfg(not(test))]
                                    let _ = option;
                                }
                            });
                    #[cfg(test)]
                    self.destination_targets
                        .push((i, destination_combo.response.rect));
                    #[cfg(not(test))]
                    let _ = destination_combo;
                    p.destination =
                        radias_synth_domain::modulation::ModulationDestination::new(destination)
                            .unwrap();
                    let mut depth = p.intensity as i16 - 64;
                    ui.add(egui::Slider::new(&mut depth, -63..=63).text("Depth"));
                    p.intensity = (depth + 64) as u8;
                });
            }
        });
        #[cfg(test)]
        {
            self.modulation_header = Some(section.header_response.rect);
        }
        #[cfg(not(test))]
        let _ = section;
        if previous != t.modulation {
            #[cfg(test)]
            self.modulation_updates
                .push((self.selected as u8, t.modulation));
            if !self.suppress_parameter_commands
                && !self.suppress_parameter_commands
                && let Some(player) = &self.player
                && let Err(error) = player.timbre_modulation(self.selected as u8, t.modulation)
            {
                self.error = error;
            }
        }
        if previous_auxiliary != t.auxiliary
            && !self.suppress_parameter_commands
            && let Some(player) = &self.player
            && let Err(error) = player.timbre_auxiliary(self.selected as u8, t.auxiliary)
        {
            self.error = error;
        }
    }
    fn update_amplifier(&mut self) {
        let program = self.timbres[self.selected].amplifier;
        #[cfg(test)]
        self.amplifier_updates.push((self.selected as u8, program));
        if !self.suppress_parameter_commands
            && let Some(player) = &self.player
            && let Err(error) = player.timbre_amplifier_program(self.selected as u8, program)
        {
            self.error = error;
        }
    }
    pub fn set_eg2_program(&mut self, envelope: ModEnvelopeProgram) {
        self.timbres[self.selected].amplifier.envelope = envelope;
        self.update_amplifier();
    }
    fn show_envelopes(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        self.envelope_targets.clear();
        let t = &mut self.timbres[self.selected];
        let previous_amp = t.amplifier;
        let previous_aux = t.auxiliary;
        let screenshot_open = std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_EG").as_deref() == Ok("1");
        let _section = egui::CollapsingHeader::new("EG / Velocity")
            .default_open(screenshot_open)
            .show(ui, |ui| {
                let [eg1, eg3] = &mut t.auxiliary;
                for (index, envelope) in [eg1, &mut t.amplifier.envelope, eg3]
                    .into_iter()
                    .enumerate()
                {
                    ui.push_id(("native-envelope", index), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("EG {}", index + 1));
                            for (_field, label, value) in [
                                (0, "Curve", &mut envelope.curve),
                                (
                                    1,
                                    "Velocity Level",
                                    &mut envelope.velocity_level_sensitivity,
                                ),
                                (2, "Velocity Time", &mut envelope.velocity_time_sensitivity),
                                (3, "Key Track", &mut envelope.key_tracking),
                            ] {
                                let _response = ui.add(
                                    egui::DragValue::new(value)
                                        .range(0..=if label == "Curve" { 4 } else { 127 })
                                        .prefix(format!("{label} ")),
                                );
                                #[cfg(test)]
                                self.envelope_targets.push((
                                    index,
                                    _field,
                                    _response.id,
                                    _response.rect,
                                ));
                            }
                        });
                    });
                }
                let _response = ui.add(
                    egui::DragValue::new(&mut t.amplifier.key_tracking)
                        .range(0..=127)
                        .prefix("AMP Key Track "),
                );
                #[cfg(test)]
                self.envelope_targets
                    .push((1, 4, _response.id, _response.rect));
            });
        #[cfg(test)]
        {
            self.envelope_header = Some(_section.header_response.rect);
        }
        if previous_amp != t.amplifier {
            self.update_amplifier();
        }
        let auxiliary = self.timbres[self.selected].auxiliary;
        if previous_aux != auxiliary
            && !self.suppress_parameter_commands
            && let Some(player) = &self.player
            && let Err(error) = player.timbre_auxiliary(self.selected as u8, auxiliary)
        {
            self.error = error;
        }
    }
    fn show_pitch(&mut self, ui: &mut egui::Ui) {
        let t = &mut self.timbres[self.selected];
        let previous = t.pitch;
        let screenshot_open = std::env::var_os("EFRAME_SCREENSHOT_TO").is_some()
            && std::env::var("RADIAS_QA_PITCH").as_deref() == Ok("1");
        egui::CollapsingHeader::new("Pitch / MIDI")
            .default_open(screenshot_open)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, value) in [
                        ("Transpose", &mut t.pitch.transpose),
                        ("Tune", &mut t.pitch.fine_tune),
                        ("Vibrato", &mut t.pitch.vibrato_intensity),
                        ("Bend Range", &mut t.pitch.bend_range),
                    ] {
                        let label_width = ui
                            .painter()
                            .layout_no_wrap(
                                label.into(),
                                egui::TextStyle::Body.resolve(ui.style()),
                                ui.visuals().text_color(),
                            )
                            .size()
                            .x;
                        let height = ui.spacing().interact_size.y;
                        ui.allocate_ui_with_layout(
                            egui::vec2(label_width + ui.spacing().item_spacing.x + 44.0, height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(label).extend());
                                let mut signed = *value as i16 - 64;
                                if ui
                                    .add_sized(
                                        [44.0, height],
                                        egui::DragValue::new(&mut signed).range(-64..=63),
                                    )
                                    .changed()
                                {
                                    *value = (signed + 64) as u8;
                                }
                            },
                        );
                    }
                    ui.checkbox(&mut t.pitch.bend_enabled, "Bend");
                    ui.checkbox(&mut t.pitch.wheel_enabled, "Wheel");
                });
            });
        let current = t.pitch;
        if current != previous {
            self.set_pitch_program(current);
        }
    }
    pub fn set_pitch_program(&mut self, program: radias_synth_domain::note_pitch::PitchProgram) {
        self.timbres[self.selected].pitch = program;
        if let Some(player) = &self.player
            && let Err(error) = player.timbre_pitch(self.selected as u8, program)
        {
            self.error = error;
        }
    }
    fn update_filter(&mut self) {
        if !self.suppress_parameter_commands
            && let (Some(player), Some(map), Some(base)) = (&self.player, &self.controls, self.base)
        {
            let t = self.timbres[self.selected];
            match map.filter(base, t.cutoff, t.resonance) {
                Ok(mut coefficients) => {
                    if let Some(table) = &self.mix_table {
                        coefficients.mix = table.weights((t.filter_type as u16) << 8);
                    }
                    if let Err(error) = player.timbre_filter(self.selected as u8, coefficients) {
                        self.error = error;
                    }
                    let filter = DynamicFilter {
                        input: radias_synth_domain::controller_filter::ControllerFilter {
                            cutoff: t.cutoff,
                            eg1_intensity: t.eg1_intensity,
                            key_tracking: t.key_tracking,
                            ..Default::default()
                        },
                        resonance: map.resonances[t.resonance as usize],
                        normalization: map.normalization,
                        base: coefficients,
                    };
                    if let Err(error) = player.timbre_dynamic_filter(self.selected as u8, filter) {
                        self.error = error;
                    }
                }
                Err(error) => self.error = error.into(),
            }
        }
        self.update_filter_routing();
    }
    pub fn show(&mut self, ui: &mut egui::Ui, gain: f32) -> bool {
        let previous = self.enabled;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.player.is_some(), |ui| {
                crate::controls::power_toggle(ui, &mut self.enabled, "Native VA · 24 голоса");
            });
            if self.enabled {
                let previous_waveform = self.timbres[self.selected].waveform;
                let mut waveform = previous_waveform;
                egui::ComboBox::from_id_salt("native-wave")
                    .selected_text(self.waveform_name())
                    .show_ui(ui, |ui| {
                        for (i, label) in crate::panel::PRIMARY_WAVEFORM_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut waveform, i, *label);
                        }
                    });
                if previous_waveform != waveform {
                    self.set_waveform(waveform);
                }
                if ui.button("C4").clicked() {
                    self.note(60, true);
                }
                if ui.button("Стоп VA").clicked() {
                    self.stop();
                }
            }
        });
        if self.enabled {
            ui.horizontal_wrapped(|ui| {
                ui.label("TIMBRE");
                for i in 0..4 {
                    crate::controls::timbre_selector(
                        ui,
                        &mut self.selected,
                        i,
                        self.timbres[i].enabled,
                    );
                }
                let t = &mut self.timbres[self.selected];
                let previous = (t.enabled, t.channel);
                crate::controls::power_toggle(ui, &mut t.enabled, "Timbre");
                egui::ComboBox::from_id_salt("native-channel")
                    .selected_text(format!("MIDI {}", t.channel + 1))
                    .show_ui(ui, |ui| {
                        for channel in 0..16 {
                            ui.selectable_value(&mut t.channel, channel, (channel + 1).to_string());
                        }
                    });
                if previous != (t.enabled, t.channel)
                    && let Some(player) = &self.player
                    && let Err(error) = player.timbre(self.selected as u8, t.enabled, t.channel)
                {
                    self.error = error;
                }
            });
            ui.horizontal(|ui| {
                ui.label("BPM");
                let mut bpm = self.tempo as f32 / 10.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut bpm)
                            .range(20.0..=300.0)
                            .speed(0.1)
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    self.tempo = (bpm * 10.0).round() as u16;
                    if let Some(player) = &self.player
                        && let Err(error) = player.tempo(self.tempo)
                    {
                        self.error = error;
                    }
                }
            });
        }
        if previous && !self.enabled {
            self.stop();
        }
        if self.enabled {
            self.show_pitch(ui);
            self.show_portamento(ui);
            self.show_voice_mode(ui);
            self.show_envelopes(ui);
            self.show_modulation(ui);
        }
        if let Some(player) = &self.player {
            player.gain(gain);
            if self.enabled {
                let status = player.status();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} · {} Гц", status.device, status.sample_rate));
                    ui.label(format!(
                        "буфер: {:.3} мс",
                        status.worst_render_ns as f64 / 1e6
                    ));
                    ui.label(format!("пропусков: {}", status.deadline_misses));
                    ui.label(format!("звуковых кадров: {}", status.audible_frames));
                    ui.label(format!("голосов: {}/24", status.active_voices));
                });
                let filters = match self.timbres[self.selected].filter_route {
                    0 => "Filter 1 / EG 1",
                    1 => "Filter 1 / EG 1 > Filter 2",
                    2 => "Filter 1 / EG 1 + Filter 2",
                    _ => "Filter 1 / EG 1 | Filter 2 (Individual)",
                };
                ui.small(format!(
                    "OSC 1 / OSC 2 > Mixer > {filters} > EG 2 / AMP · EG 3 / Virtual Patch"
                ));
                let t = self.timbres[self.selected];
                if t.filter2_type == 3 {
                    ui.small(format!(
                        "Comb · LINK {}",
                        if t.filter2_link { "ON" } else { "OFF" }
                    ));
                }
                if t.shaper_mode != 0 && t.shaper_parameters().is_some() {
                    ui.small(format!(
                        "{} · {} · Depth {}",
                        crate::panel::SHAPER_NAMES[t.shaper_mode as usize],
                        crate::panel::SHAPER_POSITION_NAMES[t.shaper_position as usize],
                        t.shaper_depth
                    ));
                }
            }
        }
        if !self.error.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(241, 114, 115), &self.error);
        }
        previous != self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::NativePanel;

    #[test]
    fn complete_eg2_controls_deliver_distinct_sensitivities_for_each_timbre_at_narrow_width() {
        use eframe::egui;
        fn frame(ctx: &egui::Context, native: &mut NativePanel, events: Vec<egui::Event>) {
            ctx.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(390.0, 844.0),
                    )),
                    ..Default::default()
                },
                |ui| native.show_envelopes(ui),
            )
            .drop_without_applying_deltas();
        }
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        let ctx = egui::Context::default();
        frame(&ctx, &mut native, vec![]);
        let center = native.envelope_header.unwrap().center();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut native,
                vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        for _ in 0..32 {
            frame(&ctx, &mut native, vec![]);
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            for (field, value) in [
                (0, (timbre as u8 + 2) % 5),
                (1, 32 + timbre as u8 * 16),
                (2, 96 - timbre as u8 * 16),
                (3, 40 + timbre as u8 * 16),
                (4, 52 + timbre as u8 * 8),
            ] {
                frame(&ctx, &mut native, vec![]);
                let (_, _, id, rect) = *native
                    .envelope_targets
                    .iter()
                    .find(|(eg, f, _, _)| *eg == 1 && *f == field)
                    .unwrap();
                assert!(
                    rect.min.x >= 0.0 && rect.max.x <= 390.0,
                    "EG2 field exceeds narrow viewport:{rect:?}"
                );
                frame(
                    &ctx,
                    &mut native,
                    vec![egui::Event::AccessKitActionRequest(
                        egui::accesskit::ActionRequest {
                            action: egui::accesskit::Action::SetValue,
                            target_node: id.accesskit_id(),
                            target_tree: egui::accesskit::TreeId::ROOT,
                            data: Some(egui::accesskit::ActionData::NumericValue(value as f64)),
                        },
                    )],
                );
                let (_, program) = *native
                    .amplifier_updates
                    .last()
                    .expect("EG2 value must reach the player adapter");
                assert_eq!(native.amplifier_updates.last().unwrap().0, timbre as u8);
                let actual = [
                    program.envelope.curve,
                    program.envelope.velocity_level_sensitivity,
                    program.envelope.velocity_time_sensitivity,
                    program.envelope.key_tracking,
                    program.key_tracking,
                ];
                assert_eq!(actual[field], value);
            }
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            let p = native.timbres[timbre].amplifier.envelope;
            assert_eq!(
                [
                    p.curve,
                    p.velocity_level_sensitivity,
                    p.velocity_time_sensitivity,
                    p.key_tracking
                ],
                [
                    (timbre as u8 + 2) % 5,
                    32 + timbre as u8 * 16,
                    96 - timbre as u8 * 16,
                    40 + timbre as u8 * 16
                ]
            );
            assert_eq!(p.adsr, [0, 0, 127, 10]);
            assert_eq!(
                native.timbres[timbre].amplifier.key_tracking,
                52 + timbre as u8 * 8
            );
        }
    }

    #[test]
    fn primary_controls_accept_all_basic_waveforms_and_keep_timbres_independent() {
        // Exercise the production UI adapter without CoreAudio or firmware assets.
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for index in 0..4 {
            native.select_timbre(index);
            for waveform in 0..4 {
                assert_eq!(native.panel_state().waveform, waveform);
                native.pot(0, 7, ((index * 8 + waveform + 17) as u16) << 3);
                native.pot(0, 6, ((90 - index * 8 - waveform) as u16) << 3);
                assert_eq!(
                    native.primary_controls(),
                    [
                        (index * 8 + waveform + 17) as u8,
                        (90 - index * 8 - waveform) as u8,
                    ]
                );
                native.step_waveform(1);
            }
            native.set_waveform(0);
            assert_eq!(native.panel_state().waveform, 0);
        }
        for index in 0..4 {
            native.select_timbre(index);
            assert_eq!(
                native.primary_controls(),
                [(index * 8 + 20) as u8, (87 - index * 8) as u8]
            );
        }
        let previous = native.primary_controls();
        for waveform in [6, 7, 16] {
            native.timbres[native.selected].waveform = waveform;
            native.pot(0, 7, 127 << 3);
            native.pot(0, 6, 127 << 3);
            assert_eq!(native.primary_controls(), previous);
        }
    }

    #[test]
    fn noise_formant_controls_forward_primary_and_mixer_without_indexing_va_templates() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            for waveform in [4, 5] {
                native.take_primary_updates();
                native.set_waveform(waveform);
                let selected = native.take_primary_updates();
                assert_eq!(
                    (selected[0].0, selected[0].1.selection),
                    (timbre as u8, waveform as u8)
                );
                assert_eq!(
                    native.waveform_name(),
                    if waveform == 4 { "Noise" } else { "Formant" }
                );
                native.pot(0, 7, ((29 + timbre * 8 + waveform) as u16) << 3);
                native.pot(0, 6, ((94 - timbre * 8 - waveform) as u16) << 3);
                let updates = native.take_primary_updates();
                assert_eq!(updates.len(), 2);
                let (index, program) = updates[1];
                assert_eq!((index, program.selection), (timbre as u8, waveform as u8));
                assert_eq!(
                    [program.control.control1, program.control.control2],
                    [
                        (29 + timbre * 8 + waveform) as u8,
                        (94 - timbre * 8 - waveform) as u8
                    ]
                );
                assert_eq!(native.panel_state().primary_mode, 0);
                assert_eq!(
                    native.timbres[timbre].mixer_program().selections[0],
                    waveform as u8
                );
                native.take_mixer_updates();
                native.pot(0, 1, ((41 + timbre * 16) as u16) << 3);
                let mixer = native.take_mixer_updates();
                assert_eq!(mixer.len(), 1);
                assert_eq!(
                    (mixer[0].0, mixer[0].1.selections[0]),
                    (timbre as u8, waveform as u8)
                );
                assert_eq!(mixer[0].1.levels, [127, 0, (41 + timbre * 16) as u8]);
            }
            native.set_waveform(4 + (timbre & 1));
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.panel_state().waveform, 4 + (timbre & 1));
            assert_eq!(
                native.primary_controls(),
                [(34 + timbre * 8) as u8, (89 - timbre * 8) as u8]
            );
            assert_eq!(native.mixer_levels()[2], (41 + timbre * 16) as u8);
            native.take_primary_updates();
            native.step_primary_modulation();
            let updated = native.take_primary_updates();
            assert_eq!(updated.last().unwrap().1.selection, 16);
            assert_eq!(native.panel_state().waveform, 0);
            assert_eq!(native.panel_state().primary_mode, 1);
            native.set_waveform(5);
            assert_eq!(native.panel_state().primary_mode, 0);
            assert_eq!(
                native.timbres[timbre].primary_program().unwrap().selection,
                5
            );
            native.set_waveform(6);
            assert_eq!(native.panel_state().waveform, 5);
            native.step_waveform(1);
            assert_eq!(native.panel_state().waveform, 0);
            native.step_waveform(-1);
            assert_eq!(native.panel_state().waveform, 5);
        }
    }

    #[test]
    fn cross_selection_survives_controls_mixer_waveform_and_timbre_changes() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for index in 0..4 {
            native.select_timbre(index);
            native.step_primary_modulation();
            for waveform in 0..4 {
                assert_eq!(native.panel_state().primary_mode, 1);
                assert_eq!(native.panel_state().waveform, waveform);
                native.pot(0, 7, ((index * 8 + waveform + 17) as u16) << 3);
                native.pot(0, 6, ((90 - index * 8 - waveform) as u16) << 3);
                native.pot(0, 3, 101 << 3);
                native.pot(0, 2, 31 << 3);
                let timbre = native.timbres[index];
                let program = timbre.primary_program().unwrap();
                assert_eq!(program.selection, 16 | waveform as u8);
                assert_eq!(program.control.control1, (index * 8 + waveform + 17) as u8);
                assert_eq!(program.control.control2, (90 - index * 8 - waveform) as u8);
                assert_eq!(timbre.mixer_program().selections[0], program.selection);
                assert_eq!(timbre.mixer_program().levels, [101, 31, 0]);
                native.step_waveform(1);
            }
            assert_eq!(native.panel_state().waveform, 0);
            assert_eq!(native.panel_state().primary_mode, 1);
        }
        native.select_timbre(0);
        // The supported cycle passes through OSC1 Unison/VPM before Waveform.
        native.step_primary_modulation();
        native.step_primary_modulation();
        native.step_primary_modulation();
        for index in 0..4 {
            native.select_timbre(index);
            assert_eq!(native.panel_state().primary_mode, u8::from(index != 0));
            assert_eq!(
                native.timbres[index].primary_program().unwrap().selection,
                if index == 0 { 0 } else { 16 }
            );
            assert_eq!(
                native.primary_controls(),
                [(index * 8 + 20) as u8, (87 - index * 8) as u8]
            );
        }
        let original_controls = native.primary_controls();
        for mode in [4, 5] {
            native.timbres[native.selected].primary_mode = mode;
            native.pot(0, 7, 127 << 3);
            native.pot(0, 6, 127 << 3);
            native.step_primary_modulation();
            assert_eq!(native.primary_controls(), original_controls);
            assert_eq!(native.panel_state().primary_mode, mode);
            assert!(native.timbres[native.selected].primary_program().is_none());
        }
    }

    #[test]
    fn unison_control_and_mixer_programs_retain_phase_value_and_mode_for_all_timbres() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for index in 0..4 {
            native.select_timbre(index);
            native.step_primary_modulation();
            native.step_primary_modulation();
            for waveform in 0..4 {
                assert_eq!(native.panel_state().primary_mode, 2);
                assert_eq!(native.panel_state().waveform, waveform);
                native.pot(0, 7, ((index * 8 + waveform + 25) as u16) << 3);
                // INITIAL PHASE is retained in the controller program for note-on;
                // this UI adapter does not manufacture a note retrigger.
                native.pot(0, 6, ((101 - index * 8 - waveform) as u16) << 3);
                native.pot(0, 3, 91 << 3);
                native.pot(0, 2, 17 << 3);
                let t = native.timbres[index];
                let program = t.primary_program().unwrap();
                assert_eq!(program.selection, 32 | waveform as u8);
                assert_eq!(program.control.control1, (index * 8 + waveform + 25) as u8);
                assert_eq!(program.control.control2, (101 - index * 8 - waveform) as u8);
                assert_eq!(t.mixer_program().selections[0], program.selection);
                assert_eq!(t.mixer_program().levels, [91, 17, 0]);
                native.step_waveform(1);
            }
            assert_eq!(native.panel_state().waveform, 0);
        }
        native.select_timbre(0);
        native.step_primary_modulation();
        native.step_primary_modulation();
        for index in 0..4 {
            native.select_timbre(index);
            assert_eq!(
                native.panel_state().primary_mode,
                if index == 0 { 0 } else { 2 }
            );
            assert_eq!(
                native.primary_controls(),
                [(index * 8 + 28) as u8, (98 - index * 8) as u8]
            );
        }
    }

    #[test]
    fn vpm_controls_and_mixer_retain_raw_selection_for_each_timbre() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for index in 0..4 {
            native.select_timbre(index);
            for _ in 0..3 {
                native.step_primary_modulation();
            }
            for waveform in 0..4 {
                assert_eq!(native.panel_state().primary_mode, 3);
                assert_eq!(native.panel_state().waveform, waveform);
                native.pot(0, 7, ((index * 8 + waveform + 31) as u16) << 3);
                native.pot(0, 6, ((112 - index * 8 - waveform) as u16) << 3);
                native.pot(0, 3, 93 << 3);
                native.pot(0, 2, 21 << 3);
                let t = native.timbres[index];
                let program = t.primary_program().unwrap();
                assert_eq!(program.selection, 48 | waveform as u8);
                assert_eq!(program.control.control1, (index * 8 + waveform + 31) as u8);
                assert_eq!(program.control.control2, (112 - index * 8 - waveform) as u8);
                assert_eq!(t.mixer_program().selections[0], program.selection);
                assert_eq!(t.mixer_program().levels, [93, 21, 0]);
                native.step_waveform(1);
            }
        }
        native.select_timbre(0);
        native.step_primary_modulation();
        for index in 0..4 {
            native.select_timbre(index);
            assert_eq!(
                native.panel_state().primary_mode,
                if index == 0 { 0 } else { 3 }
            );
            assert_eq!(
                native.primary_controls(),
                [(index * 8 + 34) as u8, (109 - index * 8) as u8]
            );
        }
    }

    #[test]
    fn filter2_coefficients_use_own_controls_and_three_outputs_per_timbre() {
        use radias_synth_domain::{filter::FilterCoefficients, filter_routing::Filter2Output};
        use radias_synth_infrastructure::prepared::ControlMap;
        let controls = ControlMap {
            frequencies: core::array::from_fn(|i| 0x01000000 + (i as i32 * 65536)),
            resonances: core::array::from_fn(|i| 0x10000000 + i as i32 * 4096),
            input_gains: core::array::from_fn(|i| 1024 + i as i16 * 16),
            normalization: 0x40000000,
        };
        let base = FilterCoefficients {
            input_gain: 16384,
            feedback: 0,
            integrator_gain: 0,
            post_gain: 0,
            post_feedback: 0,
            mix: [0; 5],
        };
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.panel_state().filter2, [127, 0]);
            let values = [27 + timbre as u8 * 8, 91 - timbre as u8 * 8];
            native.pot(1, 6, (values[0] as u16) << 3);
            native.pot(1, 3, (values[1] as u16) << 3);
            let expected = controls.filter(base, values[0], values[1]).unwrap();
            for route in 0..4 {
                assert_eq!(native.panel_state().filter_route, route);
                for output in [
                    Filter2Output::LowPass,
                    Filter2Output::HighPass,
                    Filter2Output::BandPass,
                ] {
                    let compiled = native.timbres[timbre]
                        .filter2_coefficients(&controls, base)
                        .unwrap();
                    assert_eq!(compiled.output, output);
                    assert_eq!(compiled.input_gain, expected.input_gain);
                    assert_eq!(compiled.feedback, expected.feedback);
                    assert_eq!(compiled.integrator_gain, expected.integrator_gain);
                    native.step_filter2_type();
                }
                assert_eq!(native.panel_state().filter2_type, 3);
                assert!(native.timbres[timbre].comb_program().is_some());
                assert!(
                    native.timbres[timbre]
                        .filter2_coefficients(&controls, base)
                        .is_err()
                );
                native.step_filter2_type();
                assert_eq!(native.panel_state().filter2_type, 0);
                native.step_filter_routing();
            }
            assert_eq!(native.filter_values(), (64, 48, 32));
            native.step_filter_routing();
            native.step_filter2_type();
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.panel_state().filter_route, 1);
            assert_eq!(native.panel_state().filter2_type, 1);
            assert_eq!(
                native.panel_state().filter2,
                [27 + timbre as u8 * 8, 91 - timbre as u8 * 8]
            );
        }
        native.timbres[native.selected].filter2_type = 4;
        let previous = native.panel_state().filter2;
        assert!(
            native.timbres[native.selected]
                .filter2_coefficients(&controls, base)
                .is_err()
        );
        native.pot(1, 6, 0);
        native.pot(1, 3, 0);
        native.step_filter2_type();
        native.step_filter_routing();
        assert_eq!(native.panel_state().filter2, previous);
        assert_eq!(native.panel_state().filter2_type, 4);
        assert_eq!(native.panel_state().filter_route, 1);
    }

    #[test]
    fn comb_link_uses_f1_controls_retains_own_f2_and_forwards_each_edit() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            native.set_filter2_link(true);
            assert!(
                native.panel_state().filter2_link,
                "LINK must also work for regular Filter2 outputs"
            );
            native.set_filter2_link(false);
            let own = [109 - timbre as u8 * 8, 25 + timbre as u8 * 8];
            native.pot(1, 6, (own[0] as u16) << 3);
            native.pot(1, 3, (own[1] as u16) << 3);
            for _ in 0..3 {
                native.step_filter2_type();
            }
            let first = [31 + timbre as u8 * 8, 92 - timbre as u8 * 8];
            native.pot(0, 0, (first[0] as u16) << 3);
            native.pot(1, 5, (first[1] as u16) << 3);
            native.pot(1, 7, 83 << 3);
            native.pot(1, 4, 37 << 3);
            for route in 0..4 {
                native.set_filter2_link(false);
                assert_eq!(native.panel_state().filter2, own);
                native.take_comb_updates();
                native.set_filter2_link(true);
                let updates = native.take_comb_updates();
                assert_eq!(updates.len(), 1);
                let (index, actual_route, program) = updates[0];
                assert_eq!((index, actual_route), (timbre as u8, route));
                assert_eq!([program.cutoff.cutoff, program.resonance.resonance], own);
                assert!(program.cutoff.link && program.resonance.link);
                assert_eq!(program.cutoff.linked_eg1_intensity, 83);
                assert_eq!(program.linked_key_tracking, 37);
                assert_eq!(program.cutoff.key_offset, 0);
                assert_eq!(program.cutoff.eg1_intensity, 64);
                assert_eq!(program.key_tracking, 64);
                let edited = [71 + route * 8, 62 - route * 8];
                native.pot(1, 6, (edited[0] as u16) << 3);
                native.pot(1, 3, (edited[1] as u16) << 3);
                assert_eq!(native.panel_state().filter2, edited);
                assert_eq!(native.filter_values(), (edited[0], edited[1], 32));
                let updates = native.take_comb_updates();
                assert_eq!(
                    updates.len(),
                    2,
                    "Both linked F2 edits must reach the Comb API"
                );
                assert_eq!((updates[1].0, updates[1].1), (timbre as u8, route));
                assert_eq!(updates[1].2.cutoff.linked_cutoff, edited[0]);
                assert_eq!(updates[1].2.resonance.linked_resonance, edited[1]);
                assert_eq!(native.timbres[timbre].filter2_cutoff, own[0]);
                assert_eq!(native.timbres[timbre].filter2_resonance, own[1]);
                native.step_filter_routing();
                assert_eq!(
                    native.take_comb_updates().last().unwrap().1,
                    (route + 1) % 4
                );
            }
            native.pot(0, 0, 117 << 3);
            native.pot(1, 5, 19 << 3);
            native.pot(1, 7, 93 << 3);
            native.pot(1, 4, 51 << 3);
            let updates = native.take_comb_updates();
            assert_eq!(
                updates.len(),
                4,
                "F1 cutoff/resonance/EG1/key edits all update linked Comb"
            );
            let program = updates.last().unwrap().2;
            assert_eq!(program.cutoff.linked_cutoff, 117);
            assert_eq!(program.resonance.linked_resonance, 19);
            assert_eq!(program.cutoff.linked_eg1_intensity, 93);
            assert_eq!(program.linked_key_tracking, 51);
            native.step_filter2_type();
            assert_eq!(native.panel_state().filter2_type, 0);
            assert_eq!(native.panel_state().filter2, [117, 19]);
            assert!(
                native.panel_state().filter2_link,
                "Saved LINK survives a temporary type change"
            );
            native.set_filter2_link(false);
            assert!(
                !native.panel_state().filter2_link,
                "Regular Filter2 LINK OFF restores its own parameters"
            );
            assert_eq!(native.panel_state().filter2, own);
            native.set_filter2_link(true);
            for _ in 0..3 {
                native.step_filter2_type();
            }
            assert_eq!(native.panel_state().filter2, [117, 19]);
            native.set_filter2_link(false);
            assert_eq!(native.panel_state().filter2, own);
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.panel_state().filter2_type, 3);
            assert!(!native.panel_state().filter2_link);
            assert_eq!(
                native.panel_state().filter2,
                [109 - timbre as u8 * 8, 25 + timbre as u8 * 8]
            );
        }
    }

    #[test]
    fn comb_virtual_patch_choices_require_configured_comb_and_forward_real_menu_input() {
        fn frame(
            ctx: &eframe::egui::Context,
            native: &mut NativePanel,
            events: Vec<eframe::egui::Event>,
        ) {
            ctx.run_ui(
                eframe::egui::RawInput {
                    events,
                    screen_rect: Some(eframe::egui::Rect::from_min_size(
                        eframe::egui::Pos2::ZERO,
                        eframe::egui::vec2(1440.0, 1000.0),
                    )),
                    ..Default::default()
                },
                |ui| native.show_modulation(ui),
            )
            .drop_without_applying_deltas();
        }
        fn press(pos: eframe::egui::Pos2, pressed: bool) -> eframe::egui::Event {
            eframe::egui::Event::PointerButton {
                pos,
                pressed,
                button: eframe::egui::PointerButton::Primary,
                modifiers: Default::default(),
            }
        }
        fn click(ctx: &eframe::egui::Context, native: &mut NativePanel, pos: eframe::egui::Pos2) {
            frame(ctx, native, vec![eframe::egui::Event::PointerMoved(pos)]);
            frame(ctx, native, vec![press(pos, true)]);
            frame(ctx, native, vec![press(pos, false)]);
        }
        fn open_destination(ctx: &eframe::egui::Context, native: &mut NativePanel) {
            frame(ctx, native, vec![]);
            let center = native
                .destination_targets
                .iter()
                .find(|(index, _)| *index == 0)
                .unwrap()
                .1
                .center();
            click(ctx, native, center);
            frame(ctx, native, vec![]);
        }
        fn escape(ctx: &eframe::egui::Context, native: &mut NativePanel) {
            for pressed in [true, false] {
                frame(
                    ctx,
                    native,
                    vec![eframe::egui::Event::Key {
                        key: eframe::egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
            }
        }
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        let ctx = eframe::egui::Context::default();
        frame(&ctx, &mut native, vec![]);
        let header = native.modulation_header.unwrap().center();
        click(&ctx, &mut native, header);
        for _ in 0..32 {
            frame(&ctx, &mut native, vec![]);
        }
        for configured in [false, true] {
            native.comb_configured = configured;
            open_destination(&ctx, &mut native);
            assert_eq!(
                native
                    .destination_choices
                    .iter()
                    .any(|(_, value, _)| matches!(*value, 9 | 19 | 20 | 21)),
                configured,
                "Regular Filter2 destinations require configured controller tables"
            );
            escape(&ctx, &mut native);
        }
        for _ in 0..3 {
            native.step_filter2_type();
        }
        native.comb_configured = false;
        open_destination(&ctx, &mut native);
        assert!(
            !native
                .destination_choices
                .iter()
                .any(|(_, value, _)| matches!(*value, 9 | 19 | 20 | 21)),
            "Missing Comb configuration must not offer its destinations"
        );
        escape(&ctx, &mut native);
        // This presentation fixture deliberately has no device; readiness mirrors a successful configure_comb.
        native.comb_configured = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            while native.panel_state().filter2_type != 3 {
                native.step_filter2_type();
            }
            for value in [5, 9, 19, 20, 21] {
                open_destination(&ctx, &mut native);
                let rect = native
                    .destination_choices
                    .iter()
                    .find(|(index, candidate, _)| *index == 0 && *candidate == value)
                    .unwrap()
                    .2;
                let viewport = native.destination_viewport.unwrap();
                if !viewport.contains(rect.center()) {
                    frame(
                        &ctx,
                        &mut native,
                        vec![eframe::egui::Event::PointerMoved(eframe::egui::pos2(
                            rect.center().x,
                            viewport.center().y,
                        ))],
                    );
                    frame(
                        &ctx,
                        &mut native,
                        vec![eframe::egui::Event::MouseWheel {
                            unit: eframe::egui::MouseWheelUnit::Point,
                            delta: eframe::egui::vec2(0.0, viewport.center().y - rect.center().y),
                            phase: eframe::egui::TouchPhase::Move,
                            modifiers: Default::default(),
                        }],
                    );
                    for _ in 0..32 {
                        frame(&ctx, &mut native, vec![]);
                    }
                }
                let center = native
                    .destination_choices
                    .iter()
                    .find(|(index, candidate, _)| *index == 0 && *candidate == value)
                    .unwrap()
                    .2
                    .center();
                assert!(
                    native.destination_viewport.unwrap().contains(center),
                    "Destination {value} at {center:?}, clip {:?}",
                    native.destination_viewport
                );
                native.modulation_updates.clear();
                click(&ctx, &mut native, center);
                let (index, program) = native.modulation_updates.last().copied().unwrap();
                assert_eq!(index, timbre as u8);
                assert_eq!(program.routes[0].destination.index(), value as usize);
                assert_eq!(
                    native.timbres[timbre].modulation.routes[0]
                        .destination
                        .index(),
                    value as usize
                );
            }
        }
    }

    #[test]
    fn shaper_settings_preserve_supported_modes_positions_and_depth_per_timbre() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.timbres[timbre].shaper_parameters(), Some((0, 0, 0)));
            for mode in 0..3 {
                assert_eq!(native.panel_state().shaper_mode, mode);
                for position in 0..2 {
                    native.set_shaper_position(position);
                    for depth in [0, 64, 127] {
                        native.pot(1, 0, (depth as u16) << 3);
                        assert_eq!(
                            native.timbres[timbre].shaper_parameters(),
                            Some((mode, position, depth))
                        );
                    }
                }
                native.step_shaper();
            }
            native.step_shaper();
            native.set_shaper_position((timbre & 1) as u8);
            native.pot(1, 0, ((35 + timbre * 16) as u16) << 3);
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(
                native.timbres[timbre].shaper_parameters(),
                Some((1, (timbre & 1) as u8, (35 + timbre * 16) as u8))
            );
            native.set_shaper_position(2);
            assert_eq!(native.panel_state().shaper_position, (timbre & 1) as u8);
        }
        let saved = native.panel_state().shaper_depth;
        native.set_shaper_mode(13);
        assert_eq!(native.panel_state().shaper_mode, 1);
        native.timbres[native.selected].shaper_mode = 13;
        native.pot(1, 0, 1023);
        native.step_shaper();
        native.set_shaper_position(0);
        assert_eq!(native.panel_state().shaper_depth, saved);
        assert!(
            native.timbres[native.selected]
                .shaper_parameters()
                .is_none()
        );
    }

    #[test]
    fn all_ws_types_retain_choice_depth_and_placement_across_drive_and_off() {
        let mut native = NativePanel::new(std::path::Path::new("."), true);
        native.enabled = true;
        for timbre in 0..4 {
            native.select_timbre(timbre);
            assert_eq!(native.panel_state().shaper_ws_mode, 2);
            native.set_shaper_position((timbre & 1) as u8);
            native.pot(1, 0, ((27 + timbre * 24) as u16) << 3);
            for mode in 2..crate::panel::SHAPER_NAMES.len() as u8 {
                native.set_shaper_mode(mode);
                assert_eq!(native.panel_state().shaper_ws_mode, mode);
                for expected in [0, 1, mode] {
                    native.step_shaper();
                    assert_eq!(
                        native.timbres[timbre].shaper_parameters(),
                        Some((expected, (timbre & 1) as u8, (27 + timbre * 24) as u8))
                    );
                    assert_eq!(native.panel_state().shaper_ws_mode, mode);
                }
            }
            native.set_shaper_mode(3 + timbre as u8 * 3);
            native.set_shaper_mode(0);
            assert_eq!(native.panel_state().shaper_ws_mode, 3 + timbre as u8 * 3);
            native.set_shaper_mode(1);
        }
        for timbre in 0..4 {
            native.select_timbre(timbre);
            native.step_shaper();
            assert_eq!(native.panel_state().shaper_mode, 3 + timbre as u8 * 3);
            assert_eq!(native.panel_state().shaper_ws_mode, 3 + timbre as u8 * 3);
            assert_eq!(native.panel_state().shaper_position, (timbre & 1) as u8);
            assert_eq!(native.panel_state().shaper_depth, (27 + timbre * 24) as u8);
            native.set_shaper_mode(13);
            assert_eq!(native.panel_state().shaper_mode, 3 + timbre as u8 * 3);
        }
    }
}
