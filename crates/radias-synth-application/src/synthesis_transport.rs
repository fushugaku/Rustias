//! Ordered direct AMP and Filter1 parameter delivery. Receiver/sample-job
//! preemption remains an explicit adapter responsibility, not a fixed delay.
use crate::{
    dsp_receiver::{
        ParameterMemory, ReceiveOutcome, packet_bandwidth, receive_filter_command,
        receive_memory_command, receive_pitch_command,
    },
    dsp_transport::{ParameterSendRequest, SendQueueError},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::{
    amplifier_delivery::AmplifierPacket, dsp_control::DspEndpoint, filter::FilterCoefficients,
    filter_control::FilterMixTable, filter_routing::Filter2Coefficients,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarParameter {
    Mixer(u8),
    PrimaryControl,
    PrimaryRatio,
    Pan,
    NoiseGain,
    NoiseFrequency,
    ShaperDepth,
}

#[derive(Clone, Copy)]
pub enum DeliveredSynthesisParameter {
    ActorState {
        opcode: u16,
        active: bool,
    },
    Amplifier(AmplifierPacket),
    Filter1(FilterCoefficients),
    Filter2(Filter2Coefficients),
    Pitch {
        primary: Option<(u16, radias_synth_domain::pitch::PhaseIncrement, i16)>,
        secondary: radias_synth_domain::pitch_receiver::SecondaryPitchCoefficients,
        noise_pitch: Option<i16>,
    },
    UnisonDetune(i16),
    SecondarySync(bool),
    Scalar(ScalarParameter, i16),
    NoiseShape {
        input_gain: i16,
        feedback: i16,
    },
    PrimaryInitialization {
        offset: u16,
        value: u16,
        parameters: radias_synth_domain::primary_oscillator::PrimaryParameters,
    },
}
#[derive(Clone, Copy)]
struct FilterContext {
    normalization: i32,
    base: FilterCoefficients,
    reset: bool,
}
#[derive(Clone, Copy)]
struct Filter2Context {
    normalization: i32,
    base: Filter2Coefficients,
    reset: bool,
}
#[derive(Clone, Copy)]
enum ReceiverContext {
    Construction(usize),
    Filter(FilterContext),
    Filter2(Filter2Context),
    Pitch(Option<[u16; 44]>),
    Scalar(ScalarParameter, Option<[u16; 44]>),
    NoiseShape(Option<[u16; 44]>),
    PrimaryInitialization,
}
pub struct SynthesisParameterTransport {
    queue: OrderedParameterTransport<512>,
    parameter_words: [[u16; 160]; radias_synth_domain::voice_allocation::VOICE_COUNT],
    physical_words: [[u16; 64]; radias_synth_domain::voice_allocation::VOICE_COUNT],
    template_words: [[[u16; 160]; 20]; 2],
    template_ready: [[bool; 20]; 2],
    lifecycle: radias_synth_domain::actor_lifecycle::ActorLifecycle,
    cached_amplifier_targets: [i16; radias_synth_domain::voice_allocation::VOICE_COUNT],
    filter_contexts:
        [Option<(i32, FilterCoefficients)>; radias_synth_domain::voice_allocation::VOICE_COUNT],
    filter_resets: [bool; radias_synth_domain::voice_allocation::VOICE_COUNT],
    filter2_contexts:
        [Option<(i32, Filter2Coefficients)>; radias_synth_domain::voice_allocation::VOICE_COUNT],
    filter2_resets: [bool; radias_synth_domain::voice_allocation::VOICE_COUNT],
    metadata: [Option<ReceiverContext>; 512],
    pitch_rom: Option<[radias_synth_domain::pitch_receiver::PitchReceiverRom; 2]>,
    constructor_filter_mix: Option<FilterMixTable>,
    pitch_dispatch: Option<radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable>,
    pitch_initial: [Option<[u16; 44]>; radias_synth_domain::voice_allocation::VOICE_COUNT],
    metadata_head: usize,
    metadata_len: usize,
}
impl Default for SynthesisParameterTransport {
    fn default() -> Self {
        Self {
            queue: Default::default(),
            parameter_words: [[0; 160]; radias_synth_domain::voice_allocation::VOICE_COUNT],
            physical_words: [[0; 64]; radias_synth_domain::voice_allocation::VOICE_COUNT],
            template_words: [[[0; 160]; 20]; 2],
            template_ready: [[false; 20]; 2],
            lifecycle: Default::default(),
            cached_amplifier_targets: [0; radias_synth_domain::voice_allocation::VOICE_COUNT],
            filter_contexts: [None; radias_synth_domain::voice_allocation::VOICE_COUNT],
            filter_resets: [false; radias_synth_domain::voice_allocation::VOICE_COUNT],
            filter2_contexts: [None; radias_synth_domain::voice_allocation::VOICE_COUNT],
            filter2_resets: [false; radias_synth_domain::voice_allocation::VOICE_COUNT],
            metadata: [None; 512],
            pitch_rom: None,
            constructor_filter_mix: None,
            pitch_dispatch: None,
            pitch_initial: [None; radias_synth_domain::voice_allocation::VOICE_COUNT],
            metadata_head: 0,
            metadata_len: 0,
        }
    }
}
impl SynthesisParameterTransport {
    pub fn configure_constructor_filter_mix(&mut self, table: FilterMixTable) {
        self.constructor_filter_mix = Some(table);
    }
    pub fn publish_actor_descriptors(
        &mut self,
        clock: u64,
        slot: usize,
        plan: &radias_synth_domain::actor_descriptors::DescriptorPlan,
    ) -> Result<(), SendQueueError> {
        use radias_synth_domain::actor_descriptors::DescriptorOperation;
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        if self.queue.remaining_capacity() < plan.operations().len() {
            return Err(SendQueueError::Full);
        }
        self.validate_actor_descriptors(slot, plan)?;
        for operation in plan.operations() {
            match *operation {
                DescriptorOperation::Work(clocks) => self.queue.enqueue_work(clock, clocks)?,
                DescriptorOperation::Send {
                    sender,
                    offset,
                    value,
                    before,
                    after,
                } => {
                    self.queue.enqueue_with_spacing(
                        ParameterSendRequest {
                            endpoint: if slot < 12 {
                                DspEndpoint::Master
                            } else {
                                DspEndpoint::Slave
                            },
                            sender,
                            address: 0x2000 + 160 * (slot % 12) as u32 + u32::from(offset),
                            value,
                            available_clock: clock,
                        },
                        before,
                        after,
                    )?;
                    self.construction_metadata(slot, 1);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn validate_actor_descriptors(
        &self,
        slot: usize,
        plan: &radias_synth_domain::actor_descriptors::DescriptorPlan,
    ) -> Result<(), SendQueueError> {
        use radias_synth_domain::actor_descriptors::DescriptorOperation;
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        for operation in plan.operations() {
            match operation {
                DescriptorOperation::Send { sender: 13, .. }
                    if self.constructor_filter_mix.is_none() =>
                {
                    return Err(SendQueueError::MissingFilterContext);
                }
                DescriptorOperation::Send {
                    sender: 8 | 9 | 17 | 18 | 25 | 26,
                    ..
                } if self.pitch_rom.is_none() => {
                    return Err(SendQueueError::MissingPitchContext);
                }
                DescriptorOperation::Send {
                    sender: 21, value, ..
                } => {
                    let source = *value as usize;
                    if !(0x780..0x1400).contains(&source) || !(source - 0x780).is_multiple_of(160) {
                        return Err(SendQueueError::UnsupportedSender);
                    }
                    if !self.template_ready[usize::from(slot >= 12)][(source - 0x780) / 160] {
                        return Err(SendQueueError::MissingParameterTemplate);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn descriptor_capacity(&self) -> usize {
        self.queue.remaining_capacity()
    }
    pub fn parameter_template_state(
        &self,
        endpoint: DspEndpoint,
        index: usize,
    ) -> Option<radias_synth_domain::parameter_template::ParameterTemplate> {
        let chip = usize::from(endpoint == DspEndpoint::Slave);
        if index >= 20 || !self.template_ready[chip][index] {
            return None;
        }
        Some(radias_synth_domain::parameter_template::ParameterTemplate {
            words: self.template_words[chip][index],
        })
    }
    fn construction_metadata(&mut self, slot: usize, count: usize) {
        for _ in 0..count {
            self.metadata[(self.metadata_head + self.metadata_len) % 512] =
                Some(ReceiverContext::Construction(slot));
            self.metadata_len += 1;
        }
    }
    pub fn restore_construction_state(
        &mut self,
        slot: usize,
        parameters: [u16; 160],
        physical: [u16; 64],
    ) {
        self.parameter_words[slot] = parameters;
        self.physical_words[slot] = physical;
    }
    pub fn physical_parameter_state(&self, slot: usize) -> [u16; 64] {
        self.physical_words[slot]
    }
    pub fn set_actor_lifecycle(
        &mut self,
        lifecycle: radias_synth_domain::actor_lifecycle::ActorLifecycle,
    ) {
        self.lifecycle = lifecycle;
    }
    pub fn actor_lifecycle(&self) -> radias_synth_domain::actor_lifecycle::ActorLifecycle {
        self.lifecycle
    }
    pub fn restore_cached_amplifier_target(&mut self, slot: usize, value: i16) {
        self.cached_amplifier_targets[slot] = value;
    }
    pub fn cached_amplifier_target(&self, slot: usize) -> i16 {
        self.cached_amplifier_targets[slot]
    }
    pub fn install_parameter_template(
        &mut self,
        endpoint: DspEndpoint,
        index: usize,
        template: radias_synth_domain::parameter_template::ParameterTemplate,
    ) -> Result<(), SendQueueError> {
        if index >= 20 {
            return Err(SendQueueError::InvalidSlot);
        }
        let chip = usize::from(endpoint == DspEndpoint::Slave);
        self.template_words[chip][index] = template.words;
        self.template_ready[chip][index] = true;
        Ok(())
    }
    pub fn copy_actor(
        &mut self,
        clock: u64,
        slot: usize,
        binding: radias_synth_domain::actor_copy::ActorTemplateBinding,
        addresses: &radias_synth_domain::actor_copy::ParameterTemplateAddresses,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        let source = usize::from(binding.source(addresses));
        if !(0x780..0x1400).contains(&source) || !(source - 0x780).is_multiple_of(160) {
            return Err(SendQueueError::UnsupportedSender);
        }
        if !self.template_ready[usize::from(slot >= 12)][(source - 0x780) / 160] {
            return Err(SendQueueError::MissingParameterTemplate);
        }
        crate::actor_copy::enqueue(&mut self.queue, clock, slot, binding, addresses)?;
        self.construction_metadata(slot, 1);
        Ok(())
    }
    pub fn initialize_physical_phases(
        &mut self,
        clock: u64,
        slot: usize,
        phase: &radias_synth_domain::actor_startup::PhysicalPhaseInitialization,
    ) -> Result<(), SendQueueError> {
        crate::actor_startup::phases(&mut self.queue, clock, slot, phase)?;
        self.construction_metadata(slot, usize::from(phase.count));
        Ok(())
    }
    pub fn initialize_phase_callback(
        &mut self,
        clock: u64,
        slot: usize,
        kind: radias_synth_domain::actor_startup::PhaseCallback,
        cached_word: i16,
        control: radias_synth_domain::controller_primary::PrimaryControl,
    ) -> Result<(), SendQueueError> {
        crate::actor_startup::callback(&mut self.queue, clock, slot, kind, cached_word, control)?;
        self.construction_metadata(
            slot,
            usize::from(kind != radias_synth_domain::actor_startup::PhaseCallback::None),
        );
        Ok(())
    }
    pub fn initialize_slot_counter(
        &mut self,
        clock: u64,
        slot: usize,
        selected: bool,
        controller_slot: u8,
        seeds: &radias_synth_domain::controller_noise::FormantCounterSeeds,
    ) -> Result<(), SendQueueError> {
        crate::actor_startup::counter(
            &mut self.queue,
            clock,
            slot,
            selected,
            controller_slot,
            seeds,
        )?;
        self.construction_metadata(slot, usize::from(selected));
        Ok(())
    }
    pub fn prime_actor(
        &mut self,
        clock: u64,
        slot: usize,
        priming: radias_synth_domain::actor_startup::CoefficientPriming,
    ) -> Result<(), SendQueueError> {
        crate::actor_startup::prime(&mut self.queue, clock, slot, priming)?;
        self.construction_metadata(slot, if priming.pickup() { 2 } else { 1 });
        Ok(())
    }
    pub fn activate_actor(&mut self, clock: u64, slot: usize) -> Result<(), SendQueueError> {
        crate::actor_lifecycle::activate(&mut self.queue, &mut self.lifecycle, clock, slot)?;
        self.construction_metadata(slot, 1);
        Ok(())
    }
    pub fn detach_actor(&mut self, clock: u64, slot: usize) -> Result<(), SendQueueError> {
        let active = self.lifecycle.contains(slot);
        crate::actor_lifecycle::detach(&mut self.queue, &mut self.lifecycle, clock, slot)?;
        self.construction_metadata(slot, usize::from(active));
        Ok(())
    }
    pub fn reset_actor_amplifier(
        &mut self,
        clock: u64,
        slot: usize,
        tables: &radias_synth_domain::amplifier_delivery::AmplifierRateTable,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        crate::actor_lifecycle::reset_amplifier(
            &mut self.queue,
            &mut self.cached_amplifier_targets[slot],
            clock,
            slot,
            tables,
        )?;
        self.construction_metadata(slot, 1);
        Ok(())
    }
    /// Whole SYS0202e0 VA/Noise/Formant descriptor caller. The queued gaps are
    /// caller work after the previous sender returns, including busy stalls.
    /// No descriptor or coefficient becomes visible before its own HPI commit.
    pub fn initialize_primary(
        &mut self,
        clock: u64,
        slot: usize,
        initialization: &radias_synth_domain::primary_initialization::PrimaryInitialization,
    ) -> Result<(), SendQueueError> {
        use radias_synth_domain::primary_initialization::PrimaryInitialization;
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        if usize::from(initialization.count) > initialization.constants.len()
            || initialization.words().iter().any(|word| word.offset >= 160)
        {
            return Err(SendQueueError::UnsupportedSender);
        }
        let count = usize::from(initialization.count) + 1;
        if self.queue.remaining_capacity() < count {
            return Err(SendQueueError::Full);
        }
        for index in 0..count {
            let (offset, value) = if index == 0 {
                (1, initialization.generator)
            } else {
                let word = initialization.constants[index - 1];
                (word.offset, word.value)
            };
            let before = match index {
                0 => PrimaryInitialization::FIRST_SENDER_GAP,
                1 => PrimaryInitialization::TEMPLATE_SETUP_GAP,
                _ => PrimaryInitialization::NEXT_CONSTANT_GAP,
            };
            let after = if count == 1 {
                PrimaryInitialization::NO_CONSTANTS_RETURN_GAP
            } else if index + 1 == count {
                PrimaryInitialization::CALLER_RETURN_GAP
            } else {
                0
            };
            self.queue.enqueue_with_spacing(
                ParameterSendRequest {
                    endpoint: if slot < 12 {
                        DspEndpoint::Master
                    } else {
                        DspEndpoint::Slave
                    },
                    sender: 0,
                    address: 0x2000 + 160 * (slot % 12) as u32 + u32::from(offset),
                    value: u32::from(value),
                    available_clock: clock,
                },
                before,
                after,
            )?;
            self.metadata[(self.metadata_head + self.metadata_len) % 512] =
                Some(ReceiverContext::PrimaryInitialization);
            self.metadata_len += 1;
        }
        Ok(())
    }
    pub fn caller_available_clock(&self) -> u64 {
        self.queue.caller_available_clock()
    }
    fn send(
        &mut self,
        clock: u64,
        slot: usize,
        sender: u8,
        offset: u32,
        value: u32,
        context: Option<ReceiverContext>,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        self.queue.enqueue(ParameterSendRequest {
            endpoint: if slot < 12 {
                DspEndpoint::Master
            } else {
                DspEndpoint::Slave
            },
            sender,
            address: 0x2000 + 160 * (slot % 12) as u32 + offset,
            value,
            available_clock: clock,
        })?;
        self.metadata[(self.metadata_head + self.metadata_len) % 512] = context;
        self.metadata_len += 1;
        Ok(())
    }
    pub fn enqueue(
        &mut self,
        clock: u64,
        slot: usize,
        packet: AmplifierPacket,
    ) -> Result<(), SendQueueError> {
        match packet {
            AmplifierPacket::RateAndTarget { rate, target } => self.send(
                clock,
                slot,
                19,
                124,
                (u32::from(rate) << 16) | u32::from(target as u16),
                None,
            ),
            AmplifierPacket::Target(target) => {
                self.send(clock, slot, 0, 125, u32::from(target as u16), None)
            }
        }
    }
    pub fn configure_pitch_receivers(
        &mut self,
        rom: [radias_synth_domain::pitch_receiver::PitchReceiverRom; 2],
        dispatch: radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable,
    ) {
        self.pitch_rom = Some(rom);
        self.pitch_dispatch = Some(dispatch);
    }
    pub fn pitch_enabled(&self) -> bool {
        self.pitch_rom.is_some() && self.pitch_dispatch.is_some()
    }
    pub fn clear_pending(&mut self) {
        let rom = self.pitch_rom.take();
        let mix = self.constructor_filter_mix.take();
        let dispatch = self.pitch_dispatch;
        *self = Self::default();
        self.pitch_rom = rom;
        self.constructor_filter_mix = mix;
        self.pitch_dispatch = dispatch;
    }
    pub fn reset_pitch(&mut self, slot: usize, code: u16, offset: i16, sync: bool, detune: i16) {
        let mut words = [0; 44];
        words[0] = code;
        words[4] = detune as u16;
        words[35] = offset as u16;
        words[41] = u16::from(sync);
        self.pitch_initial[slot] = Some(words);
    }
    pub fn pitch_state(&self, slot: usize) -> [u16; 44] {
        self.parameter_words[slot][2..46].try_into().unwrap()
    }
    pub fn restore_pitch(&mut self, slot: usize, state: [u16; 44]) {
        self.parameter_words[slot][2..46].copy_from_slice(&state);
        self.pitch_initial[slot] = None;
    }
    pub fn primary_pitch(
        &mut self,
        clock: u64,
        slot: usize,
        selection: u8,
        code: u16,
    ) -> Result<(), SendQueueError> {
        let table = self
            .pitch_dispatch
            .ok_or(SendQueueError::MissingPitchContext)?;
        let sender = table
            .sender(selection)
            .ok_or(SendQueueError::UnsupportedSender)?;
        self.send_pitch(clock, slot, sender, 2, code)
    }
    pub fn secondary_pitch(
        &mut self,
        clock: u64,
        slot: usize,
        offset: i16,
    ) -> Result<(), SendQueueError> {
        self.send_pitch(clock, slot, 8, 37, offset as u16)
    }
    pub fn unison_detune(
        &mut self,
        clock: u64,
        slot: usize,
        detune: i16,
    ) -> Result<(), SendQueueError> {
        self.send_pitch(clock, slot, 9, 6, detune as u16)
    }
    pub fn secondary_sync(
        &mut self,
        clock: u64,
        slot: usize,
        sync: bool,
    ) -> Result<(), SendQueueError> {
        self.send_pitch(clock, slot, 0, 43, u16::from(sync))
    }
    fn send_pitch(
        &mut self,
        clock: u64,
        slot: usize,
        sender: u8,
        offset: u32,
        value: u16,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        if !self.pitch_enabled() {
            return Err(SendQueueError::MissingPitchContext);
        }
        self.send(
            clock,
            slot,
            sender,
            offset,
            u32::from(value),
            Some(ReceiverContext::Pitch(self.pitch_initial[slot])),
        )?;
        self.pitch_initial[slot] = None;
        Ok(())
    }
    pub fn configure_filter(&mut self, slot: usize, normalization: i32, base: FilterCoefficients) {
        self.filter_contexts[slot] = Some((normalization, base));
    }
    pub fn mixer_level(
        &mut self,
        clock: u64,
        slot: usize,
        band: u8,
        value: i16,
    ) -> Result<(), SendQueueError> {
        if band >= 3 {
            return Err(SendQueueError::UnsupportedSender);
        }
        self.send_scalar(
            clock,
            slot,
            47 + 2 * u32::from(band),
            value,
            ScalarParameter::Mixer(band),
        )
    }
    pub fn primary_control(
        &mut self,
        clock: u64,
        slot: usize,
        selection: u8,
        value: i16,
    ) -> Result<(), SendQueueError> {
        if selection & 48 == 32 {
            return self.unison_detune(clock, slot, value);
        }
        let offset = if selection & 63 == 3 { 11 } else { 6 };
        self.send_scalar(clock, slot, offset, value, ScalarParameter::PrimaryControl)
    }
    pub fn primary_ratio(
        &mut self,
        clock: u64,
        slot: usize,
        value: i16,
    ) -> Result<(), SendQueueError> {
        self.send_scalar(clock, slot, 8, value, ScalarParameter::PrimaryRatio)
    }
    pub fn pan(&mut self, clock: u64, slot: usize, value: i16) -> Result<(), SendQueueError> {
        self.send_scalar(clock, slot, 127, value, ScalarParameter::Pan)
    }
    pub fn shaper_depth(
        &mut self,
        clock: u64,
        slot: usize,
        value: i16,
    ) -> Result<(), SendQueueError> {
        self.send_scalar(clock, slot, 84, value, ScalarParameter::ShaperDepth)
    }
    pub fn noise_gain(
        &mut self,
        clock: u64,
        slot: usize,
        value: i16,
    ) -> Result<(), SendQueueError> {
        self.send_scalar(clock, slot, 6, value, ScalarParameter::NoiseGain)
    }
    pub fn noise_frequency(
        &mut self,
        clock: u64,
        slot: usize,
        value: i16,
    ) -> Result<(), SendQueueError> {
        self.send_scalar(clock, slot, 8, value, ScalarParameter::NoiseFrequency)
    }
    pub fn noise_shape(
        &mut self,
        clock: u64,
        slot: usize,
        value: u32,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        self.send(
            clock,
            slot,
            14,
            16,
            value,
            Some(ReceiverContext::NoiseShape(self.pitch_initial[slot])),
        )?;
        self.pitch_initial[slot] = None;
        Ok(())
    }
    fn send_scalar(
        &mut self,
        clock: u64,
        slot: usize,
        offset: u32,
        value: i16,
        parameter: ScalarParameter,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        let initial = if offset < 46 {
            self.pitch_initial[slot]
        } else {
            None
        };
        self.send(
            clock,
            slot,
            0,
            offset,
            u32::from(value as u16),
            Some(ReceiverContext::Scalar(parameter, initial)),
        )?;
        if offset < 46 {
            self.pitch_initial[slot] = None;
        }
        Ok(())
    }
    pub fn parameter_state(&self, slot: usize) -> [u16; 160] {
        self.parameter_words[slot]
    }
    pub fn restore_parameters(&mut self, slot: usize, words: [u16; 160]) {
        self.parameter_words[slot] = words;
        self.pitch_initial[slot] = None;
        self.filter_resets[slot] = false;
        self.filter2_resets[slot] = false;
    }
    pub fn reset_filter(&mut self, slot: usize) {
        // Preserve earlier queued commands until the first new filter payload
        // reaches the receiver. Held Mono restoration cancels this reset.
        self.filter_resets[slot] = true;
        self.filter_contexts[slot] = None;
    }
    pub fn filter_state(&self, slot: usize) -> [u16; 16] {
        self.parameter_words[slot][56..72].try_into().unwrap()
    }
    pub fn restore_filter(&mut self, slot: usize, state: [u16; 16]) {
        self.parameter_words[slot][56..72].copy_from_slice(&state);
        self.filter_resets[slot] = false;
    }
    pub fn filter_frequency(
        &mut self,
        clock: u64,
        slot: usize,
        value: i32,
    ) -> Result<(), SendQueueError> {
        self.send_filter(clock, slot, 17, 60, value)
    }
    pub fn filter_resonance(
        &mut self,
        clock: u64,
        slot: usize,
        value: i32,
    ) -> Result<(), SendQueueError> {
        self.send_filter(clock, slot, 18, 62, value)
    }
    fn send_filter(
        &mut self,
        clock: u64,
        slot: usize,
        sender: u8,
        offset: u32,
        value: i32,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        let Some((normalization, base)) = self.filter_contexts[slot] else {
            return Err(SendQueueError::MissingFilterContext);
        };
        let context = FilterContext {
            normalization,
            base,
            reset: self.filter_resets[slot],
        };
        self.send(
            clock,
            slot,
            sender,
            offset,
            value as u32,
            Some(ReceiverContext::Filter(context)),
        )?;
        self.filter_resets[slot] = false;
        Ok(())
    }
    pub fn configure_filter2(
        &mut self,
        slot: usize,
        normalization: i32,
        base: Filter2Coefficients,
    ) {
        self.filter2_contexts[slot] = Some((normalization, base));
    }
    pub fn reset_filter2(&mut self, slot: usize) {
        self.filter2_resets[slot] = true;
        self.filter2_contexts[slot] = None;
    }
    pub fn filter2_frequency(
        &mut self,
        clock: u64,
        slot: usize,
        value: i32,
    ) -> Result<(), SendQueueError> {
        self.send_filter2(clock, slot, 17, 100, value as u32)
    }
    pub fn filter2_resonance(
        &mut self,
        clock: u64,
        slot: usize,
        value: i32,
    ) -> Result<(), SendQueueError> {
        self.send_filter2(clock, slot, 18, 102, value as u32)
    }
    pub fn filter2_input_gain(
        &mut self,
        clock: u64,
        slot: usize,
        value: i16,
    ) -> Result<(), SendQueueError> {
        self.send_filter2(clock, slot, 0, 94, u32::from(value as u16))
    }
    pub fn comb_delay(
        &mut self,
        clock: u64,
        slot: usize,
        value: u32,
    ) -> Result<(), SendQueueError> {
        self.send_filter2(clock, slot, 14, 104, value)
    }
    pub fn comb_feedback(
        &mut self,
        clock: u64,
        slot: usize,
        value: u32,
    ) -> Result<(), SendQueueError> {
        self.send_filter2(clock, slot, 14, 96, value)
    }
    fn send_filter2(
        &mut self,
        clock: u64,
        slot: usize,
        sender: u8,
        offset: u32,
        value: u32,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        let Some((normalization, base)) = self.filter2_contexts[slot] else {
            return Err(SendQueueError::MissingFilterContext);
        };
        self.send(
            clock,
            slot,
            sender,
            offset,
            value,
            Some(ReceiverContext::Filter2(Filter2Context {
                normalization,
                base,
                reset: self.filter2_resets[slot],
            })),
        )?;
        self.filter2_resets[slot] = false;
        Ok(())
    }
    pub fn pending(&self) -> usize {
        self.queue.pending()
    }
    pub fn advance_until(
        &mut self,
        end_clock: u64,
        commit: impl FnMut(u64, usize, DeliveredSynthesisParameter),
    ) {
        self.advance_until_with_readiness(end_clock, |_| 0, commit);
    }
    pub fn advance_until_with_readiness(
        &mut self,
        end_clock: u64,
        ready_port: impl FnMut(u64) -> u8,
        mut commit: impl FnMut(u64, usize, DeliveredSynthesisParameter),
    ) {
        let words = &mut self.parameter_words;
        let physical = &mut self.physical_words;
        let templates = &mut self.template_words;
        let pitch_rom = &self.pitch_rom;
        let constructor_filter_mix = &self.constructor_filter_mix;
        let metadata = &mut self.metadata;
        let head = &mut self.metadata_head;
        let len = &mut self.metadata_len;
        self.queue
            .advance_until(end_clock, ready_port, |clock, packet| {
                let context = metadata[*head].take();
                *head = (*head + 1) % 512;
                *len -= 1;
                let payload = packet.words();
                if let Some(ReceiverContext::Construction(slot)) = context {
                    let chip = usize::from(packet.endpoint == DspEndpoint::Slave);
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut memory = ConstructionMemory {
                        mailbox,
                        parameters: words,
                        physical,
                        templates,
                        rom: pitch_rom.as_ref().map(|rom| &rom[chip]),
                        chip,
                    };
                    let outcome = match payload[1] {
                        28 | 32 => crate::dsp_receiver::receive_phase_command(&mut memory, 0x100),
                        34 | 38 | 39 => {
                            crate::dsp_receiver::receive_shared_command(&mut memory, 0x100)
                        }
                        16 | 17 | 23..=27 | 29 | 30 => receive_pitch_command(&mut memory, 0x100),
                        18..=21 => {
                            const UNUSED_MIX: FilterMixTable = FilterMixTable {
                                weights: [[0; 128]; 5],
                            };
                            let mix = constructor_filter_mix.as_ref().unwrap_or(&UNUSED_MIX);
                            receive_filter_command(&mut memory, 0x100, mix)
                        }
                        _ => receive_memory_command(&mut memory, 0x100),
                    };
                    if outcome == ReceiveOutcome::Ready {
                        commit(
                            clock,
                            slot,
                            DeliveredSynthesisParameter::ActorState {
                                opcode: payload[1],
                                active: memory.parameters[slot][0] != 0,
                            },
                        );
                    }
                    return;
                }
                if payload.len() < 5 || payload[0] != 6 || payload[2] != 0 {
                    return;
                }
                let chip = usize::from(packet.endpoint == DspEndpoint::Slave);
                let address = usize::from(payload[3]);
                if address < 0x2000 {
                    return;
                }
                let local = (address - 0x2000) / 160;
                if local >= 12 {
                    return;
                }
                let slot = local + 12 * chip;
                let offset = (address - 0x2000) % 160;
                if let Some(ReceiverContext::PrimaryInitialization) = context {
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = ScalarMemory {
                        mailbox,
                        words: &mut words[slot],
                        start: (0x2000 + 160 * local) as u16,
                    };
                    if receive_memory_command(&mut view, 0x100) != ReceiveOutcome::Ready {
                        return;
                    }
                    if let Some(parameters) =
                        radias_synth_domain::primary_parameters::decode(view.words)
                    {
                        commit(
                            clock,
                            slot,
                            DeliveredSynthesisParameter::PrimaryInitialization {
                                offset: offset as u16,
                                value: view.words[offset],
                                parameters,
                            },
                        );
                    }
                    return;
                }
                let amp = match (payload[1], offset, payload.len()) {
                    (22, 124, 6) => Some(AmplifierPacket::RateAndTarget {
                        rate: payload[4],
                        target: payload[5] as i16,
                    }),
                    (0, 125, 5) => Some(AmplifierPacket::Target(payload[4] as i16)),
                    _ => None,
                };
                if let Some(amp) = amp {
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = ScalarMemory {
                        mailbox,
                        words: &mut words[slot],
                        start: (0x2000 + 160 * local) as u16,
                    };
                    if receive_memory_command(&mut view, 0x100) != ReceiveOutcome::Ready {
                        return;
                    }
                    commit(clock, slot, DeliveredSynthesisParameter::Amplifier(amp));
                    return;
                }
                if let Some(ReceiverContext::NoiseShape(initial)) = context {
                    if let Some(initial) = initial {
                        words[slot][2..46].copy_from_slice(&initial);
                    }
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = ScalarMemory {
                        mailbox,
                        words: &mut words[slot],
                        start: (0x2000 + 160 * local) as u16,
                    };
                    if receive_memory_command(&mut view, 0x100) != ReceiveOutcome::Ready {
                        return;
                    }
                    commit(
                        clock,
                        slot,
                        DeliveredSynthesisParameter::NoiseShape {
                            input_gain: view.words[offset] as i16,
                            feedback: view.words[offset ^ 1] as i16,
                        },
                    );
                    return;
                }
                if let Some(ReceiverContext::Scalar(parameter, initial)) = context {
                    if let Some(initial) = initial {
                        words[slot][2..46].copy_from_slice(&initial);
                    }
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = ScalarMemory {
                        mailbox,
                        words: &mut words[slot],
                        start: (0x2000 + 160 * local) as u16,
                    };
                    if receive_memory_command(&mut view, 0x100) != ReceiveOutcome::Ready {
                        return;
                    }
                    commit(
                        clock,
                        slot,
                        DeliveredSynthesisParameter::Scalar(parameter, view.words[offset] as i16),
                    );
                    return;
                }
                if let Some(ReceiverContext::Pitch(initial)) = context {
                    let Some(rom) = pitch_rom else {
                        return;
                    };
                    if let Some(initial) = initial {
                        words[slot][2..46].copy_from_slice(&initial);
                    }
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = PitchMemory {
                        mailbox,
                        words: (&mut words[slot][2..46]).try_into().unwrap(),
                        start: (0x2000 + 160 * local + 2) as u16,
                        rom: &rom[chip],
                    };
                    let outcome = if payload[1] == 0 {
                        receive_memory_command(&mut view, 0x100)
                    } else {
                        receive_pitch_command(&mut view, 0x100)
                    };
                    if outcome != ReceiveOutcome::Ready {
                        return;
                    }
                    if payload[1] == 0 {
                        commit(
                            clock,
                            slot,
                            DeliveredSynthesisParameter::SecondarySync(view.words[41] != 0),
                        );
                        return;
                    }
                    if payload[1] == 27 {
                        commit(
                            clock,
                            slot,
                            DeliveredSynthesisParameter::UnisonDetune(view.words[4] as i16),
                        );
                        return;
                    }
                    let long =
                        |i: usize| (u32::from(view.words[i]) << 16) | u32::from(view.words[i + 1]);
                    let increment = radias_synth_domain::pitch::PhaseIncrement(long(2));
                    let bandwidth = packet_bandwidth(&view, increment.0);
                    commit(
                        clock,
                        slot,
                        DeliveredSynthesisParameter::Pitch {
                            primary: (payload[1] != 17).then_some((
                                view.words[0],
                                increment,
                                bandwidth,
                            )),
                            secondary:
                                radias_synth_domain::pitch_receiver::SecondaryPitchCoefficients {
                                    increment: radias_synth_domain::pitch::PhaseIncrement(long(36)),
                                    edge: view.words[42] as i16,
                                    bandwidth: view.words[43] as i16,
                                },
                            noise_pitch: match payload[1] {
                                29 => Some(view.words[8] as i16),
                                30 => Some(view.words[9] as i16),
                                _ => None,
                            },
                        },
                    );
                    return;
                }
                if let Some(ReceiverContext::Filter2(Filter2Context {
                    normalization,
                    base,
                    reset,
                })) = context
                {
                    if reset {
                        words[slot][94..112].fill(0);
                        words[slot][94] = base.input_gain as u16;
                        words[slot][96] = (base.feedback >> 16) as u16;
                        words[slot][97] = base.feedback as u16;
                        words[slot][104] = (base.integrator_gain >> 16) as u16;
                        words[slot][105] = base.integrator_gain as u16;
                    }
                    let mut mailbox = [0; 9];
                    mailbox[..payload.len()].copy_from_slice(payload);
                    let mut view = Filter2Memory {
                        parameters: ScalarMemory {
                            mailbox,
                            words: &mut words[slot],
                            start: (0x2000 + 160 * local) as u16,
                        },
                        normalization,
                    };
                    const UNUSED_MIX: FilterMixTable = FilterMixTable {
                        weights: [[0; 128]; 5],
                    };
                    let outcome = if matches!(payload[1], 0 | 1) {
                        receive_memory_command(&mut view, 0x100)
                    } else {
                        receive_filter_command(&mut view, 0x100, &UNUSED_MIX)
                    };
                    if outcome != ReceiveOutcome::Ready {
                        return;
                    }
                    let long = |i: usize| {
                        ((u32::from(view.parameters.words[i]) << 16)
                            | u32::from(view.parameters.words[i ^ 1]))
                            as i32
                    };
                    commit(
                        clock,
                        slot,
                        DeliveredSynthesisParameter::Filter2(Filter2Coefficients {
                            input_gain: view.parameters.words[94] as i16,
                            feedback: long(96),
                            integrator_gain: long(104),
                            output: base.output,
                        }),
                    );
                    return;
                }
                if !matches!(
                    (payload[1], offset, payload.len()),
                    (19, 60, 6) | (20, 62, 6)
                ) {
                    return;
                }
                let Some(ReceiverContext::Filter(FilterContext {
                    normalization,
                    base: mut coefficients,
                    reset,
                })) = context
                else {
                    return;
                };
                if reset {
                    words[slot][56..72].fill(0);
                }
                let mut mailbox = [0; 9];
                mailbox[..payload.len()].copy_from_slice(payload);
                let mut view = FilterMemory {
                    mailbox,
                    words: (&mut words[slot][56..72]).try_into().unwrap(),
                    start: (0x2000 + 160 * local + 56) as u16,
                    normalization,
                };
                // Neither frequency nor resonance handlers use the mix LUT.
                const UNUSED_MIX: FilterMixTable = FilterMixTable {
                    weights: [[0; 128]; 5],
                };
                if receive_filter_command(&mut view, 0x100, &UNUSED_MIX) != ReceiveOutcome::Ready {
                    return;
                }
                let long = |i: usize| {
                    ((u32::from(view.words[i]) << 16) | u32::from(view.words[i + 1])) as i32
                };
                coefficients.feedback = long(0);
                coefficients.integrator_gain = long(8);
                coefficients.post_gain = view.words[12] as i16;
                coefficients.post_feedback = view.words[14] as i16;
                commit(
                    clock,
                    slot,
                    DeliveredSynthesisParameter::Filter1(coefficients),
                );
            });
    }
}
struct ScalarMemory<'a> {
    mailbox: [u16; 9],
    words: &'a mut [u16; 160],
    start: u16,
}
impl ParameterMemory for ScalarMemory<'_> {
    fn read_word(&self, address: u16) -> u16 {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)]
        } else {
            self.words
                .get(usize::from(address.wrapping_sub(self.start)))
                .copied()
                .unwrap_or(0)
        }
    }
    fn write_word(&mut self, address: u16, value: u16) {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)] = value;
        } else if let Some(word) = self
            .words
            .get_mut(usize::from(address.wrapping_sub(self.start)))
        {
            *word = value;
        }
    }
}
struct Filter2Memory<'a> {
    parameters: ScalarMemory<'a>,
    normalization: i32,
}
impl ParameterMemory for Filter2Memory<'_> {
    fn read_word(&self, address: u16) -> u16 {
        match address {
            0x4024 => (self.normalization >> 16) as u16,
            0x4025 => self.normalization as u16,
            _ => self.parameters.read_word(address),
        }
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.parameters.write_word(address, value);
    }
}
struct PitchMemory<'a> {
    mailbox: [u16; 9],
    words: &'a mut [u16; 44],
    start: u16,
    rom: &'a radias_synth_domain::pitch_receiver::PitchReceiverRom,
}
impl ParameterMemory for PitchMemory<'_> {
    fn read_word(&self, address: u16) -> u16 {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)]
        } else if (0x4000..0x4900).contains(&address) {
            self.rom.word(address)
        } else {
            self.words
                .get(usize::from(address.wrapping_sub(self.start)))
                .copied()
                .unwrap_or(0)
        }
    }
    fn write_word(&mut self, address: u16, value: u16) {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)] = value;
        } else if let Some(word) = self
            .words
            .get_mut(usize::from(address.wrapping_sub(self.start)))
        {
            *word = value;
        }
    }
}
struct FilterMemory<'a> {
    mailbox: [u16; 9],
    words: &'a mut [u16; 16],
    start: u16,
    normalization: i32,
}
impl ParameterMemory for FilterMemory<'_> {
    fn read_word(&self, address: u16) -> u16 {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)]
        } else if address == 0x4024 {
            (self.normalization >> 16) as u16
        } else if address == 0x4025 {
            self.normalization as u16
        } else {
            self.words
                .get(usize::from(address.wrapping_sub(self.start)))
                .copied()
                .unwrap_or(0)
        }
    }
    fn write_word(&mut self, address: u16, value: u16) {
        if (0x100..0x109).contains(&address) {
            self.mailbox[usize::from(address - 0x100)] = value;
        } else if let Some(word) = self
            .words
            .get_mut(usize::from(address.wrapping_sub(self.start)))
        {
            *word = value;
        }
    }
}

/// The constructor and moving controls share actor banks and FIFO ownership.
struct ConstructionMemory<'a> {
    mailbox: [u16; 9],
    parameters: &'a mut [[u16; 160]; radias_synth_domain::voice_allocation::VOICE_COUNT],
    physical: &'a mut [[u16; 64]; radias_synth_domain::voice_allocation::VOICE_COUNT],
    templates: &'a mut [[[u16; 160]; 20]; 2],
    rom: Option<&'a radias_synth_domain::pitch_receiver::PitchReceiverRom>,
    chip: usize,
}
impl ParameterMemory for ConstructionMemory<'_> {
    fn read_word(&self, address: u16) -> u16 {
        let address = usize::from(address);
        if (0x100..0x109).contains(&address) {
            self.mailbox[address - 0x100]
        } else if (0x2000..0x2780).contains(&address) {
            let relative = address - 0x2000;
            self.parameters[self.chip * 12 + relative / 160][relative % 160]
        } else if (0x3000..0x3300).contains(&address) {
            let relative = address - 0x3000;
            self.physical[self.chip * 12 + relative / 64][relative % 64]
        } else if (0x780..0x1400).contains(&address) {
            let relative = address - 0x780;
            self.templates[self.chip][relative / 160][relative % 160]
        } else if let Some(rom) = self.rom {
            rom.word(address as u16)
        } else {
            0
        }
    }
    fn write_word(&mut self, address: u16, value: u16) {
        let address = usize::from(address);
        if (0x100..0x109).contains(&address) {
            self.mailbox[address - 0x100] = value;
        } else if (0x2000..0x2780).contains(&address) {
            let relative = address - 0x2000;
            self.parameters[self.chip * 12 + relative / 160][relative % 160] = value;
        } else if (0x3000..0x3300).contains(&address) {
            let relative = address - 0x3000;
            self.physical[self.chip * 12 + relative / 64][relative % 64] = value;
        } else if (0x780..0x1400).contains(&address) {
            let relative = address - 0x780;
            self.templates[self.chip][relative / 160][relative % 160] = value;
        }
    }
}
