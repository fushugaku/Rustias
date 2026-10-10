//! A fixed voice pool for one four-timbre instrument, independent of devices.
use crate::{
    VoiceControlEvent, VoiceRenderer,
    amplifier::{AmplifierController, ControllerTables},
    lfo::{LFO_COUNT, synthesis_clock_slot},
    modulation::{ModulationProgram, SOURCE_COUNT, VoiceModulation, VoiceModulationTables},
    scale_bus,
    shared_lfo::{EffectLfoController, EffectLfoParameters, SharedTimbreLfo},
};
use radias_synth_domain::{
    Sample,
    filter::FilterCoefficients,
    fixed::saturate,
    pan::{StereoFrame, VoiceBus},
    processor_link::{ProcessorLink, scale_slave},
    voice_allocation::{
        AllocationOwner, VOICE_COUNT, VOICES_PER_PROCESSOR, VoiceAllocator, VoiceAssignment,
        VoiceMask,
    },
    waveform::WaveformTable,
};

pub const TIMBRE_COUNT: usize = radias_synth_domain::pan::TIMBRE_BUSES;

pub struct ActiveVoice {
    /// SYS008448 sets this for an alternate synthesis body; SYS0083E8 clears it.
    pub uses_program_common: bool,
    /// A drum owns its synthesis pitch controls and receives source note60.
    pub drum_pitch: Option<radias_synth_domain::note_pitch::PitchProgram>,
    pub drum_instrument: Option<u8>,
    pub drum_filter2: Option<crate::filter2::Filter2Program>,
    pub renderer: VoiceRenderer,
    pub amplifier: Option<AmplifierController>,
    pub modulation: Option<VoiceModulation>,
    pub auxiliary: Option<crate::voice_envelopes::VoiceEnvelopes>,
    pub pan: Option<radias_synth_domain::controller_pan::PanControl>,
    pub mixer: Option<crate::mixer::MixerProgram>,
    pub secondary: Option<crate::secondary::SecondaryProgram>,
    pub primary: Option<crate::primary::PrimaryProgram>,
    pub shaper: Option<crate::shaper::ShaperProgram>,
    pub comb_program: Option<crate::comb::CombProgram>,
    pub timbre: u8,
    pub note: u8,
    pub velocity: u8,
    pub held: bool,
    pub program: usize,
    pub bus: VoiceBus,
}
impl ActiveVoice {
    fn fresh_note(&self) -> Self {
        Self {
            uses_program_common: self.uses_program_common,
            drum_pitch: self.drum_pitch,
            drum_instrument: self.drum_instrument,
            drum_filter2: self.drum_filter2,
            renderer: self.renderer.fresh_note(),
            amplifier: self.amplifier,
            modulation: None,
            auxiliary: self.auxiliary,
            pan: self.pan,
            mixer: self.mixer,
            secondary: self.secondary,
            primary: self.primary,
            shaper: self.shaper,
            comb_program: self.comb_program,
            timbre: self.timbre,
            note: self.note,
            velocity: self.velocity,
            held: self.held,
            program: self.program,
            bus: self.bus,
        }
    }
}

pub struct PolyphonicRenderer {
    #[cfg(feature = "web-modular")]
    circuits: [Option<alloc::boxed::Box<dyn crate::VoiceCircuit>>; TIMBRE_COUNT],
    pub allocator: VoiceAllocator,
    voices: [Option<ActiveVoice>; VOICE_COUNT],
    physical_frames: [Option<radias_synth_domain::voice_frame::VoiceFrameState>; VOICE_COUNT],
    link: ProcessorLink,
    modulation_frame: u64,
    controller_timer: Option<radias_synth_domain::controller_service::ControllerServiceTimer>,
    controller_interrupts: u64,
    controller_flags: [u8; VOICE_COUNT],
    amplifier_rates: Option<radias_synth_domain::amplifier_delivery::AmplifierRateTable>,
    amplifier_deliveries: [radias_synth_domain::amplifier_delivery::AmplifierDelivery; VOICE_COUNT],
    amplifier_transport: crate::synthesis_transport::SynthesisParameterTransport,
    amplifier_transport_failed: bool,
    amplifier_fresh: [bool; VOICE_COUNT],
    amplifier_soft_binding: [bool; VOICE_COUNT],
    amplifier_targets: [Option<i16>; VOICE_COUNT],
    amplifier_bound: [bool; VOICE_COUNT],
    filter_delivery_targets: [Option<(i32, i32)>; VOICE_COUNT],
    filter2_delivery_resonance: [Option<(i32, i16)>; VOICE_COUNT],
    comb_feedback_delivery_targets: [Option<u32>; VOICE_COUNT],
    controller_pitch_codes: [u16; VOICE_COUNT],
    initial_controller_serviced: [bool; VOICE_COUNT],
    secondary_sync_targets: [Option<bool>; VOICE_COUNT],
    mixer_delivery_targets: [Option<[i16; 3]>; VOICE_COUNT],
    pan_delivery_targets: [Option<i16>; VOICE_COUNT],
    ratio_delivery_targets: [Option<i16>; VOICE_COUNT],
    pub modulation_random: u16,
    shared_lfo: [SharedTimbreLfo; TIMBRE_COUNT],
    effect_lfo_parameters: [[EffectLfoParameters; 2]; TIMBRE_COUNT],
    global_lfo: EffectLfoController,
    global_lfo_parameters: EffectLfoParameters,
    controller_slots: [[radias_synth_domain::lfo::LfoState; LFO_COUNT]; VOICE_COUNT],
    controller_slot_valid: [bool; VOICE_COUNT],
    timbre_modulation_active: [bool; TIMBRE_COUNT],
    clock: Option<crate::clock::InstrumentClock>,
    filter_tables: Option<radias_synth_domain::controller_filter::ControllerFilterTables>,
    comb_tables: Option<radias_synth_domain::controller_comb::CombControlTables>,
    filter2_tables: Option<radias_synth_domain::controller_filter2::Filter2ControlTables>,
    filter2_programs: [Option<crate::filter2::Filter2Program>; TIMBRE_COUNT],
    pan_tables: Option<(
        radias_synth_domain::controller_pan::PanTables,
        radias_synth_domain::control_slew::SlewWeights,
    )>,
    mixer_scales: Option<radias_synth_domain::controller_mixer::MixerScales>,
    secondary_table: Option<radias_synth_domain::controller_secondary::FineTuneTable>,
    noise_tables: Option<crate::noise::NoiseTables>,
    note_pitch_tables: Option<radias_synth_domain::note_pitch::NotePitchTables>,
    pitch_programs: [Option<radias_synth_domain::note_pitch::PitchProgram>; TIMBRE_COUNT],
    note_pitches: [Option<crate::note_pitch::VoiceNotePitch>; VOICE_COUNT],
    midi_pitch: [crate::note_pitch::MidiPitch; TIMBRE_COUNT],
    scale_context: radias_synth_domain::note_pitch::ScaleContext,
    master_tune: i32,
    portamento_tables: Option<crate::portamento::PortamentoTables>,
    portamento_programs: [Option<radias_synth_domain::portamento::PortamentoProgram>; TIMBRE_COUNT],
    portamento_voices: [Option<crate::portamento::VoicePortamento>; VOICE_COUNT],
    portamento_switches: [bool; TIMBRE_COUNT],
    assigned_pitch_slots: [i32; VOICE_COUNT],
    shared_assigned_pitch: [i32; TIMBRE_COUNT],
    voice_modes: [radias_synth_domain::mono_notes::VoiceMode; TIMBRE_COUNT],
    mono_notes: [radias_synth_domain::mono_notes::MonoNotes; TIMBRE_COUNT],
    sustain_states: [radias_synth_domain::sustain::SustainState; TIMBRE_COUNT],
    sustain_programs: [radias_synth_domain::sustain::SustainProgram; TIMBRE_COUNT],
    release_selectors: [u8; TIMBRE_COUNT],
    note_groups: radias_synth_domain::note_groups::NoteGroups,
    voice_group_tables: Option<radias_synth_domain::voice_group::VoiceGroupTables>,
    voice_group_programs: [radias_synth_domain::voice_group::VoiceGroupProgram; TIMBRE_COUNT],
    voice_group_slots: radias_synth_domain::voice_group::VoiceGroupSlots,
    voice_group_banks: [u8; VOICE_COUNT],
    voice_group_timbre_banks: [u8; TIMBRE_COUNT],
    voice_group_offsets: [radias_synth_domain::voice_group::GroupOffsets; VOICE_COUNT],
    prepared_note_tag: Option<u8>,
    program_common: Option<radias_synth_domain::program_binding::ProgramCommon>,
    drum_groups: radias_synth_domain::drum_groups::DrumVoiceGroups,
    drum_gain: f64,
    drum_slots: [bool; VOICE_COUNT],
}
impl Default for PolyphonicRenderer {
    fn default() -> Self {
        Self {
            allocator: VoiceAllocator::default(),
            voices: core::array::from_fn(|_| None),
            physical_frames: [None; VOICE_COUNT],
            #[cfg(feature = "web-modular")]
            circuits: core::array::from_fn(|_| None),
            link: ProcessorLink::default(),
            modulation_frame: 0,
            controller_timer: None,
            controller_interrupts: 0,
            controller_flags: [0; VOICE_COUNT],
            amplifier_rates: None,
            amplifier_deliveries:
                [radias_synth_domain::amplifier_delivery::AmplifierDelivery::default(); VOICE_COUNT],
            amplifier_transport: Default::default(),
            amplifier_transport_failed: false,
            amplifier_fresh: [false; VOICE_COUNT],
            amplifier_soft_binding: [false; VOICE_COUNT],
            amplifier_targets: [None; VOICE_COUNT],
            amplifier_bound: [true; VOICE_COUNT],
            filter_delivery_targets: [None; VOICE_COUNT],
            filter2_delivery_resonance: [None; VOICE_COUNT],
            comb_feedback_delivery_targets: [None; VOICE_COUNT],
            controller_pitch_codes: [0; VOICE_COUNT],
            initial_controller_serviced: [false; VOICE_COUNT],
            secondary_sync_targets: [None; VOICE_COUNT],
            mixer_delivery_targets: [None; VOICE_COUNT],
            pan_delivery_targets: [None; VOICE_COUNT],
            ratio_delivery_targets: [None; VOICE_COUNT],
            modulation_random: 0xace1,
            shared_lfo: core::array::from_fn(|_| SharedTimbreLfo::default()),
            effect_lfo_parameters: [[Default::default(); 2]; TIMBRE_COUNT],
            global_lfo: Default::default(),
            global_lfo_parameters: Default::default(),
            controller_slots: [[Default::default(); LFO_COUNT]; VOICE_COUNT],
            controller_slot_valid: [false; VOICE_COUNT],
            timbre_modulation_active: core::array::from_fn(|i| i == 0),
            clock: None,
            filter_tables: None,
            comb_tables: None,
            filter2_tables: None,
            filter2_programs: [None; TIMBRE_COUNT],
            pan_tables: None,
            mixer_scales: None,
            secondary_table: None,
            noise_tables: None,
            note_pitch_tables: None,
            pitch_programs: [None; TIMBRE_COUNT],
            note_pitches: [None; VOICE_COUNT],
            midi_pitch: [Default::default(); TIMBRE_COUNT],
            scale_context: Default::default(),
            master_tune: 0,
            portamento_tables: None,
            portamento_programs: [None; TIMBRE_COUNT],
            portamento_voices: [None; VOICE_COUNT],
            portamento_switches: [false; TIMBRE_COUNT],
            assigned_pitch_slots: [0; VOICE_COUNT],
            shared_assigned_pitch: [60 << 16; TIMBRE_COUNT],
            voice_modes: [Default::default(); TIMBRE_COUNT],
            mono_notes: [Default::default(); TIMBRE_COUNT],
            sustain_states: [Default::default(); TIMBRE_COUNT],
            sustain_programs: [Default::default(); TIMBRE_COUNT],
            release_selectors: [0; TIMBRE_COUNT],
            note_groups: Default::default(),
            voice_group_tables: None,
            voice_group_programs: [Default::default(); TIMBRE_COUNT],
            voice_group_slots: Default::default(),
            voice_group_banks: [0; VOICE_COUNT],
            voice_group_timbre_banks: [0; TIMBRE_COUNT],
            voice_group_offsets: [Default::default(); VOICE_COUNT],
            prepared_note_tag: None,
            program_common: None,
            drum_groups: Default::default(),
            drum_gain: 1.0,
            drum_slots: [false; VOICE_COUNT],
        }
    }
}
impl PolyphonicRenderer {
    /// Bind the original exclusive group after alternate-body allocation.
    /// The allocator already owns the declared processing cost.
    pub fn bind_drum_group(&mut self, slot: usize, group: u8) {
        if slot < VOICE_COUNT && self.voices[slot].is_some() {
            self.drum_groups.groups[slot] = group;
        }
    }
    pub fn drum_group(&self, slot: usize) -> Option<(u8, u16)> {
        self.voices.get(slot)?.as_ref()?;
        Some((
            self.drum_groups.groups[slot],
            self.allocator.budget.costs[slot],
        ))
    }
    pub fn retire_drum_group(&mut self, timbre: u8, event: u32, group: u8) -> VoiceMask {
        if timbre as usize >= TIMBRE_COUNT {
            return 0;
        }
        let selected = self.drum_groups.retire(
            radias_synth_domain::drum_groups::DrumGroupRequest {
                owner: AllocationOwner(timbre as u32 + 1),
                event,
                group,
            },
            &self.note_groups,
            &mut self.allocator.claims,
            &mut self.allocator.order,
            &mut self.allocator.budget.costs,
        );
        for slot in 0..VOICE_COUNT {
            if selected & (1 << slot) != 0 {
                self.remove(slot);
            }
        }
        selected
    }
    /// Change the original common level/pan context only on actors bound by
    /// the alternate-body path. Envelopes, note ownership and phases persist.
    pub fn edit_program_common(
        &mut self,
        common: radias_synth_domain::program_binding::ProgramCommon,
        tables: &ControllerTables,
    ) {
        self.program_common = Some(common);
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(voice) = voice.as_mut().filter(|v| v.uses_program_common) else {
                continue;
            };
            if let Some(amplifier) = &mut voice.amplifier {
                amplifier.program_common(Some(common.level), tables);
            }
            if let Some(pan) = &mut voice.pan {
                pan.midi_pan = Some(common.pan);
                if let Some((pan_tables, _)) = &self.pan_tables {
                    let mut composed = *pan;
                    composed.timbre_offset = composed
                        .timbre_offset
                        .wrapping_add(self.voice_group_offsets[slot].pan);
                    composed.modulation = voice
                        .modulation
                        .as_ref()
                        .map_or(0, |m| m.patches.targets.controls[10]);
                    if !self.amplifier_transport.pitch_enabled() {
                        voice
                            .renderer
                            .set_pan_target(pan_tables.compile(composed.target()) as i16);
                    }
                }
            }
        }
    }
    pub fn configure_voice_groups(
        &mut self,
        tables: radias_synth_domain::voice_group::VoiceGroupTables,
    ) {
        self.voice_group_tables = Some(tables);
    }
    pub fn edit_source_gain(&mut self, timbre: u8, gain: u16, tables: &ControllerTables) {
        for actor in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if let Some(amplifier) = &mut actor.amplifier {
                amplifier.source_gain(gain, tables);
            }
        }
    }
    /// Apply an instrument-body edit only to actors with that synthesis identity.
    /// Note allocation, physical phases and unrelated instruments are retained.
    pub fn edit_drum_instrument(
        &mut self,
        timbre: u8,
        instrument: u8,
        old: crate::drum_program::DrumInstrumentProgram,
        new: crate::drum_program::DrumInstrumentProgram,
        tables: &ControllerTables,
        modulation_tables: &VoiceModulationTables,
    ) {
        let before = old.controls;
        let c = new.controls;
        let graph = new.graph;
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(voice) = voice
                .as_mut()
                .filter(|v| v.timbre == timbre && v.drum_instrument == Some(instrument))
            else {
                continue;
            };
            if before.pitch != c.pitch {
                voice.drum_pitch = Some(c.pitch);
                if let Some(pitch_tables) = &self.note_pitch_tables {
                    if before.pitch.transpose != c.pitch.transpose {
                        self.note_pitches[slot] = c
                            .pitch
                            .initialize(
                                60,
                                self.scale_context,
                                pitch_tables,
                                &mut self.modulation_random,
                            )
                            .map(|note| crate::note_pitch::VoiceNotePitch { note });
                        if let Some(pitch) = self.note_pitches[slot] {
                            if let Some(amp) = &mut voice.amplifier {
                                amp.retarget(pitch.note.wrapped, voice.velocity);
                            }
                            if let Some(aux) = &mut voice.auxiliary {
                                aux.retarget(pitch.note.wrapped, voice.velocity);
                            }
                        }
                    }
                    if let (Some(pitch), Some(modulation)) =
                        (self.note_pitches[slot], &mut voice.modulation)
                    {
                        modulation.base_pitch_q16 =
                            pitch.drum_base(c.pitch, pitch_tables, self.master_tune);
                        modulation.vibrato_depth = c.pitch.vibrato_depth(
                            self.midi_pitch[timbre as usize].wheel,
                            &pitch_tables.vibrato,
                        );
                    }
                }
            }
            if (before.envelope[1] != c.envelope[1]
                || before.amplifier_level != c.amplifier_level
                || before.amplifier_key_tracking != c.amplifier_key_tracking)
                && let Some(amp) = &mut voice.amplifier
            {
                let current = amp.control();
                amp.edit_program(
                    c.amplifier(
                        current.source_gain,
                        current.midi_volume,
                        current.program_volume,
                    ),
                    tables,
                );
            }
            if let Some(auxiliary) = &mut voice.auxiliary {
                if before.envelope[0] != c.envelope[0] || before.envelope[2] != c.envelope[2] {
                    auxiliary.edit([c.envelope[0], c.envelope[2]], tables);
                }
                auxiliary.filter = Some(graph.dynamic_filter);
            }
            if (before.cutoff[0] != c.cutoff[0]
                || before.resonance[0] != c.resonance[0]
                || before.filter_type != c.filter_type)
                && self.amplifier_rates.is_none()
            {
                voice.renderer.set_filter(graph.filter);
            }
            voice.drum_filter2 = graph.dynamic_filter2;
            voice.comb_program = graph.comb;
            if before.filter_route != c.filter_route
                || before.cutoff[1] != c.cutoff[1]
                || before.resonance[1] != c.resonance[1]
            {
                voice
                    .renderer
                    .set_filter_routing(graph.filter_routing, graph.filter2);
            }
            if before.pan != c.pan
                && let Some(pan) = &mut voice.pan
            {
                pan.position = c.pan;
            }
            if before.mixer != c.mixer {
                voice.mixer = Some(c.mixer);
            }
            if before.secondary != c.secondary {
                voice.secondary = Some(c.secondary);
                voice.renderer.select_secondary(c.secondary);
            }
            if before.shaper != c.shaper {
                voice.shaper = Some(c.shaper);
                let mut shaper = c.shaper;
                shaper.control.modulation = voice
                    .modulation
                    .as_ref()
                    .map_or(0, |m| m.patches.targets.controls[8]);
                voice
                    .renderer
                    .set_shaper(shaper.parameters_with_pitch(voice.renderer.primary_pitch_code()));
            }
            if before.modulation != c.modulation
                && let Some(modulation) = &mut voice.modulation
            {
                modulation.pair.parameters = c.modulation.lfo;
                modulation.patches.routes = c.modulation.routes;
                modulation.patches.manual_offsets = c.modulation.manual_offsets;
                modulation.tempo_divisions = c.modulation.tempo_divisions;
                if let Some(clock) = &mut self.clock {
                    clock.divisions.voices[slot] = c.modulation.tempo_divisions;
                    for i in 0..LFO_COUNT {
                        clock.bank.voices[slot][i].previous_increment = clock
                            .tables
                            .compile_increment(
                                (c.modulation.tempo_divisions[i] & 31) as i32,
                                0,
                                clock.receiver.tempo.clock_rate(),
                            )
                            .1;
                    }
                }
            }
            if before.oscillator_selection != c.oscillator_selection
                || before.oscillator_controls != c.oscillator_controls
            {
                let primary = c.primary();
                let changed = voice
                    .primary
                    .is_none_or(|p| p.selection != primary.selection);
                let code =
                    radias_synth_domain::pitch::PitchCode::new(voice.renderer.primary_pitch_code())
                        .unwrap();
                let increment = modulation_tables.pitch.increment(code);
                if changed
                    && let Some(parameters) = primary.compile_waveform(
                        increment,
                        modulation_tables.bandwidth.coefficient(increment),
                    )
                {
                    voice.renderer.select_primary(parameters);
                    voice.program = if primary.selection & 15 < 4 {
                        (primary.selection & 3) as usize
                    } else {
                        0
                    };
                }
                if changed && primary.selection & 0x30 == 0x20 {
                    voice
                        .renderer
                        .update_unison_phases(primary.control, primary.selection & 3 == 2);
                }
                voice.primary = Some(primary);
                if changed && let Some(noise) = &self.noise_tables {
                    Self::initialize_noise(noise, slot, voice);
                }
            }
        }
    }
    pub fn edit_voice_group(
        &mut self,
        timbre: u8,
        program: radias_synth_domain::voice_group::VoiceGroupProgram,
    ) {
        let Some(current) = self.voice_group_programs.get_mut(timbre as usize) else {
            return;
        };
        let previous = *current;
        *current = program;
        if previous.raw & 143 != program.raw & 143 {
            self.retire_timbre_for_program_edit(timbre);
        }
        // A compound adapter update follows the original parameter order:
        // Detune first, then Spread. Each callback recomputes both offsets.
        let mut intermediate = radias_synth_domain::voice_group::VoiceGroupProgram {
            raw: program.raw,
            ..previous
        };
        let selected = self
            .voices
            .iter()
            .enumerate()
            .fold(0, |mask, (slot, actor)| {
                mask | if actor.as_ref().is_some_and(|v| v.timbre == timbre) {
                    1 << slot
                } else {
                    0
                }
            });
        let primary = self
            .voices
            .iter()
            .flatten()
            .find(|v| v.timbre == timbre)
            .and_then(|v| v.primary)
            .map_or(0, |p| p.selection);
        let groups = self.note_groups;
        let claims = self.allocator.claims;
        if previous.detune != program.detune {
            intermediate.detune = program.detune;
            self.edit_selected_group_members(selected, intermediate, primary, &groups, &claims);
        }
        if previous.spread != program.spread {
            intermediate.spread = program.spread;
            self.edit_selected_group_members(selected, intermediate, primary, &groups, &claims);
        }
    }
    pub fn edit_selected_group_members(
        &mut self,
        selected: VoiceMask,
        program: radias_synth_domain::voice_group::VoiceGroupProgram,
        primary: u8,
        groups: &radias_synth_domain::note_groups::NoteGroups,
        claims: &[radias_synth_domain::voice_allocation::VoiceClaim; VOICE_COUNT],
    ) -> VoiceMask {
        let Some(tables) = &self.voice_group_tables else {
            return 0;
        };
        let previous = self.voice_group_offsets;
        let changed = crate::voice_groups::edit_selected_groups(
            selected,
            program,
            primary,
            groups,
            claims,
            &mut self.voice_group_slots,
            &mut self.voice_group_banks,
            &mut self.voice_group_offsets,
            tables,
            &mut self.modulation_random,
        )
        .unwrap_or(0);
        self.publish_group_offsets(changed, previous);
        changed
    }
    pub fn retire_timbre_for_program_edit(&mut self, timbre: u8) -> VoiceMask {
        if timbre as usize >= TIMBRE_COUNT {
            return 0;
        }
        let retired = self
            .allocator
            .retire_owner_for_program_edit(AllocationOwner(timbre as u32 + 1));
        for slot in 0..VOICE_COUNT {
            if retired & (1 << slot) != 0 {
                self.cache_retired_voice(slot);
                self.voices[slot] = None;
                self.note_pitches[slot] = None;
                self.portamento_voices[slot] = None;
            }
        }
        self.clear_mono_notes(timbre);
        self.sustain_states[timbre as usize].flags &= 127;
        retired
    }
    pub fn voice_group_slots(&self) -> radias_synth_domain::voice_group::VoiceGroupSlots {
        self.voice_group_slots
    }
    pub fn initialize_voice_group_slots(
        &mut self,
        slots: radias_synth_domain::voice_group::VoiceGroupSlots,
    ) {
        self.voice_group_slots = slots;
    }
    pub fn voice_group_bank(&self, slot: usize) -> u8 {
        self.voice_group_banks[slot]
    }
    pub fn voice_group_offsets(
        &self,
        slot: usize,
    ) -> radias_synth_domain::voice_group::GroupOffsets {
        self.voice_group_offsets[slot]
    }
    /// Original01EBE4 operates before the new note overwrites age/tag data.
    /// A caller with a separate allocation clock supplies its temporary flags.
    pub fn repair_retired_group_members(
        &mut self,
        retired: VoiceMask,
        program: radias_synth_domain::voice_group::VoiceGroupProgram,
        primary: u8,
        groups: &radias_synth_domain::note_groups::NoteGroups,
        claims: &[radias_synth_domain::voice_allocation::VoiceClaim; VOICE_COUNT],
    ) -> VoiceMask {
        let Some(tables) = &self.voice_group_tables else {
            return 0;
        };
        let previous = self.voice_group_offsets;
        let repaired = crate::voice_groups::repair_retired_groups(
            retired,
            program,
            primary,
            groups,
            claims,
            &self.voice_group_slots,
            &mut self.voice_group_banks,
            &mut self.voice_group_offsets,
            tables,
            &mut self.modulation_random,
        )
        .unwrap_or(0);
        self.publish_group_offsets(repaired, previous);
        repaired
    }
    fn publish_group_offsets(
        &mut self,
        changed: VoiceMask,
        previous: [radias_synth_domain::voice_group::GroupOffsets; VOICE_COUNT],
    ) {
        for (slot, previous) in previous.iter().enumerate() {
            if changed & (1 << slot) == 0 {
                continue;
            }
            let Some(actor) = &mut self.voices[slot] else {
                continue;
            };
            if let Some(amplifier) = &mut actor.amplifier {
                amplifier.set_group_gain_bank(self.voice_group_banks[slot]);
            }
            if let Some(modulation) = &mut actor.modulation {
                modulation.base_pitch_q16 = modulation
                    .base_pitch_q16
                    .wrapping_sub(previous.tuning_q16)
                    .wrapping_add(self.voice_group_offsets[slot].tuning_q16);
            }
            if let (Some((tables, _)), Some(mut pan)) = (&self.pan_tables, actor.pan) {
                pan.timbre_offset = pan
                    .timbre_offset
                    .wrapping_add(self.voice_group_offsets[slot].pan);
                pan.modulation = actor
                    .modulation
                    .as_ref()
                    .map_or(0, |m| m.patches.targets.controls[10]);
                if !self.amplifier_transport.pitch_enabled() {
                    actor
                        .renderer
                        .set_pan_target(tables.compile(pan.target()) as i16);
                }
            }
        }
    }
    fn repair_poly_allocation(
        &mut self,
        timbre: u8,
        primary: u8,
        selected: VoiceMask,
        retired: VoiceMask,
        previous_claims: &[radias_synth_domain::voice_allocation::VoiceClaim; VOICE_COUNT],
    ) {
        let mut temporary = self.allocator.claims;
        for slot in 0..VOICE_COUNT {
            if selected & (1 << slot) != 0 {
                temporary[slot] = previous_claims[slot];
                temporary[slot].note_flags |= 128;
            }
        }
        let groups = self.note_groups;
        self.repair_retired_group_members(
            retired,
            self.voice_group_programs[timbre as usize],
            primary,
            &groups,
            &temporary,
        );
    }
    fn initialize_group_offsets(&mut self, slot: usize, timbre: u8) {
        self.voice_group_banks[slot] = self.voice_group_timbre_banks[timbre as usize];
        self.voice_group_offsets[slot] = self
            .voice_group_tables
            .as_ref()
            .and_then(|tables| {
                tables.offsets(
                    self.voice_group_programs[timbre as usize],
                    self.voice_group_banks[slot],
                    self.voice_group_slots.indices[slot],
                    &mut self.modulation_random,
                )
            })
            .unwrap_or_default();
    }
    fn initialize_single_group(&mut self, slot: usize, timbre: u8) {
        self.voice_group_slots.timbres[slot] = timbre;
        self.voice_group_slots.indices[slot] = 0;
        self.voice_group_slots.stereo[slot] = 0;
        self.voice_group_banks[slot] = 0;
        self.voice_group_timbre_banks[timbre as usize] = 0;
    }
    /// Prepare the instrument event before queue/priority decisions. Rejected
    /// notes and note-offs also advance the original wrapping age counter.
    pub fn begin_note_event(&mut self, tag: u8) {
        self.note_groups.dispatch();
        self.prepared_note_tag = Some(tag);
    }
    pub fn finish_note_event(&mut self) {
        self.prepared_note_tag = None;
    }
    fn consume_note_event(&mut self) -> u8 {
        if let Some(tag) = self.prepared_note_tag.take() {
            tag
        } else {
            self.note_groups.dispatch();
            0x10
        }
    }
    pub fn note_groups(&self) -> radias_synth_domain::note_groups::NoteGroups {
        self.note_groups
    }
    pub fn initialize_note_groups(&mut self, groups: radias_synth_domain::note_groups::NoteGroups) {
        self.note_groups = groups;
    }
    pub fn edit_sustain_program(
        &mut self,
        timbre: u8,
        program: radias_synth_domain::sustain::SustainProgram,
        release_selector: u8,
    ) {
        if let Some(current) = self.sustain_programs.get_mut(timbre as usize) {
            *current = program;
            self.release_selectors[timbre as usize] = release_selector & 12;
        }
    }
    pub fn sustain_state(&self, timbre: u8) -> Option<radias_synth_domain::sustain::SustainState> {
        self.sustain_states.get(timbre as usize).copied()
    }
    pub fn initialize_sustain_states(
        &mut self,
        states: [radias_synth_domain::sustain::SustainState; TIMBRE_COUNT],
    ) {
        self.sustain_states = states;
    }
    /// 00983c first records all four timbre flags, then services all four.
    /// Its Poly pending scan intentionally visits every physical actor.
    pub fn sustain_event(
        &mut self,
        channels: [u8; TIMBRE_COUNT],
        event: u32,
        tables: Option<&ControllerTables>,
    ) -> VoiceMask {
        use radias_synth_domain::sustain::SustainState;
        for (timbre, channel) in channels.iter().enumerate() {
            self.sustain_states[timbre].receive(*channel, event);
        }
        let mut released = 0;
        for timbre in 0..TIMBRE_COUNT {
            if self.sustain_states[timbre].flags & 15 != 0 {
                continue;
            }
            if self.voice_modes[timbre].polyphonic {
                for slot in 0..VOICE_COUNT {
                    if SustainState::release_pending_poly(&mut self.allocator.claims[slot]) {
                        self.release_slot(slot, tables);
                        released |= 1 << slot;
                    }
                }
            } else if self.sustain_states[timbre].release_pending_mono() {
                released |= self.release_mono_owner(timbre as u8, tables);
            }
        }
        released
    }
    fn release_slot(&mut self, slot: usize, tables: Option<&ControllerTables>) {
        let Some(voice) = &mut self.voices[slot] else {
            return;
        };
        voice.held = false;
        self.controller_flags[slot] = 0x81;
        if let (Some(amplifier), Some(tables)) = (&mut voice.amplifier, tables) {
            amplifier.release(tables);
        } else {
            voice.renderer.release();
        }
        if let (Some(auxiliary), Some(tables)) = (&mut voice.auxiliary, tables) {
            auxiliary.release(tables);
        }
    }
    fn release_mono_owner(&mut self, timbre: u8, tables: Option<&ControllerTables>) -> VoiceMask {
        let mut released = 0;
        for slot in 0..VOICE_COUNT {
            let claim = &mut self.allocator.claims[slot];
            if claim.owner == AllocationOwner(timbre as u32 + 1)
                && radias_synth_domain::sustain::SustainState::release_mono_claim(claim)
            {
                self.release_slot(slot, tables);
                released |= 1 << slot;
            }
        }
        released
    }
    pub fn set_voice_mode(&mut self, timbre: u8, mode: radias_synth_domain::mono_notes::VoiceMode) {
        if let Some(value) = self.voice_modes.get_mut(timbre as usize) {
            *value = mode;
        }
    }
    pub fn voice_mode(&self, timbre: u8) -> Option<radias_synth_domain::mono_notes::VoiceMode> {
        self.voice_modes.get(timbre as usize).copied()
    }
    pub fn mono_notes(&self, timbre: u8) -> Option<radias_synth_domain::mono_notes::MonoNotes> {
        self.mono_notes.get(timbre as usize).copied()
    }
    pub fn mono_event(
        &mut self,
        timbre: u8,
        event: u32,
    ) -> Option<radias_synth_domain::mono_notes::MonoDecision> {
        let mode = *self.voice_modes.get(timbre as usize)?;
        if mode.polyphonic {
            return None;
        }
        let queue = &mut self.mono_notes[timbre as usize];
        Some(if (event as i8) < 0 {
            queue.note_on(mode, event)
        } else {
            queue.note_off(mode, event)
        })
    }
    pub fn clear_mono_notes(&mut self, timbre: u8) {
        if let Some(notes) = self.mono_notes.get_mut(timbre as usize) {
            *notes = Default::default();
        }
    }
    /// Single-trigger Mono retargets every held voice belonging to this
    /// timbre, retaining EG/LFO timing, oscillator phases and filter memory.
    pub fn legato(&mut self, timbre: u8, note: u8, velocity: u8) -> VoiceMask {
        if timbre as usize >= TIMBRE_COUNT || note > 127 {
            return 0;
        }
        let tag = self.consume_note_event();
        self.sustain_states[timbre as usize].flags &= 127;
        let selected = self
            .allocator
            .retarget_mono(AllocationOwner(timbre as u32 + 1), note);
        self.note_groups
            .assign(selected, ((tag as u32) << 24) | note as u32 | 128);
        for slot in 0..VOICE_COUNT {
            if selected & (1 << slot) == 0 {
                continue;
            }
            let Some(mut voice) = self.voices[slot].take() else {
                continue;
            };
            voice.note = note;
            voice.velocity = velocity;
            voice.held = true;
            self.initialize_group_offsets(slot, timbre);
            if self.voice_group_tables.is_some()
                && let Some(amp) = &mut voice.amplifier
            {
                amp.set_group_gain_bank(self.voice_group_banks[slot]);
            }
            // SYS 01ef78 follows single-trigger retarget as well as allocation.
            radias_synth_domain::lfo::LfoState::initialize_oscillator_random(
                &mut self.modulation_random,
            );
            self.initialize_note_pitch(slot, &mut voice, false);
            let synthesis_note = self.note_pitches[slot].map_or(note, |p| p.note.wrapped);
            if let Some(amplifier) = &mut voice.amplifier {
                amplifier.retarget(synthesis_note, velocity);
            }
            if let Some(auxiliary) = &mut voice.auxiliary {
                auxiliary.retarget(synthesis_note, velocity);
            }
            if let Some(modulation) = &mut voice.modulation {
                modulation.base_pitch_q16 = self.note_pitches[slot]
                    .map_or((note as i32) << 16, |_| modulation.base_pitch_q16);
            }
            self.voices[slot] = Some(voice);
        }
        selected
    }
    pub fn configure_portamento(&mut self, tables: crate::portamento::PortamentoTables) {
        self.portamento_tables = Some(tables);
    }
    /// Accepted controller boot pitch inputs are separate from DSP phase seeds.
    pub fn initialize_controller_pitches(
        &mut self,
        slots: [i32; VOICE_COUNT],
        shared: [i32; TIMBRE_COUNT],
    ) {
        self.assigned_pitch_slots = slots;
        self.shared_assigned_pitch = shared;
    }
    pub fn edit_portamento_program(
        &mut self,
        timbre: u8,
        program: radias_synth_domain::portamento::PortamentoProgram,
    ) {
        let Some(current) = self.portamento_programs.get_mut(timbre as usize) else {
            return;
        };
        *current = Some(program);
        if let Some(tables) = &self.portamento_tables {
            for (slot, voice) in self.voices.iter().enumerate() {
                if voice.as_ref().is_some_and(|v| v.timbre == timbre) {
                    if let Some(port) = &mut self.portamento_voices[slot] {
                        port.edit(
                            program,
                            &tables.rates,
                            self.portamento_switches[timbre as usize],
                        );
                    } else {
                        self.portamento_voices[slot] =
                            Some(crate::portamento::VoicePortamento::new(program));
                    }
                }
            }
        }
    }
    pub fn set_portamento_switch(&mut self, timbre: u8, value: bool) {
        let Some(current) = self.portamento_switches.get_mut(timbre as usize) else {
            return;
        };
        *current = value;
        if let Some(tables) = &self.portamento_tables {
            for (slot, voice) in self.voices.iter().enumerate() {
                if voice.as_ref().is_some_and(|v| v.timbre == timbre)
                    && let Some(port) = &mut self.portamento_voices[slot]
                {
                    port.compile_rate(&tables.rates, value);
                }
            }
        }
    }
    pub fn voice_portamento(&self, slot: usize) -> Option<crate::portamento::VoicePortamento> {
        self.portamento_voices.get(slot).copied().flatten()
    }
    pub fn configure_note_pitch(
        &mut self,
        tables: radias_synth_domain::note_pitch::NotePitchTables,
        scale: radias_synth_domain::note_pitch::ScaleContext,
        master_tune: i32,
    ) {
        self.note_pitch_tables = Some(tables);
        self.scale_context = scale;
        self.master_tune = master_tune;
        for (slot, voice) in self.voices.iter().enumerate() {
            if let Some(voice) = voice {
                self.note_pitches[slot] = voice
                    .drum_pitch
                    .or(self.pitch_programs[voice.timbre as usize])
                    .and_then(|program| {
                        program.initialize(
                            if voice.drum_pitch.is_some() {
                                60
                            } else {
                                voice.note
                            },
                            scale,
                            &tables,
                            &mut self.modulation_random,
                        )
                    })
                    .map(|note| crate::note_pitch::VoiceNotePitch { note });
            }
        }
    }
    pub fn edit_pitch_program(
        &mut self,
        timbre: u8,
        program: radias_synth_domain::note_pitch::PitchProgram,
    ) {
        let Some(current) = self.pitch_programs.get_mut(timbre as usize) else {
            return;
        };
        let previous = current.replace(program);
        if let Some(tables) = &self.note_pitch_tables {
            for (slot, voice) in self.voices.iter().enumerate() {
                let Some(voice) = voice
                    .as_ref()
                    .filter(|v| v.timbre == timbre && v.drum_pitch.is_none())
                else {
                    continue;
                };
                if self.note_pitches[slot].is_none()
                    || previous.is_none_or(|p| p.transpose != program.transpose)
                {
                    self.note_pitches[slot] = program
                        .initialize(
                            voice.note,
                            self.scale_context,
                            tables,
                            &mut self.modulation_random,
                        )
                        .map(|note| crate::note_pitch::VoiceNotePitch { note });
                }
            }
        }
    }
    pub fn set_midi_pitch(&mut self, timbre: u8, midi: crate::note_pitch::MidiPitch) {
        if let Some(state) = self.midi_pitch.get_mut(timbre as usize) {
            *state = midi;
        }
    }
    pub fn midi_pitch(&self, timbre: u8) -> Option<crate::note_pitch::MidiPitch> {
        self.midi_pitch.get(timbre as usize).copied()
    }
    pub fn voice_note_pitch(&self, slot: usize) -> Option<crate::note_pitch::VoiceNotePitch> {
        self.note_pitches.get(slot).copied().flatten()
    }
    fn initialize_note_pitch(
        &mut self,
        slot: usize,
        voice: &mut ActiveVoice,
        inherit_timbre: bool,
    ) {
        self.note_pitches[slot] = self.note_pitch_tables.as_ref().and_then(|tables| {
            voice
                .drum_pitch
                .or(self.pitch_programs[voice.timbre as usize])
                .and_then(|program| {
                    program
                        .initialize(
                            if voice.drum_pitch.is_some() {
                                60
                            } else {
                                voice.note
                            },
                            self.scale_context,
                            tables,
                            &mut self.modulation_random,
                        )
                        .map(|note| crate::note_pitch::VoiceNotePitch { note })
                })
        });
        let timbre = voice.timbre as usize;
        self.portamento_voices[slot] = self.portamento_tables.as_ref().and_then(|tables| {
            if voice.drum_pitch.is_some() {
                return None;
            }
            self.portamento_programs[timbre].map(|program| {
                let mut port = self.portamento_voices[slot]
                    .unwrap_or_else(|| crate::portamento::VoicePortamento::new(program));
                port.program = program;
                let note = self.note_pitches[slot].map_or(voice.note, |p| p.note.wrapped);
                port.note_on(
                    note,
                    self.assigned_pitch_slots[slot],
                    inherit_timbre.then_some(self.shared_assigned_pitch[timbre]),
                    &tables.rates,
                    self.portamento_switches[timbre],
                );
                port
            })
        });
        let port_offset = self.portamento_voices[slot].map_or(0, |p| p.state.current_q16);
        let note = self.note_pitches[slot].map_or(voice.note, |p| p.note.wrapped);
        self.assigned_pitch_slots[slot] = ((note as i32) << 16).wrapping_add(port_offset);
        if let (Some(pitch), Some(program), Some(tables), Some(modulation)) = (
            self.note_pitches[slot],
            voice
                .drum_pitch
                .or(self.pitch_programs[voice.timbre as usize]),
            &self.note_pitch_tables,
            &mut voice.modulation,
        ) {
            let midi = self.midi_pitch[voice.timbre as usize];
            modulation.base_pitch_q16 = if voice.drum_pitch.is_some() {
                pitch.drum_base(program, tables, self.master_tune)
            } else {
                pitch.base(program, midi, tables, self.master_tune)
            }
            .wrapping_add(self.voice_group_offsets[slot].tuning_q16)
            .wrapping_add(port_offset);
            modulation.vibrato_depth = program.vibrato_depth(midi.wheel, &tables.vibrato);
        }
    }
    pub fn configure_noise(&mut self, tables: crate::noise::NoiseTables) {
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            if let Some(voice) = voice {
                Self::initialize_noise(&tables, slot, voice);
            }
        }
        self.noise_tables = Some(tables);
    }

    fn initialize_noise(tables: &crate::noise::NoiseTables, slot: usize, voice: &mut ActiveVoice) {
        let Some(program) = voice.primary else { return };
        let Some(code) =
            radias_synth_domain::pitch::PitchCode::new(voice.renderer.primary_pitch_code())
        else {
            return;
        };
        if let Some(control) = tables.compile(program, code, 0, [0; 2]) {
            voice.renderer.voice.primary.noise = Default::default();
            voice.renderer.voice.primary.formant = radias_synth_domain::noise::FormantState {
                counter: tables.counters.for_slot(slot as u8),
                filter: Default::default(),
            };
            voice.renderer.configure_noise_control(control);
        }
    }

    fn cache_retired_voice(&mut self, slot: usize) {
        // Standalone reference scenes can drive their own original retirement
        // timing. Automatic lifecycle handling belongs to the configured player.
        if self.noise_tables.is_some()
            && let Some(voice) = &self.voices[slot]
        {
            self.set_stereo_cache_decay(slot, voice.renderer.envelope_rate());
        }
    }
    /// Reset the two physical frame banks from accepted DSP boot input words.
    /// Normal note-off, stealing and parameter changes retain this memory.
    pub fn initialize_physical_frames(
        &mut self,
        seeds: [radias_synth_domain::noise::NoiseFrameSeeds; 2],
    ) {
        self.physical_frames = core::array::from_fn(|slot| {
            Some(
                radias_synth_domain::voice_frame::VoiceFrameState::from_boot(
                    &seeds[slot / VOICES_PER_PROCESSOR],
                    slot % 12,
                ),
            )
        });
    }
    /// Production scheduling uses the original shared TMU0 cadence. The
    /// legacy explicitly timed coefficient fixtures keep their own clock.
    pub fn configure_controller_service(
        &mut self,
        timer: radias_synth_domain::controller_service::ControllerServiceTimer,
    ) {
        self.controller_timer = Some(timer);
        self.controller_interrupts = 0;
        for (slot, voice) in self.voices.iter().enumerate() {
            self.controller_flags[slot] = if voice.is_some() { 0x80 } else { 0 };
        }
    }
    pub fn configure_amplifier_delivery(
        &mut self,
        rates: radias_synth_domain::amplifier_delivery::AmplifierRateTable,
    ) {
        self.amplifier_rates = Some(rates);
    }
    pub fn amplifier_delivery_state(&self) -> (usize, bool) {
        (
            self.amplifier_transport.pending(),
            self.amplifier_transport_failed,
        )
    }
    fn deliver_amplifier(&mut self, clock: u64) {
        let voices = &mut self.voices;
        let bound = &mut self.amplifier_bound;
        let soft_binding = &self.amplifier_soft_binding;
        self.amplifier_transport
            .advance_until(clock, |_, slot, packet| {
                let Some(voice) = &mut voices[slot] else {
                    return;
                };
                match packet {
                    crate::synthesis_transport::DeliveredSynthesisParameter::ActorState{opcode,active}=>{
                        // Full constructor composition is not enabled yet. Its
                        // explicit activation/detach flag publication is kept
                        // separate from ordinary AMP target delivery.
                        if opcode==0 || opcode==38 {bound[slot]=active;}
                    },
                    crate::synthesis_transport::DeliveredSynthesisParameter::Amplifier(packet)=>match packet {
                    radias_synth_domain::amplifier_delivery::AmplifierPacket::RateAndTarget {
                        rate,
                        target,
                    } => {
                        voice.renderer.set_envelope_rate(rate as i16);
                        voice.renderer.set_envelope_target(target);
                        bound[slot] = true;
                    }
                    radias_synth_domain::amplifier_delivery::AmplifierPacket::Target(target) => {
                        voice.renderer.set_envelope_target(target)
                    }
                    },
                    crate::synthesis_transport::DeliveredSynthesisParameter::Filter1(coefficients)=>{
                        if voice.renderer.rendered_frames()==0 {voice.renderer.set_filter_immediate(coefficients);}
                        else {voice.renderer.set_filter(coefficients);}
                    }
                    crate::synthesis_transport::DeliveredSynthesisParameter::Filter2(coefficients)=>{
                        if !soft_binding[slot] && voice.renderer.rendered_frames()==0 {voice.renderer.set_filter2_immediate(coefficients);}
                        else {voice.renderer.set_filter2_target(coefficients);}
                    }
                    crate::synthesis_transport::DeliveredSynthesisParameter::Pitch{primary,secondary,noise_pitch}=>{
                        if let Some((code,increment,bandwidth))=primary {
                            voice.renderer.set_primary_pitch_code(radias_synth_domain::pitch::PitchCode::new(code).expect("Native controller pitch range"));
                            voice.renderer.set_pitch(increment,bandwidth);
                            if let(Some(coefficient),Some(control))=(noise_pitch,voice.renderer.noise_control_mut()){control.receive_pitch(increment,coefficient);}
                        }
                        voice.renderer.set_secondary_coefficients(secondary);
                    }
                    crate::synthesis_transport::DeliveredSynthesisParameter::UnisonDetune(value)=>voice.renderer.set_primary_waveform_control(value),
                    crate::synthesis_transport::DeliveredSynthesisParameter::SecondarySync(value)=>voice.renderer.set_secondary_sync(value),
                    crate::synthesis_transport::DeliveredSynthesisParameter::NoiseShape{input_gain,feedback}=>voice.renderer.set_noise_target(
                        crate::noise::NoiseTarget::FormantShape{input_gain,feedback},false),
                    crate::synthesis_transport::DeliveredSynthesisParameter::PrimaryInitialization{parameters,..}=>voice.renderer.receive_primary_parameters(parameters),
                    crate::synthesis_transport::DeliveredSynthesisParameter::Scalar(parameter,value)=>match parameter {
                        crate::synthesis_transport::ScalarParameter::Mixer(band)=>voice.renderer.set_mixer_band(band,value),
                        crate::synthesis_transport::ScalarParameter::PrimaryControl=>voice.renderer.set_primary_waveform_control(value),
                        crate::synthesis_transport::ScalarParameter::PrimaryRatio=>voice.renderer.set_primary_ratio(value),
                        crate::synthesis_transport::ScalarParameter::Pan=>voice.renderer.set_pan_target(value),
                        crate::synthesis_transport::ScalarParameter::ShaperDepth=>voice.renderer.set_shaper_depth(value),
                        crate::synthesis_transport::ScalarParameter::NoiseGain=>voice.renderer.set_noise_target(
                            crate::noise::NoiseTarget::ExcitationGain(value),
                            !soft_binding[slot] && voice.renderer.rendered_frames()==0),
                        crate::synthesis_transport::ScalarParameter::NoiseFrequency=>voice.renderer.set_noise_target(
                            crate::noise::NoiseTarget::ExcitationBias(value),
                            !soft_binding[slot] && voice.renderer.rendered_frames()==0),
                    },
                }
            });
    }
    pub fn configure_constructor_filter_mix(
        &mut self,
        table: radias_synth_domain::filter_control::FilterMixTable,
    ) {
        self.amplifier_transport
            .configure_constructor_filter_mix(table);
    }
    pub fn configure_pitch_delivery(
        &mut self,
        rom: [radias_synth_domain::pitch_receiver::PitchReceiverRom; 2],
        dispatch: radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable,
    ) {
        self.amplifier_transport
            .configure_pitch_receivers(rom, dispatch);
    }
    pub fn controller_service_state(
        &self,
    ) -> Option<(
        radias_synth_domain::controller_service::ControllerServiceTimer,
        u64,
    )> {
        self.controller_timer
            .map(|timer| (timer, self.controller_interrupts))
    }
    pub fn physical_frame(
        &self,
        slot: usize,
    ) -> Option<radias_synth_domain::voice_frame::VoiceFrameState> {
        self.physical_frames[slot]
    }
    pub fn set_stereo_cache_decay(&mut self, slot: usize, delta: i16) {
        if let Some(frame) = &mut self.physical_frames[slot] {
            frame
                .stereo_cache
                .initialize(frame.last_amplified, frame.last_pan_current, delta);
            frame.last_amplified = Sample(0);
        }
    }
    pub fn configure_comb(
        &mut self,
        tables: radias_synth_domain::controller_comb::CombControlTables,
    ) {
        self.comb_tables = Some(tables);
    }
    pub fn comb_tables(&self) -> Option<&radias_synth_domain::controller_comb::CombControlTables> {
        self.comb_tables.as_ref()
    }
    pub fn configure_filter2(
        &mut self,
        tables: radias_synth_domain::controller_filter2::Filter2ControlTables,
    ) {
        self.filter2_tables = Some(tables);
    }
    pub fn edit_filter2_program(
        &mut self,
        timbre: u8,
        program: Option<crate::filter2::Filter2Program>,
    ) {
        if let Some(current) = self.filter2_programs.get_mut(timbre as usize) {
            *current = program;
        }
    }
    pub fn edit_comb_program(&mut self, timbre: u8, program: Option<crate::comb::CombProgram>) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            voice.comb_program = program;
        }
    }
    pub fn configure_secondary(
        &mut self,
        table: radias_synth_domain::controller_secondary::FineTuneTable,
    ) {
        self.secondary_table = Some(table);
    }
    pub fn edit_primary_control(&mut self, timbre: u8, program: crate::primary::PrimaryProgram) {
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(voice) = voice.as_mut().filter(|v| v.timbre == timbre) else {
                continue;
            };
            let changed = voice
                .primary
                .is_none_or(|old| old.selection != program.selection);
            if program.selection & 0x30 == 0x20
                && voice
                    .primary
                    .is_none_or(|old| old.selection != program.selection)
            {
                let mut control = program.control;
                control.control2_modulation = voice
                    .modulation
                    .as_ref()
                    .map_or(0, |m| m.patches.targets.controls[14]);
                voice
                    .renderer
                    .update_unison_phases(control, program.selection & 3 == 2);
            }
            voice.primary = Some(program);
            if changed && let Some(tables) = &self.noise_tables {
                Self::initialize_noise(tables, slot, voice);
            }
        }
    }
    pub fn edit_primary(
        &mut self,
        timbre: u8,
        program: usize,
        parameters: radias_synth_domain::primary_oscillator::PrimaryParameters,
    ) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            voice.program = program;
            voice.renderer.select_primary(parameters);
        }
    }
    pub fn edit_secondary(&mut self, timbre: u8, program: crate::secondary::SecondaryProgram) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            voice.secondary = Some(program);
            let prior_sync = voice.renderer.secondary_sync();
            voice.renderer.select_secondary(program);
            if self.amplifier_transport.pitch_enabled() {
                voice.renderer.set_secondary_sync(prior_sync);
            }
        }
    }
    pub fn configure_mixer(&mut self, scales: radias_synth_domain::controller_mixer::MixerScales) {
        self.mixer_scales = Some(scales);
    }
    pub fn edit_mixer(&mut self, timbre: u8, program: crate::mixer::MixerProgram) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            voice.mixer = Some(program);
        }
    }
    pub fn configure_pan(
        &mut self,
        tables: radias_synth_domain::controller_pan::PanTables,
        weights: radias_synth_domain::control_slew::SlewWeights,
    ) {
        self.pan_tables = Some((tables, weights));
    }
    pub fn edit_pan(&mut self, timbre: u8, pan: radias_synth_domain::controller_pan::PanControl) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            let mut pan = pan;
            if voice.uses_program_common
                && let Some(common) = self.program_common
            {
                pan.midi_pan = Some(common.pan);
            }
            voice.pan = Some(pan);
        }
    }
    pub fn edit_amplifier_level(&mut self, timbre: u8, level: u8, tables: &ControllerTables) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if let Some(amplifier) = &mut voice.amplifier {
                amplifier.edit_level(level, tables);
            }
        }
    }
    pub fn edit_amplifier_program(
        &mut self,
        timbre: u8,
        program: crate::amplifier::AmplifierProgram,
        tables: &ControllerTables,
    ) {
        for (slot, actor) in self.voices.iter_mut().enumerate() {
            let Some(voice) = actor.as_mut().filter(|v| v.timbre == timbre) else {
                continue;
            };
            if let Some(amplifier) = &mut voice.amplifier {
                let mut program = program;
                if voice.uses_program_common
                    && let Some(common) = self.program_common
                {
                    program.midi_volume = Some(common.level);
                }
                amplifier.edit_program(program, tables);
                if self.voice_group_tables.is_some() {
                    amplifier.set_group_gain_bank(self.voice_group_banks[slot]);
                }
            }
        }
    }
    pub fn controller_filter_tables(
        &mut self,
        tables: radias_synth_domain::controller_filter::ControllerFilterTables,
    ) {
        self.filter_tables = Some(tables);
    }
    pub fn edit_auxiliary(
        &mut self,
        timbre: u8,
        programs: [crate::voice_envelopes::ModEnvelopeProgram; 2],
        tables: &ControllerTables,
    ) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if let Some(auxiliary) = &mut voice.auxiliary {
                auxiliary.edit(programs, tables);
            }
        }
    }
    pub fn edit_dynamic_filter(
        &mut self,
        timbre: u8,
        filter: crate::voice_envelopes::DynamicFilter,
    ) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if let Some(auxiliary) = &mut voice.auxiliary {
                auxiliary.filter = Some(filter);
            }
        }
    }
    pub fn enable_tempo_clock(
        &mut self,
        tables: radias_synth_domain::lfo_tempo::LfoTempoTables,
        tempo: u16,
    ) {
        let mut clock = crate::clock::InstrumentClock::internal(tables, tempo);
        for (slot, states) in self.controller_slots.iter().enumerate() {
            for (i, state) in states.iter().enumerate() {
                clock.bank.voices[slot][i].phase = state.phase;
            }
            if let Some(modulation) = self.voices[slot]
                .as_ref()
                .and_then(|voice| voice.modulation.as_ref())
            {
                clock.divisions.voices[slot] = modulation.tempo_divisions;
            }
        }
        for (timbre, pair) in self.shared_lfo.iter().enumerate() {
            for i in 0..LFO_COUNT {
                clock.bank.timbres[timbre][synthesis_clock_slot(i)].phase =
                    pair.synthesis.states[i].phase;
            }
            for i in 0..2 {
                clock.bank.timbres[timbre][i + 2].phase = pair.effects[i].state.phase;
                clock.divisions.timbres[timbre][i + 2] =
                    self.effect_lfo_parameters[timbre][i].beat & 31;
            }
        }
        clock.bank.global.phase = self.global_lfo.state.phase;
        clock.divisions.global = self.global_lfo_parameters.beat & 31;
        clock
            .bank
            .compile_rates(&clock.divisions, clock.receiver.tempo, &clock.tables);
        self.clock = Some(clock);
    }
    pub fn set_tempo(&mut self, tempo: u16) {
        if let Some(clock) = &mut self.clock {
            clock.set_program_tempo(tempo);
        }
    }
    pub fn tempo_clock(&self) -> Option<&crate::clock::InstrumentClock> {
        self.clock.as_ref()
    }
    /// Deliver an accepted controller clock pulse to all 65 physical states.
    /// Source selection and interval measurement belong to InstrumentClock.
    pub fn clock_pulse(&mut self, pulse: crate::clock::ClockPulse) {
        if let Some(clock) = &mut self.clock {
            clock.bank.pulse(pulse);
        }
    }
    /// Native note use case owns allocation and same-program state retention.
    pub fn trigger(&mut self, mut voice: ActiveVoice, cost: u16) -> Option<VoiceAssignment> {
        let tag = self.consume_note_event();
        if !self.voice_modes[voice.timbre as usize].polyphonic {
            self.sustain_states[voice.timbre as usize].flags &= 127;
        }
        let assignment = if self.voice_modes[voice.timbre as usize].polyphonic {
            self.allocator
                .allocate_poly(AllocationOwner(voice.timbre as u32 + 1), voice.note, cost)
        } else {
            self.allocator
                .allocate_mono(AllocationOwner(voice.timbre as u32 + 1), voice.note, cost)
        }?;
        self.note_groups.assign(
            1 << assignment.slot,
            ((tag as u32) << 24) | voice.note as u32 | 128,
        );
        self.initialize_single_group(assignment.slot as usize, voice.timbre);
        self.initialize_note_pitch(assignment.slot as usize, &mut voice, true);
        self.install_retaining(voice, assignment);
        self.initialize_fresh_waveform_phase(assignment.slot as usize);
        Some(assignment)
    }
    /// Allocate first, then initialize from the selected physical controller
    /// slot. Rejected allocations do not consume the instrument random seed.
    pub fn trigger_modulated(
        &mut self,
        voice: ActiveVoice,
        cost: u16,
        program: ModulationProgram,
    ) -> Option<VoiceAssignment> {
        if (voice.timbre as usize) < TIMBRE_COUNT
            && self.voice_group_programs[voice.timbre as usize].raw & 128 != 0
        {
            return self.trigger_group_modulated(voice, cost, program);
        }
        self.trigger_modulated_with_inheritance(voice, cost, program, true)
    }
    /// SYS007DA8 uses fresh allocation for an independent drum body, including
    /// exclusive retirement before allocation. Timbre Mono/Unison do not turn
    /// this one-instrument assignment into the ordinary note path.
    pub fn trigger_drum_modulated(
        &mut self,
        voice: ActiveVoice,
        cost: u16,
        program: ModulationProgram,
        exclusive_group: u8,
    ) -> Option<VoiceAssignment> {
        if voice.timbre as usize >= TIMBRE_COUNT || voice.note > 127 || voice.drum_pitch.is_none() {
            return None;
        }
        program.validate_with_clock(self.clock.is_some()).ok()?;
        let tag = self.consume_note_event();
        let event = ((tag as u32) << 24) | voice.note as u32 | 128;
        self.shared_lfo[voice.timbre as usize].synthesis.parameters = program.lfo;
        self.retire_drum_group(voice.timbre, event, exclusive_group);
        let owner = AllocationOwner(voice.timbre as u32 + 1);
        let previous_claims = self.allocator.claims;
        let assignment = self.allocator.allocate_poly(owner, voice.note, cost)?;
        self.repair_poly_allocation(
            voice.timbre,
            voice.primary.map_or(0, |p| p.selection),
            1 << assignment.slot,
            assignment.displaced,
            &previous_claims,
        );
        self.note_groups.assign(1 << assignment.slot, event);
        self.initialize_single_group(assignment.slot as usize, voice.timbre);
        self.initialize_modulated_actor(voice, assignment, program, false, true);
        self.bind_drum_group(assignment.slot as usize, exclusive_group);
        Some(assignment)
    }
    fn trigger_group_modulated(
        &mut self,
        voice: ActiveVoice,
        cost: u16,
        program: ModulationProgram,
    ) -> Option<VoiceAssignment> {
        program.validate_with_clock(self.clock.is_some()).ok()?;
        let layout = self.voice_group_programs[voice.timbre as usize]
            .layout(voice.primary.map_or(0, |p| p.selection));
        if layout.bank >= 8 || self.voice_group_tables.is_none() || voice.note > 127 {
            return None;
        }
        let tag = self.consume_note_event();
        let owner = AllocationOwner(voice.timbre as u32 + 1);
        let previous_claims = self.allocator.claims;
        let group = if self.voice_modes[voice.timbre as usize].polyphonic {
            self.allocator.allocate_poly_group(
                owner,
                voice.timbre,
                voice.note,
                cost,
                layout,
                &mut self.voice_group_slots,
            )
        } else {
            self.sustain_states[voice.timbre as usize].flags &= 127;
            self.allocator.allocate_mono_group(
                owner,
                voice.timbre,
                voice.note,
                cost,
                layout,
                &self.note_groups.ages,
                &mut self.voice_group_slots,
            )
        }?;
        if self.voice_modes[voice.timbre as usize].polyphonic {
            self.repair_poly_allocation(
                voice.timbre,
                voice.primary.map_or(0, |p| p.selection),
                group.selected,
                group.displaced,
                &previous_claims,
            );
        }
        self.note_groups.assign(
            group.selected,
            ((tag as u32) << 24) | voice.note as u32 | 128,
        );
        self.voice_group_timbre_banks[voice.timbre as usize] = group.bank;
        for slot in 0..VOICE_COUNT {
            if group.displaced & (1 << slot) != 0 {
                self.cache_retired_voice(slot);
                if group.selected & (1 << slot) == 0 {
                    self.voices[slot] = None;
                    self.portamento_voices[slot] = None;
                    self.note_pitches[slot] = None;
                }
            }
        }
        for slot in 0..VOICE_COUNT {
            if group.selected & (1 << slot) != 0 {
                self.voice_group_banks[slot] = group.bank;
                self.initialize_modulated_actor(
                    voice.fresh_note(),
                    VoiceAssignment {
                        slot: slot as u8,
                        displaced: 0,
                    },
                    program,
                    true,
                    true,
                );
            }
        }
        Some(VoiceAssignment {
            slot: group.selected.trailing_zeros() as u8,
            displaced: group.displaced,
        })
    }
    /// Multi-trigger Mono restarts private EG/LFO controls on its held actor.
    /// If that actor was stolen, the original falls back to fresh inheritance.
    pub fn retrigger_modulated(
        &mut self,
        voice: ActiveVoice,
        cost: u16,
        program: ModulationProgram,
    ) -> Option<VoiceAssignment> {
        if voice.timbre as usize >= TIMBRE_COUNT || voice.note > 127 {
            return None;
        }
        program.validate_with_clock(self.clock.is_some()).ok()?;
        let selected = self.allocator.retrigger_mono(
            AllocationOwner(voice.timbre as u32 + 1),
            voice.note,
            cost,
        );
        if selected == 0 {
            return self.trigger_modulated(voice, cost, program);
        }
        let tag = self.consume_note_event();
        self.sustain_states[voice.timbre as usize].flags &= 127;
        self.note_groups
            .assign(selected, ((tag as u32) << 24) | voice.note as u32 | 128);
        let first = selected.trailing_zeros() as u8;
        for slot in 0..VOICE_COUNT {
            if selected & (1 << slot) != 0 {
                let assignment = VoiceAssignment {
                    slot: slot as u8,
                    displaced: 0,
                };
                // Held Mono Multi restarts controller EG/LFO state without
                // the fresh DSP parameter-block reset(D534). Retain private
                // Filter1 history and the DSP amplitude smoother for each actor.
                let dsp_memory = self.voices[slot].as_ref().map(|v| {
                    (
                        v.renderer.voice.filter.state,
                        v.renderer.voice.envelope,
                        v.renderer.envelope_rate(),
                        v.renderer.envelope_target(),
                        v.renderer.current_filter(),
                        v.renderer.current_pitch(),
                        v.renderer.voice.secondary,
                        v.renderer.pitch_state(),
                        v.renderer.scalar_state(),
                        v.renderer.filter2_state(),
                        v.renderer.shaper_state(),
                    )
                });
                let mut restarted = voice.fresh_note();
                if self.amplifier_transport.pitch_enabled()
                    && let Some(comb) = self.voices[slot]
                        .as_mut()
                        .and_then(|v| v.renderer.comb.take())
                {
                    // Held Mono does not reset external Comb RAM or DMA phase.
                    restarted.renderer.comb = Some(comb);
                }
                self.initialize_modulated_actor(restarted, assignment, program, false, false);
                if let (
                    Some((
                        filter,
                        envelope,
                        rate,
                        target,
                        coefficients,
                        pitch,
                        secondary,
                        pitch_state,
                        scalar_state,
                        filter2_state,
                        shaper_state,
                    )),
                    Some(current),
                ) = (dsp_memory, &mut self.voices[slot])
                {
                    current.renderer.voice.filter.state = filter;
                    current.renderer.voice.envelope = envelope;
                    if self.amplifier_rates.is_some() {
                        current.renderer.set_envelope_rate(rate);
                        current.renderer.set_envelope_target(target);
                        current.renderer.set_filter_immediate(coefficients);
                        if self.amplifier_transport.pitch_enabled() {
                            current.renderer.set_primary_pitch_code(
                                radias_synth_domain::pitch::PitchCode::new(pitch.0).unwrap(),
                            );
                            current.renderer.set_pitch(pitch.1, pitch.2);
                            current.renderer.voice.secondary = secondary;
                            current.renderer.restore_pitch_state(pitch_state);
                            current.renderer.restore_scalar_state(scalar_state);
                            current.renderer.restore_filter2_state(filter2_state);
                            current.renderer.restore_shaper_state(shaper_state);
                        }
                        // SYS01e1a8 keeps the parameter block running while
                        // its final002978 soft binding packet is delivered.
                        self.amplifier_bound[slot] = true;
                        self.amplifier_targets[slot] = Some(target);
                    }
                }
            }
        }
        Some(VoiceAssignment {
            slot: first,
            displaced: 0,
        })
    }
    fn trigger_modulated_with_inheritance(
        &mut self,
        voice: ActiveVoice,
        cost: u16,
        program: ModulationProgram,
        inherit: bool,
    ) -> Option<VoiceAssignment> {
        if voice.timbre as usize >= TIMBRE_COUNT || voice.note > 127 {
            return None;
        }
        program.validate_with_clock(self.clock.is_some()).ok()?;
        let tag = self.consume_note_event();
        let previous_claims = self.allocator.claims;
        if !self.voice_modes[voice.timbre as usize].polyphonic {
            self.sustain_states[voice.timbre as usize].flags &= 127;
        }
        let assignment = if self.voice_modes[voice.timbre as usize].polyphonic {
            self.allocator
                .allocate_poly(AllocationOwner(voice.timbre as u32 + 1), voice.note, cost)
        } else {
            self.allocator
                .allocate_mono(AllocationOwner(voice.timbre as u32 + 1), voice.note, cost)
        }?;
        if self.voice_modes[voice.timbre as usize].polyphonic {
            self.repair_poly_allocation(
                voice.timbre,
                voice.primary.map_or(0, |p| p.selection),
                1 << assignment.slot,
                assignment.displaced,
                &previous_claims,
            );
        }
        self.note_groups.assign(
            1 << assignment.slot,
            ((tag as u32) << 24) | voice.note as u32 | 128,
        );
        self.initialize_single_group(assignment.slot as usize, voice.timbre);
        self.initialize_modulated_actor(voice, assignment, program, inherit, true);
        Some(assignment)
    }
    fn initialize_modulated_actor(
        &mut self,
        mut voice: ActiveVoice,
        assignment: VoiceAssignment,
        program: ModulationProgram,
        inherit: bool,
        fresh_parameter_block: bool,
    ) {
        self.initialize_group_offsets(assignment.slot as usize, voice.timbre);
        if self.voice_group_tables.is_some()
            && let Some(amp) = &mut voice.amplifier
        {
            amp.set_group_gain_bank(self.voice_group_banks[assignment.slot as usize]);
        }
        let prior = self.controller_slots[assignment.slot as usize];
        voice.modulation = VoiceModulation::from_prior_with_clock(
            program,
            (voice.note as i32) << 16,
            self.shared_lfo[voice.timbre as usize].synthesis.states,
            prior,
            &mut self.modulation_random,
            self.clock.is_some(),
        )
        .ok();
        if let (Some(clock), Some(modulation)) = (&mut self.clock, &voice.modulation) {
            let slot = assignment.slot as usize;
            clock.divisions.voices[slot] = program.tempo_divisions;
            for i in 0..LFO_COUNT {
                clock.bank.voices[slot][i].previous_increment = clock
                    .tables
                    .compile_increment(
                        (program.tempo_divisions[i] & 31) as i32,
                        0,
                        clock.receiver.tempo.clock_rate(),
                    )
                    .1;
                clock.bank.voices[slot][i].initialize_note(
                    program.lfo[i].phase_sync,
                    program.tempo_divisions[i],
                    clock.bank.timbres[voice.timbre as usize][synthesis_clock_slot(i)],
                );
                clock.bank.voices[slot][i].phase = modulation.pair.states[i].phase;
            }
        }
        // SYS 01ef78 follows private LFO note initialization. Its three random
        // words advance the common seed for every waveform.
        radias_synth_domain::lfo::LfoState::initialize_oscillator_random(
            &mut self.modulation_random,
        );
        self.initialize_note_pitch(assignment.slot as usize, &mut voice, inherit);
        let parameter_state = (!fresh_parameter_block).then(|| {
            self.amplifier_transport
                .parameter_state(assignment.slot as usize)
        });
        self.install_retaining(voice, assignment);
        if let Some(state) = parameter_state {
            self.amplifier_transport
                .restore_parameters(assignment.slot as usize, state);
        }
        self.amplifier_soft_binding[assignment.slot as usize] = !fresh_parameter_block;
        if fresh_parameter_block {
            self.initialize_fresh_waveform_phase(assignment.slot as usize);
        }
    }
    fn install_retaining(&mut self, mut voice: ActiveVoice, assignment: VoiceAssignment) {
        if let Some(mut state) = self.retained_state(assignment.slot as usize, voice.program) {
            if voice.amplifier.is_some() {
                state.envelope.0 = 0;
            }
            state.filter = voice.renderer.voice.filter;
            voice.renderer.voice = state;
        }
        self.install(assignment.slot as usize, assignment.displaced, voice);
    }
    pub fn install(&mut self, slot: usize, displaced: VoiceMask, mut voice: ActiveVoice) {
        self.controller_flags[slot] = 0xc0;
        self.initial_controller_serviced[slot] = false;
        self.secondary_sync_targets[slot] = voice.secondary.map(|p| p.modulation().sync);
        self.mixer_delivery_targets[slot] = None;
        self.pan_delivery_targets[slot] = None;
        self.ratio_delivery_targets[slot] = None;
        self.amplifier_fresh[slot] = voice.amplifier.is_some();
        self.filter_delivery_targets[slot] = None;
        self.filter2_delivery_resonance[slot] = None;
        self.comb_feedback_delivery_targets[slot] = None;
        self.amplifier_transport.reset_filter(slot);
        self.amplifier_transport.reset_filter2(slot);
        self.controller_pitch_codes[slot] = voice.renderer.primary_pitch_code();
        let offset = voice
            .secondary
            .zip(self.secondary_table.as_ref())
            .map_or(0, |(secondary, table)| secondary.pitch.relative_code(table));
        self.amplifier_transport.reset_pitch(
            slot,
            voice.renderer.primary_pitch_code(),
            offset,
            voice.secondary.is_some_and(|p| p.modulation().sync),
            voice
                .primary
                .and_then(|p| p.waveform_control(0, [0; 2]))
                .unwrap_or(0),
        );
        self.amplifier_soft_binding[slot] = false;
        self.amplifier_targets[slot] = None;
        self.amplifier_bound[slot] = self.amplifier_rates.is_none() || voice.amplifier.is_none();
        if let Some(rates) = &self.amplifier_rates
            && voice.amplifier.is_some()
        {
            voice.renderer.set_envelope_target(0);
            voice.renderer.set_envelope_rate(rates.reset_rate as i16);
        }
        self.drum_groups.groups[slot] = 0;
        if voice.uses_program_common
            && let Some(common) = self.program_common
        {
            if let Some(amplifier) = &mut voice.amplifier {
                amplifier.set_program_common(Some(common.level));
            }
            if let Some(pan) = &mut voice.pan {
                pan.midi_pan = Some(common.pan);
            }
        }
        for index in 0..VOICE_COUNT {
            if displaced & (1 << index) != 0 {
                self.cache_retired_voice(index);
            }
        }
        if let Some(frame) = self.physical_frames[slot] {
            frame.restore(&mut voice.renderer.voice);
            if let Some(primary) = voice.primary
                && primary.selection & 0x30 == 0x20
            {
                voice.renderer.initialize_unison_phases(
                    primary.control.phase_code(),
                    primary.selection & 3 == 2,
                );
            }
        }
        if let Some(tables) = &self.noise_tables {
            Self::initialize_noise(tables, slot, &mut voice);
        }
        if let Some(secondary) = voice.secondary {
            voice.renderer.select_secondary(secondary);
        }
        if let (Some(scales), Some(mixer)) = (&self.mixer_scales, voice.mixer) {
            voice
                .renderer
                .initialize_mixer(mixer.compile(scales, [0; 3]));
            voice.renderer.mixer_slew_phase(3);
        }
        if let (Some((tables, weights)), Some(mut pan)) = (&self.pan_tables, voice.pan) {
            pan.timbre_offset = pan
                .timbre_offset
                .wrapping_add(self.voice_group_offsets[slot].pan);
            let target = tables.compile(pan.target()) as i16;
            voice.renderer.initialize_pan(
                radias_synth_domain::pan::PanSmoother {
                    current: target,
                    target,
                },
                *weights,
            );
        }
        if let Some(modulation) = &voice.modulation {
            self.controller_slots[slot] = modulation.pair.states;
            self.controller_slot_valid[slot] = true;
        }
        for (index, current) in self.voices.iter_mut().enumerate() {
            if displaced & (1 << index) != 0 {
                *current = None;
                if index != slot {
                    self.portamento_voices[index] = None;
                    self.note_pitches[index] = None;
                }
            }
        }
        #[cfg(feature = "web-modular")]
        {
            voice.renderer.circuit = self.circuits[voice.timbre as usize]
                .as_ref()
                .map(|p| p.fresh());
        }
        self.voices[slot] = Some(voice);
    }
    #[cfg(feature = "web-modular")]
    pub fn set_circuit(
        &mut self,
        timbre: usize,
        prototype: Option<alloc::boxed::Box<dyn crate::VoiceCircuit>>,
    ) {
        for v in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre as usize == timbre)
        {
            match (&mut v.renderer.circuit, &prototype) {
                (Some(current), Some(next)) => current.reconfigure(&**next),
                (current, next) => *current = next.as_ref().map(|p| p.fresh()),
            }
        }
        self.circuits[timbre] = prototype;
    }
    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }
    pub fn set_drum_gain(&mut self, gain: f64) {
        self.drum_gain = gain;
    }
    pub fn steal_oldest_voice(&mut self) -> bool {
        let slot = self
            .allocator
            .order
            .0
            .iter()
            .copied()
            .map(usize::from)
            .find(|&slot| self.voices[slot].is_some());
        if let Some(slot) = slot {
            self.remove(slot);
            true
        } else {
            false
        }
    }
    pub fn held_count(&self) -> usize {
        self.voices.iter().flatten().filter(|v| v.held).count()
    }
    pub fn active_voice(&self, slot: usize) -> Option<&ActiveVoice> {
        self.voices[slot].as_ref()
    }
    /// Fresh parameter-block phases belong to the selected physical actor;
    /// held Mono reuse retains its DSP phase memory.
    fn initialize_fresh_waveform_phase(&mut self, slot: usize) {
        if let Some(voice) = self.voices[slot].as_mut()
            && let Some(primary) = voice.primary
        {
            voice
                .renderer
                .voice
                .primary
                .initialize_waveform_phase(primary.selection);
        }
    }
    pub fn retained_state(
        &self,
        slot: usize,
        program: usize,
    ) -> Option<radias_synth_domain::voice::Voice> {
        self.voices[slot]
            .as_ref()
            .filter(|v| v.program == program)
            .map(|v| v.renderer.voice)
    }
    pub fn retained_modulation_state(
        &self,
        slot: usize,
    ) -> Option<[radias_synth_domain::lfo::LfoState; LFO_COUNT]> {
        self.controller_slot_valid
            .get(slot)
            .copied()
            .unwrap_or(false)
            .then(|| self.controller_slots[slot])
    }
    pub fn set_timbre_modulation_active(&mut self, timbre: u8, active: bool) {
        if let Some(value) = self.timbre_modulation_active.get_mut(timbre as usize) {
            *value = active;
        }
    }
    pub fn stop(&mut self) {
        for slot in 0..VOICE_COUNT {
            self.cache_retired_voice(slot);
        }
        self.voices = core::array::from_fn(|_| None);
        self.controller_flags.fill(0);
        self.initial_controller_serviced.fill(false);
        self.secondary_sync_targets.fill(None);
        self.mixer_delivery_targets.fill(None);
        self.pan_delivery_targets.fill(None);
        self.ratio_delivery_targets.fill(None);
        self.amplifier_transport.clear_pending();
        self.amplifier_transport_failed = false;
        self.amplifier_fresh.fill(false);
        self.filter_delivery_targets.fill(None);
        self.filter2_delivery_resonance.fill(None);
        self.comb_feedback_delivery_targets.fill(None);
        self.amplifier_soft_binding.fill(false);
        self.amplifier_targets.fill(None);
        self.amplifier_bound.fill(true);
        self.note_pitches.fill(None);
        self.portamento_voices.fill(None);
        self.mono_notes.fill(Default::default());
        self.prepared_note_tag = None;
        self.drum_groups = Default::default();
        for state in &mut self.sustain_states {
            state.flags &= 127;
        }
        self.allocator = VoiceAllocator::default();
        self.link = ProcessorLink::default();
    }
    pub fn remove(&mut self, slot: usize) {
        self.cache_retired_voice(slot);
        self.voices[slot] = None;
        self.controller_flags[slot] = 0;
        self.note_pitches[slot] = None;
        self.portamento_voices[slot] = None;
        self.allocator.finish(slot);
    }
    pub fn stop_timbre(&mut self, timbre: u8) {
        self.clear_mono_notes(timbre);
        if let Some(state) = self.sustain_states.get_mut(timbre as usize) {
            state.flags &= 127;
        }
        for slot in 0..VOICE_COUNT {
            if self.voices[slot]
                .as_ref()
                .is_some_and(|v| v.timbre == timbre)
            {
                self.remove(slot);
            }
        }
    }
    pub fn release_note(&mut self, timbre: u8, note: u8, tables: Option<&ControllerTables>) {
        self.release_note_with_mode(timbre, note, tables, false);
    }
    pub fn release_drum_note(&mut self, timbre: u8, note: u8, tables: Option<&ControllerTables>) {
        self.release_note_with_mode(timbre, note, tables, true);
    }
    fn release_note_with_mode(
        &mut self,
        timbre: u8,
        note: u8,
        tables: Option<&ControllerTables>,
        drum: bool,
    ) {
        if timbre as usize >= TIMBRE_COUNT {
            return;
        }
        let tag = self.consume_note_event();
        let selected = self.note_groups.release_mask(
            &self.allocator.order,
            &self.allocator.claims,
            AllocationOwner(timbre as u32 + 1),
            ((tag as u32) << 24) | note as u32,
        );
        let state = &mut self.sustain_states[timbre as usize];
        let program = self.sustain_programs[timbre as usize];
        let selector = self.release_selectors[timbre as usize];
        if !drum && !self.voice_modes[timbre as usize].polyphonic {
            if state.mono_note_off(program, selector) {
                self.release_mono_owner(timbre, tables);
            }
            return;
        }
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(voice) = voice else { continue };
            if selected & (1 << slot) == 0 {
                continue;
            }
            voice.held = false;
            let claim = &mut self.allocator.claims[slot];
            claim.note_flags &= 127;
            if !state.poly_note_off(program, selector, claim) {
                continue;
            }
            if let (Some(amplifier), Some(tables)) = (&mut voice.amplifier, tables) {
                amplifier.release(tables);
            } else {
                voice.renderer.release();
            }
            if let (Some(auxiliary), Some(tables)) = (&mut voice.auxiliary, tables) {
                auxiliary.release(tables);
            }
        }
    }
    pub fn release_all_notes(&mut self, timbre: u8, tables: Option<&ControllerTables>) {
        if timbre as usize >= TIMBRE_COUNT {
            return;
        }
        self.clear_mono_notes(timbre);
        let program = self.sustain_programs[timbre as usize];
        let selector = self.release_selectors[timbre as usize];
        if !self.voice_modes[timbre as usize].polyphonic {
            if self.sustain_states[timbre as usize].mono_note_off(program, selector) {
                self.release_mono_owner(timbre, tables);
            }
            return;
        }
        for slot in 0..VOICE_COUNT {
            if self.allocator.claims[slot].owner != AllocationOwner(timbre as u32 + 1)
                || self.allocator.claims[slot].note_flags & 128 == 0
            {
                continue;
            }
            let release = self.sustain_states[timbre as usize].poly_note_off(
                program,
                selector,
                &mut self.allocator.claims[slot],
            );
            if let Some(voice) = &mut self.voices[slot] {
                voice.held = false;
            }
            if release {
                self.release_slot(slot, tables);
            }
        }
    }
    pub fn edit_filter(&mut self, timbre: u8, filter: FilterCoefficients) {
        let transported = self.amplifier_rates.is_some();
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if !transported || voice.auxiliary.as_ref().is_none_or(|a| a.filter.is_none()) {
                voice.renderer.set_filter(filter);
            }
        }
    }
    pub fn edit_filter_routing(
        &mut self,
        timbre: u8,
        routing: Option<radias_synth_domain::filter_routing::FilterRouting>,
        second: radias_synth_domain::filter_routing::Filter2Coefficients,
    ) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            let mut delivered = second;
            if self.amplifier_transport.pitch_enabled()
                && let Some(prior) = voice.renderer.filter2_target()
            {
                // Desktop sends descriptor and controller commands together.
                // A numeric knob edit must wait for its original packets.
                if prior.output == second.output && voice.renderer.filter_routing() == routing {
                    continue;
                }
                delivered = radias_synth_domain::filter_routing::Filter2Coefficients {
                    output: second.output,
                    ..prior
                };
            }
            voice.renderer.set_filter_routing(routing, delivered);
        }
    }
    pub fn edit_shaper(&mut self, timbre: u8, shaper: crate::shaper::ShaperProgram) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            let descriptor_changed = voice
                .shaper
                .is_none_or(|old| old.mode != shaper.mode || old.position != shaper.position);
            voice.shaper = Some(shaper);
            if self.amplifier_transport.pitch_enabled() && !descriptor_changed {
                continue;
            }
            let mut current = shaper;
            current.control.modulation = voice
                .modulation
                .as_ref()
                .map_or(0, |m| m.patches.targets.controls[8]);
            let mut target = current.parameters_with_pitch(voice.renderer.primary_pitch_code());
            if self.amplifier_transport.pitch_enabled()
                && let Some(new) = &mut target
            {
                let depth = voice
                    .renderer
                    .shaper_target()
                    .map_or(0, |s| s.coefficients.depth());
                new.coefficients.set_depth(depth);
                if let radias_synth_domain::waveshaper::ShaperCoefficients::SubOscillator(c) =
                    &mut new.coefficients
                {
                    c.target_depth = depth;
                }
            }
            voice.renderer.set_shaper(target);
        }
    }
    pub fn edit_adsr(&mut self, timbre: u8, values: [u8; 4], tables: &ControllerTables) {
        for voice in self
            .voices
            .iter_mut()
            .flatten()
            .filter(|v| v.timbre == timbre)
        {
            if let Some(amp) = &mut voice.amplifier {
                amp.edit_adsr(values, tables);
            }
        }
    }
    pub fn edit_modulation(
        &mut self,
        timbre: u8,
        program: ModulationProgram,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        program.validate_with_clock(self.clock.is_some())?;
        if timbre as usize >= TIMBRE_COUNT {
            return Ok(());
        }
        if let Some(pair) = self.shared_lfo.get_mut(timbre as usize) {
            pair.synthesis.parameters = program.lfo;
        }
        if let Some(clock) = &mut self.clock {
            for i in 0..LFO_COUNT {
                let c = synthesis_clock_slot(i);
                clock.divisions.timbres[timbre as usize][c] = program.tempo_divisions[i];
                clock.bank.timbres[timbre as usize][c].previous_increment = clock
                    .tables
                    .compile_increment(
                        (program.tempo_divisions[i] & 31) as i32,
                        0,
                        clock.receiver.tempo.clock_rate(),
                    )
                    .1;
            }
        }
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(voice) = voice.as_mut().filter(|v| v.timbre == timbre) else {
                continue;
            };
            if let Some(modulation) = &mut voice.modulation {
                modulation.edit_with_clock(program, self.clock.is_some())?;
                if let Some(clock) = &mut self.clock {
                    clock.divisions.voices[slot] = program.tempo_divisions;
                    for i in 0..LFO_COUNT {
                        clock.bank.voices[slot][i].previous_increment = clock
                            .tables
                            .compile_increment(
                                (program.tempo_divisions[i] & 31) as i32,
                                0,
                                clock.receiver.tempo.clock_rate(),
                            )
                            .1;
                    }
                }
            }
        }
        Ok(())
    }
    /// Effect phase controllers also consume the common synthesis random seed.
    /// Audio effect processing is independent of this controller input.
    pub fn edit_effect_lfo(
        &mut self,
        timbre: Option<u8>,
        effect: usize,
        parameters: EffectLfoParameters,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        if self.clock.is_none() && parameters.phase_sync & 128 != 0 {
            return Err(crate::lfo::TempoSynchronizationPending);
        }
        let state = if let Some(timbre) = timbre {
            if timbre as usize >= TIMBRE_COUNT || effect >= 2 {
                return Ok(());
            }
            self.effect_lfo_parameters[timbre as usize][effect] = parameters;
            self.clock.as_mut().map(|clock| {
                clock.divisions.timbres[timbre as usize][effect + 2] = parameters.beat & 31;
                (
                    &mut clock.bank.timbres[timbre as usize][effect + 2],
                    &clock.tables,
                    clock.receiver.tempo,
                )
            })
        } else {
            self.global_lfo_parameters = parameters;
            self.clock.as_mut().map(|clock| {
                clock.divisions.global = parameters.beat & 31;
                (&mut clock.bank.global, &clock.tables, clock.receiver.tempo)
            })
        };
        if let Some((state, tables, tempo)) = state {
            state.previous_increment = tables
                .compile_increment((parameters.beat & 31) as i32, 0, tempo.clock_rate())
                .1;
        }
        Ok(())
    }
    /// Apply original effect configuration/rate publication without resetting
    /// the existing phase, random seed or MIDI clock correction state.
    pub fn apply_effect_lfo_publication(
        &mut self,
        publication: radias_synth_domain::effect_lfo_program::EffectLfoPublication,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        let slot = usize::from(publication.slot.raw());
        if slot < 8 {
            let timbre = slot / 2;
            let effect = slot % 2;
            let parameters =
                self.effect_lfo_parameters[timbre][effect].with_program(publication.program);
            self.edit_effect_lfo(Some(timbre as u8), effect, parameters)?;
            self.shared_lfo[timbre].effects[effect]
                .tempo
                .previous_increment = publication.tempo_increment;
            if let Some(clock) = &mut self.clock {
                clock.bank.timbres[timbre][effect + 2].previous_increment =
                    publication.tempo_increment;
            }
        } else {
            let parameters = self.global_lfo_parameters.with_program(publication.program);
            self.edit_effect_lfo(None, 0, parameters)?;
            self.global_lfo.tempo.previous_increment = publication.tempo_increment;
            if let Some(clock) = &mut self.clock {
                clock.bank.global.previous_increment = publication.tempo_increment;
            }
        }
        Ok(())
    }
    pub fn effect_lfo_parameters(&self, slot: u8) -> Option<EffectLfoParameters> {
        if slot < 8 {
            Some(self.effect_lfo_parameters[usize::from(slot) / 2][usize::from(slot) % 2])
        } else if slot == 8 {
            Some(self.global_lfo_parameters)
        } else {
            None
        }
    }
    pub fn effect_lfo_rate(&self, slot: u8) -> Option<u32> {
        if slot < 8 {
            let t = usize::from(slot) / 2;
            let e = usize::from(slot) % 2;
            Some(if let Some(clock) = &self.clock {
                clock.bank.timbres[t][e + 2].previous_increment
            } else {
                self.shared_lfo[t].effects[e].tempo.previous_increment
            })
        } else if slot == 8 {
            Some(if let Some(clock) = &self.clock {
                clock.bank.global.previous_increment
            } else {
                self.global_lfo.tempo.previous_increment
            })
        } else {
            None
        }
    }
    pub fn shared_lfo_states(
        &self,
        timbre: usize,
    ) -> Option<[radias_synth_domain::lfo::LfoState; 4]> {
        self.shared_lfo.get(timbre).map(|shared| {
            [
                shared.synthesis.states[0],
                shared.synthesis.states[1],
                shared.effects[0].state,
                shared.effects[1].state,
            ]
        })
    }
    pub fn global_lfo_state(&self) -> radias_synth_domain::lfo::LfoState {
        self.global_lfo.state
    }
    pub fn effect_lfo_value_states(
        &self,
    ) -> [radias_synth_domain::effect_lfo_values::EffectLfoValueState; 9] {
        core::array::from_fn(|slot| {
            let (oscillator, alternate_phase) = if slot < 8 {
                (
                    self.shared_lfo[slot / 2].effects[slot % 2].state,
                    self.effect_lfo_parameters[slot / 2][slot % 2].alternate_phase,
                )
            } else {
                (
                    self.global_lfo.state,
                    self.global_lfo_parameters.alternate_phase,
                )
            };
            radias_synth_domain::effect_lfo_values::EffectLfoValueState {
                oscillator,
                alternate_phase,
            }
        })
    }
    /// Original 0172e8 traverses even/odd physical slots, independently of the
    /// Master/Slave DSP split. The caller owns the scheduler parity and timing.
    pub fn service_lfos(&mut self, parity: u8, tables: &VoiceModulationTables) {
        let parity = (parity & 1) as usize;
        if parity == 0 {
            if let Some(clock) = &mut self.clock {
                self.global_lfo.tempo = clock.bank.global;
                self.global_lfo.state.phase = clock.bank.global.phase;
                self.global_lfo.tick(
                    self.global_lfo_parameters,
                    &tables.lfo,
                    &clock.tables,
                    &mut self.modulation_random,
                );
                clock.bank.global = self.global_lfo.tempo;
            } else {
                self.global_lfo.tick_free(
                    self.global_lfo_parameters,
                    &tables.lfo,
                    &mut self.modulation_random,
                );
            }
        }
        for timbre in (0..TIMBRE_COUNT)
            .step_by(4)
            .flat_map(|start| (start + parity * 2)..(start + parity * 2 + 2))
        {
            let shared = &mut self.shared_lfo[timbre];
            if let Some(clock) = &mut self.clock {
                for i in 0..LFO_COUNT {
                    shared.tempo[i] = clock.bank.timbres[timbre][synthesis_clock_slot(i)];
                    shared.synthesis.states[i].phase = shared.tempo[i].phase;
                }
                for i in 0..2 {
                    shared.effects[i].tempo = clock.bank.timbres[timbre][i + 2];
                    shared.effects[i].state.phase = shared.effects[i].tempo.phase;
                }
                shared.tick(
                    self.timbre_modulation_active[timbre],
                    core::array::from_fn(|i| {
                        clock.divisions.timbres[timbre][synthesis_clock_slot(i)]
                    }),
                    self.effect_lfo_parameters[timbre],
                    &tables.lfo,
                    &clock.tables,
                    &mut self.modulation_random,
                );
                for i in 0..LFO_COUNT {
                    clock.bank.timbres[timbre][synthesis_clock_slot(i)] = shared.tempo[i];
                }
                for i in 0..2 {
                    clock.bank.timbres[timbre][i + 2] = shared.effects[i].tempo;
                }
            } else {
                shared.tick_free(
                    self.timbre_modulation_active[timbre],
                    self.effect_lfo_parameters[timbre],
                    &tables.lfo,
                    &mut self.modulation_random,
                );
            }
        }
        for slot in (parity..VOICE_COUNT).step_by(2) {
            let Some(modulation) = self.voices[slot]
                .as_mut()
                .and_then(|v| v.modulation.as_mut())
            else {
                continue;
            };
            if let Some(clock) = &mut self.clock {
                modulation.pair.tick_with_tempo(
                    &tables.lfo,
                    &clock.tables,
                    modulation.tempo_divisions,
                    &mut clock.bank.voices[slot],
                    &mut self.modulation_random,
                );
            } else {
                let _ = modulation.lfo_tick(&tables.lfo, &mut self.modulation_random);
            }
            self.controller_slots[slot] = modulation.pair.states;
            self.controller_slot_valid[slot] = true;
        }
    }
    pub fn next_sample<'a>(
        &mut self,
        table: &WaveformTable,
        tables: Option<&ControllerTables>,
        mut events: impl FnMut(usize) -> &'a [VoiceControlEvent],
    ) -> StereoFrame {
        let buses = self.next_buses(table, tables, &mut events);
        // Master already contains the serial Slave contribution.
        // Dry monitoring folds its four stereo pairs before the absent FXD03.
        let (left, right) = buses[0].iter().fold((0i64, 0i64), |(l, r), b| {
            (l + b.left.0 as i64, r + b.right.0 as i64)
        });
        StereoFrame {
            left: Sample(saturate(left)),
            right: Sample(saturate(right)),
        }
    }
    pub fn next_buses<'a>(
        &mut self,
        table: &WaveformTable,
        tables: Option<&ControllerTables>,
        mut events: impl FnMut(usize) -> &'a [VoiceControlEvent],
    ) -> [[StereoFrame; TIMBRE_COUNT]; 2] {
        self.next_buses_with_modulation(table, tables, None, &mut events)
    }
    pub fn next_sample_with_modulation<'a>(
        &mut self,
        table: &WaveformTable,
        tables: Option<&ControllerTables>,
        modulation: Option<&VoiceModulationTables>,
        events: impl FnMut(usize) -> &'a [VoiceControlEvent],
    ) -> StereoFrame {
        let buses = self.next_buses_with_modulation(table, tables, modulation, events);
        let (left, right) = buses[0].iter().fold((0i64, 0i64), |(l, r), b| {
            (l + b.left.0 as i64, r + b.right.0 as i64)
        });
        StereoFrame {
            left: Sample(saturate(left)),
            right: Sample(saturate(right)),
        }
    }
    pub fn next_buses_with_modulation<'a>(
        &mut self,
        table: &WaveformTable,
        tables: Option<&ControllerTables>,
        modulation_tables: Option<&VoiceModulationTables>,
        mut events: impl FnMut(usize) -> &'a [VoiceControlEvent],
    ) -> [[StereoFrame; TIMBRE_COUNT]; 2] {
        let mut buses = [[StereoFrame::default(); TIMBRE_COUNT]; 2];
        let delivery_clock = self.modulation_frame * 3000;
        self.deliver_amplifier(delivery_clock);
        let mut amplifier_updates = [None; VOICE_COUNT];
        let shared_controller = self.controller_timer.is_some();
        let controller_tick = self
            .controller_timer
            .as_mut()
            .is_some_and(|timer| timer.advance_cpu_clocks(3000) != 0);
        if controller_tick {
            self.controller_interrupts = self.controller_interrupts.wrapping_add(1);
        }
        if let Some(clock) = &mut self.clock {
            clock.next_audio_frame();
        }
        // SH controller services traverse the 24 voice slots independently of
        // the later Slave/Master DSP accumulation order.
        if let Some(mod_tables) = modulation_tables
            && (if shared_controller {
                controller_tick && self.controller_interrupts.is_multiple_of(2)
            } else {
                self.modulation_frame != 0 && self.modulation_frame.is_multiple_of(48)
            })
        {
            let parity = (if shared_controller {
                self.controller_interrupts / 2 - 1
            } else {
                self.modulation_frame / 48 - 1
            } & 1) as u8;
            if parity == 0
                && let Some(clock) = &mut self.clock
            {
                let _ = clock.controller_service();
            }
            self.service_lfos(parity, mod_tables);
        }
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(active) = voice else { continue };
            let initial = active.renderer.rendered_frames() == 0;
            let envelope_service = if shared_controller {
                controller_tick
                    && radias_synth_domain::controller_service::select_voice_service(
                        &mut self.controller_flags[slot],
                        0,
                    ) == radias_synth_domain::controller_service::VoiceService::Envelopes
            } else {
                false
            };
            let service = (initial && !self.initial_controller_serviced[slot])
                || if shared_controller {
                    envelope_service
                } else {
                    self.modulation_frame.is_multiple_of(24)
                };
            if service {
                self.initial_controller_serviced[slot] = true;
                if !initial
                    && let (Some(port), Some(tables)) =
                        (&mut self.portamento_voices[slot], &self.portamento_tables)
                {
                    port.tick(&tables.curves);
                }
                let note = self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped);
                self.assigned_pitch_slots[slot] = ((note as i32) << 16)
                    .wrapping_add(self.portamento_voices[slot].map_or(0, |p| p.state.current_q16));
                self.shared_assigned_pitch[active.timbre as usize] =
                    self.assigned_pitch_slots[slot];
            }
            if let (Some(amp), Some(tables)) = (&mut active.amplifier, tables) {
                if service {
                    let relative_pitch = if self.portamento_voices[slot].is_some() {
                        (self.assigned_pitch_slots[slot].wrapping_sub(60 << 16) >> 8) as i16
                    } else {
                        (self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped) as i16
                            - 60)
                            * 256
                    };
                    amp.relative_pitch(relative_pitch, tables);
                }
                let target = if shared_controller {
                    if envelope_service {
                        amp.service(tables, true);
                    }
                    amp.held_target(tables)
                } else {
                    amp.next_target(tables)
                };
                if let Some(rates) = &self.amplifier_rates {
                    if self.amplifier_fresh[slot] {
                        amplifier_updates[slot] = Some(self.amplifier_deliveries[slot].binding(
                            rates,
                            amp.parameters.attack,
                            0,
                            target,
                            self.amplifier_soft_binding[slot],
                        ));
                        self.amplifier_fresh[slot] = false;
                    } else if envelope_service {
                        if amp.envelope.stage
                            == radias_synth_domain::amp_envelope::EnvelopeStage::ReleaseHold
                        {
                            self.amplifier_deliveries[slot].release_zero();
                        }
                        amplifier_updates[slot] =
                            Some(self.amplifier_deliveries[slot].service(rates, target));
                    }
                } else {
                    active.renderer.set_envelope_target(target);
                }
            }
            if let (Some(auxiliary), Some(tables)) = (&mut active.auxiliary, tables) {
                if shared_controller {
                    if envelope_service {
                        auxiliary.service(tables);
                    }
                } else {
                    auxiliary.next(tables);
                }
            }
            if let (Some(modulation), Some(mod_tables), Some(tables)) =
                (&mut active.modulation, modulation_tables, tables)
                && service
            {
                let midi = self.midi_pitch[active.timbre as usize];
                let relative_note = self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped);
                if let (Some(pitch), Some(program), Some(pitch_tables)) = (
                    self.note_pitches[slot],
                    active
                        .drum_pitch
                        .or(self.pitch_programs[active.timbre as usize]),
                    &self.note_pitch_tables,
                ) {
                    modulation.base_pitch_q16 = if active.drum_pitch.is_some() {
                        pitch.drum_base(program, pitch_tables, self.master_tune)
                    } else {
                        pitch.base(program, midi, pitch_tables, self.master_tune)
                    }
                    .wrapping_add(self.voice_group_offsets[slot].tuning_q16)
                    .wrapping_add(self.portamento_voices[slot].map_or(0, |p| p.state.current_q16));
                    modulation.vibrato_depth =
                        program.vibrato_depth(midi.wheel, &pitch_tables.vibrato);
                }
                let (level, sensitivity) = active.amplifier.as_ref().map_or((0, 64), |amp| {
                    (amp.envelope.segment.level, amp.level_sensitivity())
                });
                let signals = radias_synth_domain::modulation::ControllerSources {
                    envelope_levels: active
                        .auxiliary
                        .as_ref()
                        .map_or([0, level, 0], |a| [a.levels()[0], level, a.levels()[1]]),
                    envelope_velocity_sensitivity: active.auxiliary.as_ref().map_or(
                        [64, sensitivity, 64],
                        |a| {
                            [
                                a.programs[0].velocity_level_sensitivity,
                                sensitivity,
                                a.programs[1].velocity_level_sensitivity,
                            ]
                        },
                    ),
                    lfo: [0; 2],
                    velocity: active.velocity,
                    bend: midi.bend,
                    wheel: midi.wheel,
                    relative_pitch: if self.portamento_voices[slot].is_some() {
                        (self.assigned_pitch_slots[slot].wrapping_sub(60 << 16) >> 8) as i16
                    } else {
                        (relative_note as i16 - 60) * 256
                    },
                    auxiliary: 0,
                }
                .normalized(&tables.amplifier);
                let mut sources = [0; SOURCE_COUNT];
                sources[..10].copy_from_slice(&signals);
                let targets = modulation.service(&mod_tables.lfo, &mod_tables.matrix, sources);
                if let (Some(port), Some(port_tables)) =
                    (&mut self.portamento_voices[slot], &self.portamento_tables)
                {
                    port.modulate(
                        targets.controls[13],
                        &port_tables.rates,
                        self.portamento_switches[active.timbre as usize],
                    );
                }
                let code = radias_synth_domain::pitch::PitchCode::new(
                    modulation.pitch_code(&mod_tables.lfo),
                )
                .unwrap();
                let increment = mod_tables.pitch.increment(code);
                self.controller_pitch_codes[slot] = code.raw();
                if self.amplifier_transport.pitch_enabled() {
                    let selection = active.primary.map_or(0, |p| p.selection);
                    let sync = active.secondary.is_some_and(|p| p.modulation().sync);
                    if self.secondary_sync_targets[slot] != Some(sync) {
                        if self
                            .amplifier_transport
                            .secondary_sync(delivery_clock, slot, sync)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        } else {
                            self.secondary_sync_targets[slot] = Some(sync);
                        }
                    }
                    if let Some(primary) = active.primary.filter(|p| p.selection & 48 == 32) {
                        let lfo = modulation.pair.values(&mod_tables.lfo)[0];
                        let control = primary
                            .waveform_control(lfo, [targets.controls[0], targets.controls[14]])
                            .unwrap_or(0);
                        if self
                            .amplifier_transport
                            .unison_detune(delivery_clock, slot, control)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                    }
                    let offset = active.secondary.zip(self.secondary_table.as_ref()).map_or(
                        0,
                        |(secondary, table)| {
                            let mut pitch = secondary.pitch;
                            pitch.virtual_patch_q16 = targets.oscillator_pitch_q16[1];
                            pitch.relative_code(table)
                        },
                    );
                    if self
                        .amplifier_transport
                        .secondary_pitch(delivery_clock, slot, offset)
                        .is_err()
                        || self
                            .amplifier_transport
                            .primary_pitch(delivery_clock, slot, selection, code.raw())
                            .is_err()
                    {
                        self.amplifier_transport_failed = true;
                    }
                } else {
                    active.renderer.set_primary_pitch_code(code);
                    active
                        .renderer
                        .set_pitch(increment, mod_tables.bandwidth.coefficient(increment));
                    if let (Some(secondary), Some(table)) =
                        (active.secondary, &self.secondary_table)
                    {
                        let code = secondary.code(code, table, targets.oscillator_pitch_q16[1]);
                        active.renderer.set_secondary_pitch_mode(
                            code,
                            &mod_tables.pitch,
                            &mod_tables.bandwidth,
                            secondary.modulation().sync,
                        );
                    } else if active.mixer.is_some() {
                        active.renderer.set_secondary_pitch(
                            code,
                            &mod_tables.pitch,
                            &mod_tables.bandwidth,
                        );
                    }
                }
                if let Some(amp) = &mut active.amplifier {
                    let target = amp.modulation(targets.controls[9], tables);
                    if self.amplifier_rates.is_none() {
                        active.renderer.set_envelope_target(target);
                    }
                }
            }
            if service
                && let (Some(auxiliary), Some(filter_tables), Some(tables)) =
                    (&active.auxiliary, &self.filter_tables, tables)
            {
                let modulation = active.modulation.as_ref().map_or([0; 3], |m| {
                    [
                        m.patches.targets.controls[5],
                        m.patches.targets.controls[15],
                        m.patches.targets.controls[16],
                    ]
                });
                let relative_pitch = if self.portamento_voices[slot].is_some() {
                    (self.assigned_pitch_slots[slot].wrapping_sub(60 << 16) >> 8) as i16
                } else {
                    (self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped) as i16 - 60)
                        * 256
                };
                if self.amplifier_rates.is_some() {
                    if let Some((frequency, filter)) = auxiliary.filter_inputs_with_pitch(
                        filter_tables,
                        tables,
                        modulation,
                        relative_pitch,
                    ) {
                        self.amplifier_transport.configure_filter(
                            slot,
                            filter.normalization,
                            filter.base,
                        );
                        if self.filter_delivery_targets[slot]
                            .is_none_or(|(_, resonance)| resonance != filter.resonance)
                            && self
                                .amplifier_transport
                                .filter_resonance(delivery_clock, slot, filter.resonance)
                                .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                        if self
                            .amplifier_transport
                            .filter_frequency(delivery_clock, slot, frequency)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                        self.filter_delivery_targets[slot] = Some((frequency, filter.resonance));
                    }
                } else if let Some(target) = auxiliary.filter_target_with_pitch(
                    filter_tables,
                    tables,
                    modulation,
                    relative_pitch,
                ) {
                    if initial {
                        active.renderer.set_filter_immediate(target);
                    } else {
                        active.renderer.set_filter(target);
                    }
                }
            }
            if service && let (Some((tables, _)), Some(mut pan)) = (&self.pan_tables, active.pan) {
                pan.timbre_offset = pan
                    .timbre_offset
                    .wrapping_add(self.voice_group_offsets[slot].pan);
                pan.modulation = active
                    .modulation
                    .as_ref()
                    .map_or(0, |m| m.patches.targets.controls[10]);
                let target = tables.compile(pan.target()) as i16;
                if self.amplifier_transport.pitch_enabled() {
                    if self.pan_delivery_targets[slot] != Some(target) {
                        if self
                            .amplifier_transport
                            .pan(delivery_clock, slot, target)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        } else {
                            self.pan_delivery_targets[slot] = Some(target);
                        }
                    }
                } else {
                    active.renderer.set_pan_target(target);
                }
            }
            if service
                && let (Some(program), Some(comb_tables), Some(filter_tables), Some(tables)) = (
                    active.comb_program,
                    &self.comb_tables,
                    &self.filter_tables,
                    tables,
                )
            {
                let modulation = active.modulation.as_ref().map_or([0; 4], |m| {
                    [
                        m.patches.targets.controls[7],
                        m.patches.targets.controls[17],
                        m.patches.targets.controls[18],
                        m.patches.targets.controls[19],
                    ]
                });
                let input = crate::comb::CombVoiceControl {
                    eg1_level: active.auxiliary.as_ref().map_or(0, |a| a.levels()[0]),
                    eg1_velocity_sensitivity: active
                        .auxiliary
                        .as_ref()
                        .map_or(64, |a| a.programs[0].velocity_level_sensitivity),
                    velocity: active.velocity,
                    relative_pitch: if self.portamento_voices[slot].is_some() {
                        (self.assigned_pitch_slots[slot].wrapping_sub(60 << 16) >> 8) as i16
                    } else {
                        (self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped) as i16
                            - 60)
                            * 256
                    },
                    modulation,
                };
                let coefficients = program
                    .for_voice(input, filter_tables)
                    .coefficients(comb_tables, &tables.amplifier);
                if self.amplifier_transport.pitch_enabled() {
                    let mut base = active.renderer.current_filter2().unwrap_or(coefficients);
                    base.output = radias_synth_domain::filter_routing::Filter2Output::Comb;
                    self.amplifier_transport.configure_filter2(slot, 0, base);
                    if self
                        .amplifier_transport
                        .comb_delay(delivery_clock, slot, coefficients.integrator_gain as u32)
                        .is_err()
                    {
                        self.amplifier_transport_failed = true;
                    }
                    if self.comb_feedback_delivery_targets[slot]
                        != Some(coefficients.feedback as u32)
                    {
                        if self
                            .amplifier_transport
                            .comb_feedback(delivery_clock, slot, coefficients.feedback as u32)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        } else {
                            self.comb_feedback_delivery_targets[slot] =
                                Some(coefficients.feedback as u32);
                        }
                    }
                } else if initial {
                    active.renderer.set_filter2_immediate(coefficients);
                } else {
                    active.renderer.set_filter2_target(coefficients);
                }
            }
            if service
                && let (Some(program), Some(filter2_tables), Some(filter_tables), Some(tables)) = (
                    if active.drum_pitch.is_some() {
                        active.drum_filter2
                    } else {
                        self.filter2_programs[active.timbre as usize]
                    },
                    &self.filter2_tables,
                    &self.filter_tables,
                    tables,
                )
            {
                let modulation = active.modulation.as_ref().map_or([0; 4], |m| {
                    [
                        m.patches.targets.controls[7],
                        m.patches.targets.controls[17],
                        m.patches.targets.controls[18],
                        m.patches.targets.controls[19],
                    ]
                });
                let input = crate::comb::CombVoiceControl {
                    eg1_level: active.auxiliary.as_ref().map_or(0, |a| a.levels()[0]),
                    eg1_velocity_sensitivity: active
                        .auxiliary
                        .as_ref()
                        .map_or(64, |a| a.programs[0].velocity_level_sensitivity),
                    velocity: active.velocity,
                    relative_pitch: if self.portamento_voices[slot].is_some() {
                        (self.assigned_pitch_slots[slot].wrapping_sub(60 << 16) >> 8) as i16
                    } else {
                        (self.note_pitches[slot].map_or(active.note, |p| p.note.wrapped) as i16
                            - 60)
                            * 256
                    },
                    modulation,
                };
                if self.amplifier_transport.pitch_enabled() {
                    if let Some(target) =
                        program.targets(input, filter_tables, filter2_tables, &tables.amplifier)
                    {
                        let mut base = active.renderer.current_filter2().unwrap_or(
                            radias_synth_domain::filter_routing::Filter2Coefficients {
                                input_gain: 0,
                                feedback: 0,
                                integrator_gain: 0,
                                output: target.output,
                            },
                        );
                        base.output = target.output;
                        self.amplifier_transport.configure_filter2(
                            slot,
                            program.normalization,
                            base,
                        );
                        if self
                            .amplifier_transport
                            .filter2_frequency(delivery_clock, slot, target.frequency)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                        if self.filter2_delivery_resonance[slot]
                            != Some((target.resonance, target.input_gain))
                        {
                            if self
                                .amplifier_transport
                                .filter2_resonance(delivery_clock, slot, target.resonance)
                                .is_err()
                                || self
                                    .amplifier_transport
                                    .filter2_input_gain(delivery_clock, slot, target.input_gain)
                                    .is_err()
                            {
                                self.amplifier_transport_failed = true;
                            } else {
                                self.filter2_delivery_resonance[slot] =
                                    Some((target.resonance, target.input_gain));
                            }
                        }
                    }
                } else if let Some(coefficients) =
                    program.coefficients(input, filter_tables, filter2_tables, &tables.amplifier)
                {
                    if initial {
                        active.renderer.set_filter2_immediate(coefficients);
                    } else {
                        active.renderer.set_filter2_target(coefficients);
                    }
                }
            }
            if service && let (Some(scales), Some(mixer)) = (&self.mixer_scales, active.mixer) {
                let values = active.modulation.as_ref().map_or([0; 3], |m| {
                    [
                        m.patches.targets.controls[1],
                        m.patches.targets.controls[2],
                        m.patches.targets.controls[3],
                    ]
                });
                let target = mixer.compile(scales, values);
                if self.amplifier_transport.pitch_enabled() {
                    let values = [
                        target.primary_gain,
                        target.secondary_gain,
                        target.noise_gain,
                    ];
                    let mut prior = self.mixer_delivery_targets[slot];
                    for (band, value) in values.into_iter().enumerate() {
                        if prior.is_none_or(|p| p[band] != value) {
                            if self
                                .amplifier_transport
                                .mixer_level(delivery_clock, slot, band as u8, value)
                                .is_err()
                            {
                                self.amplifier_transport_failed = true;
                            } else {
                                let p = prior.get_or_insert([i16::MIN; 3]);
                                p[band] = value;
                            }
                        }
                    }
                    self.mixer_delivery_targets[slot] = prior;
                } else {
                    active.renderer.set_mixer(target);
                }
            }
            if service && let Some(mut shaper) = active.shaper {
                shaper.control.modulation = active
                    .modulation
                    .as_ref()
                    .map_or(0, |m| m.patches.targets.controls[8]);
                let target = shaper.parameters_with_pitch(self.controller_pitch_codes[slot]);
                if self.amplifier_transport.pitch_enabled() {
                    if let Some(target) = target
                        && self
                            .amplifier_transport
                            .shaper_depth(delivery_clock, slot, target.coefficients.depth())
                            .is_err()
                    {
                        self.amplifier_transport_failed = true;
                    }
                } else {
                    active.renderer.set_shaper(target);
                }
            }
            if service && let Some(primary) = active.primary {
                let (lfo1, modulation) = match (&active.modulation, modulation_tables) {
                    (Some(m), Some(t)) => (
                        m.pair.values(&t.lfo)[0],
                        [
                            m.patches.targets.controls[0],
                            m.patches.targets.controls[14],
                        ],
                    ),
                    _ => (0, [0; 2]),
                };
                if let Some(tables) = &self.noise_tables
                    && let Some(code) = radias_synth_domain::pitch::PitchCode::new(
                        self.controller_pitch_codes[slot],
                    )
                    && let Some(control) = active.renderer.noise_control_mut()
                {
                    if self.amplifier_transport.pitch_enabled() {
                        let target = tables.control_targets(
                            control,
                            primary.control,
                            code,
                            lfo1,
                            modulation,
                        );
                        if let Some(shape) = target.shape
                            && self
                                .amplifier_transport
                                .noise_shape(delivery_clock, slot, shape)
                                .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                        if self
                            .amplifier_transport
                            .noise_gain(delivery_clock, slot, target.excitation_gain)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                        if self
                            .amplifier_transport
                            .noise_frequency(delivery_clock, slot, target.excitation_bias)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                    } else {
                        tables.update(control, primary.control, code, lfo1, modulation);
                        if initial {
                            control.initialize_targets();
                        }
                    }
                }
                if let Some(control) = primary.waveform_control(lfo1, modulation)
                    && (!self.amplifier_transport.pitch_enabled() || primary.selection & 48 != 32)
                {
                    if self.amplifier_transport.pitch_enabled() {
                        if self
                            .amplifier_transport
                            .primary_control(delivery_clock, slot, primary.selection, control)
                            .is_err()
                        {
                            self.amplifier_transport_failed = true;
                        }
                    } else {
                        active.renderer.set_primary_waveform_control(control);
                    }
                }
                if primary.selection & 48 == 48 {
                    let ratio = radias_synth_domain::controller_primary::PrimaryControl {
                        control2_modulation: modulation[1],
                        ..primary.control
                    }
                    .vpm_ratio();
                    if self.amplifier_transport.pitch_enabled() {
                        if self.ratio_delivery_targets[slot] != Some(ratio) {
                            if self
                                .amplifier_transport
                                .primary_ratio(delivery_clock, slot, ratio)
                                .is_err()
                            {
                                self.amplifier_transport_failed = true;
                            } else {
                                self.ratio_delivery_targets[slot] = Some(ratio);
                            }
                        }
                    } else {
                        active.renderer.set_primary_ratio(ratio);
                    }
                }
            }
        }
        for (slot, update) in amplifier_updates.iter().copied().enumerate() {
            if self.amplifier_rates.is_none() {
                break;
            }
            let Some(amp) = self.voices[slot]
                .as_ref()
                .and_then(|v| v.amplifier.as_ref())
            else {
                continue;
            };
            // The signed termination mode forces silence in SYS002a6c even
            // if a still-running Virtual Patch compiles a nonzero AMP target.
            let target = if (self.amplifier_deliveries[slot].mode as i8) < 0 {
                0
            } else {
                amp.target()
            };
            let packet = match update {
                Some(radias_synth_domain::amplifier_delivery::AmplifierPacket::RateAndTarget {
                    rate,
                    ..
                }) => Some(
                    radias_synth_domain::amplifier_delivery::AmplifierPacket::RateAndTarget {
                        rate,
                        target,
                    },
                ),
                Some(radias_synth_domain::amplifier_delivery::AmplifierPacket::Target(_)) => {
                    Some(radias_synth_domain::amplifier_delivery::AmplifierPacket::Target(target))
                }
                None if self.amplifier_targets[slot] != Some(target) => {
                    Some(radias_synth_domain::amplifier_delivery::AmplifierPacket::Target(target))
                }
                _ => None,
            };
            if let Some(packet) = packet {
                if self
                    .amplifier_transport
                    .enqueue(delivery_clock, slot, packet)
                    .is_err()
                {
                    self.amplifier_transport_failed = true;
                } else {
                    self.amplifier_targets[slot] = Some(target);
                }
            }
        }
        // Slave produces its frame while Master consumes a previous DMA group.
        for processor in [1, 0] {
            if processor == 0 {
                buses[0] = self.link.advance(buses[1]);
            }
            for (local, voice) in self.voices
                [processor * VOICES_PER_PROCESSOR..(processor + 1) * VOICES_PER_PROCESSOR]
                .iter_mut()
                .enumerate()
            {
                let slot = processor * VOICES_PER_PROCESSOR + local;
                let Some(active) = voice else {
                    if let Some(frame) = &mut self.physical_frames[slot] {
                        let bus = &mut buses[slot / VOICES_PER_PROCESSOR][frame.stereo_bus.index()];
                        if self.drum_slots[slot] && self.drum_gain != 1.0 {
                            let sample = frame.stereo_cache.advance(Default::default());
                            bus.left = Sample(radias_synth_domain::fixed::saturate(
                                bus.left.0 as i64 + (sample.left.0 as f64 * self.drum_gain) as i64,
                            ));
                            bus.right = Sample(radias_synth_domain::fixed::saturate(
                                bus.right.0 as i64
                                    + (sample.right.0 as f64 * self.drum_gain) as i64,
                            ));
                        } else {
                            *bus = frame.stereo_cache.advance(*bus);
                        }
                    }
                    continue;
                };
                if !self.amplifier_bound[slot] {
                    if let Some(frame) = &mut self.physical_frames[slot] {
                        let bus = &mut buses[slot / VOICES_PER_PROCESSOR][frame.stereo_bus.index()];
                        *bus = frame.stereo_cache.advance(*bus);
                    }
                    continue;
                }
                let bus = &mut buses[slot / VOICES_PER_PROCESSOR][active.bus.index()];
                #[cfg(feature = "web-modular")]
                if let Some(circuit) = &mut active.renderer.circuit {
                    let eg = active.auxiliary.as_ref().map_or([0; 2], |p| p.levels());
                    let lfo = active
                        .modulation
                        .as_ref()
                        .zip(modulation_tables)
                        .map_or([0; LFO_COUNT], |(m, t)| m.pair.values(&t.lfo));
                    circuit.controls([
                        if active.held { 1.0 } else { 0.0 },
                        active.velocity as f64 / 127.0,
                        active.note as f64,
                        eg[0] as f64 / 65535.0,
                        eg[1] as f64 / 65535.0,
                        lfo[0] as f64 / 32768.0,
                        lfo[1] as f64 / 32768.0,
                        lfo.get(2).copied().unwrap_or(0) as f64 / 32768.0,
                    ]);
                }
                self.drum_slots[slot] = active.drum_instrument.is_some();
                if self.drum_slots[slot] && self.drum_gain != 1.0 {
                    let sample = active.renderer.next_on_bus(
                        table,
                        events(active.program),
                        Default::default(),
                    );
                    bus.left = Sample(radias_synth_domain::fixed::saturate(
                        bus.left.0 as i64 + (sample.left.0 as f64 * self.drum_gain) as i64,
                    ));
                    bus.right = Sample(radias_synth_domain::fixed::saturate(
                        bus.right.0 as i64 + (sample.right.0 as f64 * self.drum_gain) as i64,
                    ));
                } else {
                    *bus = active
                        .renderer
                        .next_on_bus(table, events(active.program), *bus);
                }
                if self.physical_frames[slot].is_some() {
                    let cache = self.physical_frames[slot].unwrap().stereo_cache;
                    let mut frame = radias_synth_domain::voice_frame::VoiceFrameState::capture(
                        &active.renderer.voice,
                    );
                    frame.stereo_cache = cache;
                    frame.last_amplified = active.renderer.last_amplified();
                    frame.last_pan_current = active.renderer.last_pan_current();
                    frame.stereo_bus = active.bus;
                    self.physical_frames[slot] = Some(frame);
                }
                let finished = active
                    .amplifier
                    .as_ref()
                    .is_some_and(AmplifierController::finished)
                    && active.renderer.voice.envelope.0 == 0;
                let fallback_finished = active.amplifier.is_none()
                    && !active.held
                    && active.renderer.voice.envelope.0 == 0;
                #[cfg(not(feature = "web-modular"))]
                let tail_active = false;
                #[cfg(feature = "web-modular")]
                let tail_active = active
                    .renderer
                    .circuit
                    .as_ref()
                    .is_some_and(|p| p.tail_active());
                if (finished || fallback_finished) && !tail_active {
                    if self.noise_tables.is_some()
                        && let Some(frame) = &mut self.physical_frames[slot]
                    {
                        frame.stereo_cache.initialize(
                            frame.last_amplified,
                            frame.last_pan_current,
                            active.renderer.envelope_rate(),
                        );
                        frame.last_amplified = Sample(0);
                    }
                    *voice = None;
                    self.allocator.finish(slot);
                }
            }
            buses[processor] = if processor == 0 {
                buses[processor].map(scale_bus)
            } else {
                buses[processor].map(scale_slave)
            };
        }
        self.modulation_frame = self.modulation_frame.wrapping_add(1);
        buses
    }

    pub fn reset_amplifier(&mut self, slot: usize, value: i16) {
        if let Some(voice) = self.voices[slot].as_mut() {
            voice.renderer.voice.envelope.0 = value;
        }
    }
    /// Publish a controller result on its delivery clock. The adapter owns
    /// scheduling; the aggregate retains the actual DSP envelope smoother.
    pub fn publish_amplifier(&mut self, slot: usize, target: i16) {
        if let Some(voice) = self.voices.get_mut(slot).and_then(Option::as_mut) {
            voice.renderer.set_envelope_target(target);
        }
    }
    pub fn reset_filter(&mut self, slot: usize) {
        if let Some(voice) = self.voices[slot].as_mut() {
            voice.renderer.reset_filter_memory();
        }
    }
}
