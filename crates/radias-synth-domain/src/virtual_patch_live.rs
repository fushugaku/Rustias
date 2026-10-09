//! Live destination stores and changed-value compiler dispatch, SYS0219a4..222fc.
//! Compiler arithmetic and host publication are separate owned transitions.
use crate::actor_control_state::ActorControlState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveModulationCompiler {
    SecondaryPitch,
    PrimaryMixer,
    SecondaryMixer,
    NoiseMixer,
    FilterMix,
    Filter1Resonance,
    ShaperDepth,
    Pan,
    Portamento,
    Filter1EnvelopeIntensity,
    Filter1KeyTracking,
    Filter2Resonance,
    Filter2EnvelopeIntensity,
    Filter2KeyTracking,
    EnvelopeParameter { envelope: u8, parameter: u8 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveDestinationUpdate {
    pub changed: bool,
    pub compiler: Option<LiveModulationCompiler>,
}
pub struct LiveFilterResonanceTables {
    pub gain: [i32; 128],
    pub normalization: [[u16; 128]; 2],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveCompilerPorts {
    pub portamento_time: u8,
    pub portamento_switch_required: bool,
    pub portamento_switch: bool,
    pub midi_pan: Option<u8>,
}
pub struct LiveCompilerTables<'a> {
    pub fine: &'a crate::controller_secondary::FineTuneTable,
    pub pan: &'a crate::controller_pan::PanTables,
    pub timing: &'a crate::envelope_segment::EnvelopeTimingTables,
    pub resonance: &'a LiveFilterResonanceTables,
    pub amplifier: &'a crate::amplifier_control::AmplifierTables,
    pub frequency: &'a crate::controller_filter::ControllerFilterTables,
    pub comb: &'a crate::controller_comb::CombControlTables,
    pub portamento: &'a crate::portamento::PortamentoRates,
}
#[derive(Clone, Copy)]
pub struct LiveDestinationRequest<'a> {
    pub destination: u8,
    pub amount: i32,
    pub linked_pitch: i32,
    pub body: &'a [u8; 104],
    pub ports: LiveCompilerPorts,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCompilationError {
    InvalidDestination,
    UnsupportedPrimary,
    UnsupportedShaper,
    VirtualPatch(crate::actor_virtual_patch::ActorVirtualPatchError),
}
pub struct CompiledLiveDestination {
    pub controller: ActorControlState,
    pub update: LiveDestinationUpdate,
    pub publication: crate::actor_descriptors::DescriptorPlan,
}
#[derive(Clone, Copy)]
pub struct LiveVirtualPatchRequest<'a> {
    pub body: &'a [u8; 104],
    pub sources: crate::actor_virtual_patch::ActorVirtualPatchPorts,
    pub compilers: LiveCompilerPorts,
}
pub struct CompiledLiveVirtualPatches {
    pub controller: ActorControlState,
    pub targets: crate::modulation::ModulationTargets,
    pub publication: crate::actor_descriptors::DescriptorPlan,
}
impl ActorControlState {
    /// Whole SYS021832(r0=0): six routes from prior feedback, followed by
    /// all forty native changed-value callbacks in firmware order.
    pub fn compile_live_virtual_patches(
        &self,
        request: LiveVirtualPatchRequest<'_>,
        tables: &LiveCompilerTables<'_>,
        modulation: &crate::modulation::ModulationTables,
    ) -> Result<CompiledLiveVirtualPatches, LiveCompilationError> {
        let (targets, work) = self
            .calculate_virtual_patches(request.body, request.sources, tables.amplifier, modulation)
            .map_err(LiveCompilationError::VirtualPatch)?;
        let mut publication = crate::actor_descriptors::DescriptorPlan::default();
        //626 wrapper clocks =261 before the first target +39*9 between
        // callbacks +14 for the last loop test and register restoration.
        publication.work(261 + work.routes.iter().sum::<u16>());
        let mut controller = *self;
        for destination in 0..40 {
            let compiled = controller.compile_live_destination(
                LiveDestinationRequest {
                    destination,
                    amount: targets.values[usize::from(destination)],
                    linked_pitch: targets.linked_pitch,
                    body: request.body,
                    ports: request.compilers,
                },
                tables,
            )?;
            controller = compiled.controller;
            publication.append_compacted(&compiled.publication);
            let mut continuation = crate::actor_descriptors::DescriptorPlan::default();
            continuation.work(if destination == 39 { 14 } else { 9 });
            publication.append_compacted(&continuation);
        }
        Ok(CompiledLiveVirtualPatches {
            controller,
            targets,
            publication,
        })
    }
    /// Complete live destination calculation and functional caller publication
    /// plan. Source-derived work and sender timing remain separate from the
    /// adapter's interrupt, receiver and audio-job schedule.
    pub fn compile_live_destination(
        &self,
        request: LiveDestinationRequest<'_>,
        tables: &LiveCompilerTables<'_>,
    ) -> Result<CompiledLiveDestination, LiveCompilationError> {
        use LiveModulationCompiler::*;
        let mut controller = *self;
        let update = controller
            .store_live_destination(request.destination, request.amount, request.linked_pitch)
            .ok_or(LiveCompilationError::InvalidDestination)?;
        let body = request.body;
        let mut sends = [(0u8, 0u16, 0u32); 2];
        let mut count = 0;
        if let Some(compiler) = update.compiler {
            match compiler {
                SecondaryPitch => {
                    sends[0] = (
                        8,
                        0x25,
                        controller.compile_live_secondary_pitch(body, tables.fine) as u16 as u32,
                    );
                    count = 1;
                }
                PrimaryMixer | SecondaryMixer | NoiseMixer => {
                    let index = request.destination - 3;
                    let value = controller
                        .compile_live_mixer(body, index)
                        .ok_or(LiveCompilationError::UnsupportedPrimary)?;
                    sends[0] = (0, 0x2f + 2 * u16::from(index), value as u16 as u32);
                    count = 1;
                }
                FilterMix => {
                    sends[0] = (
                        13,
                        0x35,
                        u32::from(controller.compile_live_filter_mix(body)),
                    );
                    count = 1;
                }
                Filter1Resonance => {
                    let (gain, norm) =
                        controller.compile_live_filter1_resonance(body, tables.resonance);
                    sends = [(18, 0x3e, gain as u32), (0, 0x36, u32::from(norm))];
                    count = 2;
                }
                ShaperDepth => {
                    if let Some(value) = controller
                        .compile_live_shaper_depth(body)
                        .ok_or(LiveCompilationError::UnsupportedShaper)?
                    {
                        sends[0] = (0, 0x54, value as u16 as u32);
                        count = 1;
                    }
                }
                Pan => {
                    sends[0] = (
                        0,
                        0x7f,
                        u32::from(controller.compile_live_pan(
                            body,
                            request.ports.midi_pan,
                            tables.pan,
                        )),
                    );
                    count = 1;
                }
                Portamento => {
                    controller.compile_live_portamento(
                        request.ports.portamento_time,
                        request.ports.portamento_switch_required,
                        request.ports.portamento_switch,
                        tables.portamento,
                    );
                }
                Filter1EnvelopeIntensity => {
                    sends[0] = (
                        17,
                        0x3c,
                        controller.compile_live_filter1_frequency(
                            body,
                            tables.frequency,
                            tables.amplifier,
                        ),
                    );
                    count = 1;
                }
                Filter1KeyTracking => {
                    controller.compile_live_filter1_key_tracking(body, tables.frequency);
                }
                Filter2Resonance => {
                    let (value, norm) = controller.compile_live_filter2_resonance(
                        body,
                        tables.resonance,
                        tables.comb,
                    );
                    sends[0] = (
                        if norm.is_some() { 18 } else { 14 },
                        if norm.is_some() { 0x66 } else { 0x60 },
                        value,
                    );
                    count = 1;
                    if let Some(norm) = norm {
                        sends[1] = (0, 0x5e, u32::from(norm));
                        count = 2;
                    }
                }
                Filter2EnvelopeIntensity | Filter2KeyTracking => {
                    if compiler == Filter2KeyTracking {
                        controller.compile_live_filter2_key_tracking(body, tables.frequency);
                    }
                    let (value, feedback) = controller.compile_live_filter2_frequency(
                        body,
                        tables.frequency,
                        tables.amplifier,
                        tables.resonance,
                        tables.comb,
                    );
                    sends[0] = (
                        if feedback.is_some() { 14 } else { 17 },
                        if feedback.is_some() { 0x68 } else { 0x64 },
                        value,
                    );
                    count = 1;
                    if let Some(feedback) = feedback {
                        sends[1] = (14, 0x60, feedback);
                        count = 2;
                    }
                }
                EnvelopeParameter {
                    envelope,
                    parameter,
                } => {
                    controller
                        .compile_live_envelope_parameter(body, envelope, parameter, tables.timing)
                        .unwrap();
                }
            }
        }
        let work = crate::virtual_patch_live_work::destination_work(
            request.destination,
            update,
            &controller,
            body,
            request.ports,
            tables,
        );
        let mut publication = crate::actor_descriptors::DescriptorPlan::default();
        if count == 0 {
            publication.work(work.finish);
        } else {
            for (index, (sender, offset, value)) in sends[..count].iter().copied().enumerate() {
                publication.send(
                    sender,
                    offset,
                    value,
                    if index == 0 { work.first } else { work.second },
                    if index + 1 == count { work.finish } else { 0 },
                );
            }
        }
        Ok(CompiledLiveDestination {
            controller,
            update,
            publication,
        })
    }
    pub fn compile_live_portamento(
        &mut self,
        program_time: u8,
        switch_required: bool,
        switch: bool,
        tables: &crate::portamento::PortamentoRates,
    ) -> u32 {
        let program = crate::portamento::PortamentoProgram {
            time: program_time,
            switch_required,
            curve: 0,
        };
        let value = program.rate(tables, switch, self.word(0x132), self.bytes[0x180] as i8);
        self.set_long(0x84, value as i32);
        value
    }
    pub fn compile_live_shaper_depth(&self, body: &[u8; 104]) -> Option<Option<i16>> {
        let mode = self.bytes[0x1e3] & 3;
        if mode == 0 {
            return Some(None);
        }
        let control = crate::controller_shaper::ShaperControl {
            depth: body[48],
            manual_offset: self.word(0x1a6),
            modulation: self.word(0x128),
        };
        let value = if mode == 1 {
            control.drive_depth()
        } else {
            control.waveshaper_depth(crate::controller_shaper::WaveshaperType::from_raw(
                self.bytes[0x1e4] & 15,
            )?)
        };
        Some(Some(value))
    }
    pub fn compile_live_filter2_key_tracking(
        &mut self,
        body: &[u8; 104],
        tables: &crate::controller_filter::ControllerFilterTables,
    ) -> i16 {
        let control = crate::controller_filter::ControllerFilter {
            key_tracking: body[if self.bytes[0x1e2] & 128 != 0 { 38 } else { 43 }],
            key_manual_offset: self.bytes[0x1a0] as i8,
            key_modulation: self.word(0x13e),
            relative_pitch: self.word(0xea),
            ..Default::default()
        };
        let value = control.key_offset(tables);
        self.set_word(0x10e, value);
        value
    }
    pub fn compile_live_filter2_resonance(
        &mut self,
        body: &[u8; 104],
        tables: &LiveFilterResonanceTables,
        comb: &crate::controller_comb::CombControlTables,
    ) -> (u32, Option<u16>) {
        let link = self.bytes[0x1e2] & 128 != 0;
        let control = crate::controller_comb::CombResonanceControl {
            link,
            resonance: body[41],
            linked_resonance: body[36],
            modulation: self.word(0x13a),
            manual_offset: self.bytes[0x19c] as i8,
        };
        let (value, normalization) = if self.bytes[0x1e2] & 0x30 == 0x30 {
            let code = i32::from_be_bytes(self.bytes[0xc4..0xc8].try_into().unwrap());
            (comb.compile_feedback(code, control), None)
        } else {
            let code = control.level() as usize;
            let bank = usize::from(self.bytes[0x1e2] & 0x83 == 0x81);
            let norm = tables.normalization[bank][code];
            self.set_word(0x112, norm as i16);
            (tables.gain[code] as u32, Some(norm))
        };
        self.set_long(0xc0, value as i32);
        (value, normalization)
    }
    pub fn compile_live_filter2_frequency(
        &mut self,
        body: &[u8; 104],
        frequency: &crate::controller_filter::ControllerFilterTables,
        amplifier: &crate::amplifier_control::AmplifierTables,
        gain: &LiveFilterResonanceTables,
        comb: &crate::controller_comb::CombControlTables,
    ) -> (u32, Option<u32>) {
        let control = crate::controller_comb::CombCutoffControl {
            link: self.bytes[0x1e2] & 128 != 0,
            cutoff: body[40],
            linked_cutoff: body[35],
            manual_offset: self.word(0x19a),
            key_offset: self.word(0x10e),
            lfo_offset: self.word(0xe8),
            eg1_intensity: body[42],
            linked_eg1_intensity: body[37],
            eg1_manual_offset: self.bytes[0x19e] as i8,
            eg1_depth_modulation: self.word(0x13c),
            eg1_level: self.word(0x48) as u16,
            velocity: self.bytes[0x37],
            eg1_velocity_sensitivity: body[57],
            additional_offset: self.word(0x10a),
            cutoff_modulation: self.word(0x126),
        };
        let code = control.code(amplifier);
        self.set_long(0xc4, code);
        let (value, feedback) = if self.bytes[0x1e2] & 0x30 == 0x30 {
            let (feedback, _) = self.compile_live_filter2_resonance(body, gain, comb);
            (comb.delay(code), Some(feedback))
        } else {
            (frequency.frequency(code), None)
        };
        self.set_long(0xb8, value as i32);
        (value, feedback)
    }
    pub fn compile_live_filter1_key_tracking(
        &mut self,
        body: &[u8; 104],
        tables: &crate::controller_filter::ControllerFilterTables,
    ) -> i16 {
        let control = crate::controller_filter::ControllerFilter {
            key_tracking: body[38],
            key_manual_offset: self.bytes[0x198] as i8,
            key_modulation: self.word(0x138),
            relative_pitch: self.word(0xea),
            ..Default::default()
        };
        let value = control.key_offset(tables);
        self.set_word(0x10c, value);
        value
    }
    pub fn compile_live_filter1_frequency(
        &mut self,
        body: &[u8; 104],
        tables: &crate::controller_filter::ControllerFilterTables,
        amplifier: &crate::amplifier_control::AmplifierTables,
    ) -> u32 {
        let depth = (i32::from(body[37] & 127) - 64
            + i32::from(self.word(0x136))
            + i32::from(self.bytes[0x196] as i8))
        .clamp(-63, 63);
        let level = amplifier.envelope_level(self.word(0x48) as u16, self.bytes[0x37], body[57])
            as i16 as i32;
        let code = (i32::from(body[35] as i8) << 8)
            + i32::from(self.word(0x190))
            + i32::from(self.word(0x10c))
            + (i32::from(self.word(0xe6)) >> 7)
            + ((depth * level) >> 5)
            + i32::from(self.word(0x108))
            + 2 * i32::from(self.word(0x122));
        let value = tables.frequency(code);
        self.set_long(0xb4, value as i32);
        value
    }
    pub fn compile_live_filter_mix(&self, body: &[u8; 104]) -> u16 {
        (i32::from(body[34] & 127) * 258
            + i32::from(self.word(0x194))
            + 2 * i32::from(self.word(0x120)))
        .clamp(0, 32767) as u16
    }
    pub fn compile_live_filter1_resonance(
        &mut self,
        body: &[u8; 104],
        tables: &LiveFilterResonanceTables,
    ) -> (i32, u16) {
        let code = (i32::from(body[36] & 127)
            + i32::from(self.bytes[0x192] as i8)
            + i32::from(self.word(0x124)))
        .clamp(0, 127) as usize;
        let gain = tables.gain[code];
        let bank = usize::from(self.bytes[0x1e2] & 0x83 == 0x81);
        let normalization = tables.normalization[bank][code];
        self.set_long(0xbc, gain);
        self.set_word(0x110, normalization as i16);
        (gain, normalization)
    }
    /// Whole cached attack-time update for EG1/EG2/EG3. Active-segment
    /// recomposition remains in the envelope service, as in the source.
    pub fn compile_live_envelope_attack(
        &mut self,
        body: &[u8; 104],
        envelope: u8,
        timing: &crate::envelope_segment::EnvelopeTimingTables,
    ) -> Option<()> {
        if envelope >= 3 {
            return None;
        }
        let e = usize::from(envelope);
        if self.bytes[0x94 + e] == 0 {
            let code = (i32::from(body[52 + 8 * e] & 127)
                + i32::from(self.word(0x140 + 8 * e))
                + i32::from(self.bytes[0x1a8 + 8 * e] as i8))
            .clamp(0, 127) as usize;
            self.set_long(0x40 + 24 * e, timing.increments[3][code] as i32);
        }
        Some(())
    }
    /// SYS01528e/015366 and their EG2/EG3 counterparts update only the
    /// currently active decay or release increment. Sustain edits preserve
    /// decay phase and start, adjusting the difference against the old target.
    pub fn compile_live_envelope_parameter(
        &mut self,
        body: &[u8; 104],
        envelope: u8,
        parameter: u8,
        timing: &crate::envelope_segment::EnvelopeTimingTables,
    ) -> Option<()> {
        if envelope >= 3 || parameter >= 4 {
            return None;
        }
        if parameter == 0 {
            return self.compile_live_envelope_attack(body, envelope, timing);
        }
        let e = usize::from(envelope);
        let p = usize::from(parameter);
        let stage = self.bytes[0x94 + e];
        let code = (i32::from(body[52 + 8 * e + p] & 127)
            + i32::from(self.word(0x140 + 8 * e + 2 * p))
            + i32::from(self.bytes[0x1a8 + 8 * e + 2 * p] as i8))
        .clamp(0, 127) as u8;
        if parameter == 2 {
            let target = i16::from(code) * 258;
            let target_offset = 0x4e + 24 * e;
            let difference_offset = 0x46 + 24 * e;
            if stage == 1 {
                let delta = target.wrapping_sub(self.word(target_offset));
                self.set_word(
                    difference_offset,
                    self.word(difference_offset).wrapping_add(delta),
                );
                self.set_word(target_offset, target);
            } else if stage == 2 {
                self.set_word(target_offset, target);
                self.set_word(difference_offset, target);
                self.set_word(0x48 + 24 * e, target);
            }
        } else if stage == parameter {
            let increment = timing.increment(crate::envelope_segment::EnvelopeTiming {
                curve: body[56 + 8 * e],
                time: code,
                velocity: self.bytes[0x37],
                velocity_sensitivity: body[58 + 8 * e],
                note: self.bytes[0x36],
                key_tracking: body[59 + 8 * e],
            });
            self.set_long(0x40 + 24 * e, increment as i32);
        }
        Some(())
    }
    /// Native SYS0026b6/00279e/002882 and the following squared-gain compilers.
    /// Cached scale words are owned by note preparation, not recomputed here.
    pub fn compile_live_mixer(&mut self, body: &[u8; 104], mixer: u8) -> Option<i16> {
        if mixer >= 3 || mixer == 0 && self.bytes[0x1e0] & 15 == 6 {
            return None;
        }
        let index = usize::from(mixer);
        let scale = match mixer {
            0 => self.word(0xfe) as u16,
            1 => self.word(0x100) as u16,
            _ => 0x3333,
        };
        let level = crate::controller_mixer::MixerLevel {
            level: body[30 + index],
            manual_offset: self.word(0x18a + 2 * index),
            modulation: self.word(0x11a + 2 * index),
            scale,
        };
        self.set_word(0x102 + 2 * index, level.composed() as i16);
        Some(level.gain())
    }
    pub fn compile_live_secondary_pitch(
        &mut self,
        body: &[u8; 104],
        table: &crate::controller_secondary::FineTuneTable,
    ) -> i16 {
        let value = crate::controller_secondary::SecondaryPitch {
            semitone: body[28],
            fine_tune: body[29],
            semitone_manual_offset: self.word(0x186),
            fine_manual_offset: self.word(0x188),
            virtual_patch_q16: i32::from_be_bytes(self.bytes[0xa8..0xac].try_into().unwrap()),
        }
        .relative_code(table);
        self.set_word(0xee, value);
        value
    }
    pub fn compile_live_pan(
        &mut self,
        body: &[u8; 104],
        midi_pan: Option<u8>,
        table: &crate::controller_pan::PanTables,
    ) -> u16 {
        let control = crate::controller_pan::PanControl {
            position: body[49],
            manual_offset: self.word(0x1a4),
            modulation: self.word(0x12c),
            timbre_offset: self.bytes[0x1e8] as i8,
            midi_pan,
        };
        let value = control.target();
        self.set_word(0xfc, value as i16);
        table.compile(value)
    }
    /// Whole live callback's stores/dispatch decisions. The returned compiler
    /// must execute before this transition can be treated as a complete update.
    pub fn store_live_destination(
        &mut self,
        destination: u8,
        amount: i32,
        linked_pitch: i32,
    ) -> Option<LiveDestinationUpdate> {
        if destination >= 40 {
            return None;
        }
        let shift = match destination {
            8 | 15 | 16 | 19 | 22..=33 => 7,
            17 | 18 | 20 | 21 | 34..=39 => 8,
            _ => 0,
        };
        let changed = if destination < 2 {
            let offset = 0xa4 + 4 * usize::from(destination);
            let value = amount.wrapping_shl(8);
            let previous = i32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap());
            self.set_long(offset, value);
            previous != value
        } else {
            let value = (amount.clamp(-32767, 32767) >> shift) as i16;
            let offset = if destination == 2 {
                0x116
            } else {
                0x11a + 2 * usize::from(destination - 3)
            };
            let changed = self.word(offset) != value;
            self.set_word(offset, value);
            if destination == 2 {
                self.set_word(0x118, linked_pitch.clamp(-32767, 32767) as i16);
            }
            changed
        };
        if destination == 16 && changed {
            self.bytes[0x1ee] |= 1;
        }
        let compiler = if !changed {
            None
        } else {
            use LiveModulationCompiler::*;
            match destination {
                1 => Some(SecondaryPitch),
                3 => Some(PrimaryMixer),
                4 => Some(SecondaryMixer),
                5 => Some(NoiseMixer),
                6 => Some(FilterMix),
                8 => Some(Filter1Resonance),
                10 => Some(ShaperDepth),
                12 => Some(Pan),
                15 => Some(Portamento),
                17 => Some(Filter1EnvelopeIntensity),
                18 => Some(Filter1KeyTracking),
                19 => Some(Filter2Resonance),
                20 => Some(Filter2EnvelopeIntensity),
                21 => Some(Filter2KeyTracking),
                22..=33 => Some(EnvelopeParameter {
                    envelope: (destination - 22) / 4,
                    parameter: (destination - 22) % 4,
                }),
                _ => None,
            }
        };
        Some(LiveDestinationUpdate { changed, compiler })
    }
}
