//! Allocation and routing use case for the audible native reconstruction.
//! Two serial Insert FX per timbre, then the stereo Master FX. This is a
//! native audition topology; original FXD03 bus/arithmetic parity is unproven.
use alloc::boxed::Box;
#[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
use alloc::vec;
#[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
use radias_synth_domain::effect_audio::DELAY_WORDS;
use radias_synth_domain::{
    Sample,
    effect_audio::{
        EffectAudioContext, EffectAudioProcessor, EffectAudioProgram, EffectAudioSettings,
    },
    pan::StereoFrame,
};

use crate::polyphony::TIMBRE_COUNT;
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
use alloc::{string::String, vec::Vec};
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
struct ChainEffect {
    id: String,
    processor: EffectAudioProcessor,
    memory: Box<[f32]>,
}
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
fn effect_memory(kind: u8) -> Result<Box<[f32]>, &'static str> {
    let mut memory = Vec::new();
    let words = radias_synth_domain::effect_audio::delay_storage_words(kind);
    if words != 0 {
        memory
            .try_reserve_exact(words)
            .map_err(|_| "Not enough memory for this effect")?;
        memory.resize(words, 0.0);
    }
    Ok(memory.into_boxed_slice())
}
pub const MASTER_EFFECT_SLOT: usize = 2 * TIMBRE_COUNT;
// Keep the original insert and Master indices stable for saved browser bindings.
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
pub const TIMBRE_EFFECTS: usize = 4;
#[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
pub const TIMBRE_EFFECTS: usize = 2;
pub const EXTRA_EFFECT_START: usize = MASTER_EFFECT_SLOT + 1;
pub const EFFECT_SLOTS: usize = EXTRA_EFFECT_START + TIMBRE_COUNT * (TIMBRE_EFFECTS - 2);
pub const fn timbre_effect_slot(timbre: usize, role: usize) -> usize {
    if role < 2 {
        2 * timbre + role
    } else {
        EXTRA_EFFECT_START + 2 * timbre + role - 2
    }
}
pub const fn effect_uses_master(slot: usize) -> bool {
    slot == MASTER_EFFECT_SLOT || slot >= EXTRA_EFFECT_START && slot < EFFECT_SLOTS
}
pub struct EffectAudioRack {
    instances: [EffectAudioProcessor; EFFECT_SLOTS],
    memory: [Box<[f32]>; EFFECT_SLOTS],
    contexts: [EffectAudioContext; TIMBRE_COUNT + 1],
    held: [[u8; 128]; TIMBRE_COUNT],
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    chains: [Vec<ChainEffect>; TIMBRE_COUNT],
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    active_fixed: [u8; TIMBRE_COUNT],
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    active_extra: [Vec<usize>; TIMBRE_COUNT],
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    master_active: bool,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    any_active: bool,
}
impl EffectAudioRack {
    /// All delay allocation occurs on the caller's control thread.
    pub fn new(settings: [EffectAudioSettings; EFFECT_SLOTS]) -> Self {
        #[allow(unused_mut)]
        let mut rack = Self {
            instances: settings.map(EffectAudioProcessor::new),
            memory: core::array::from_fn(|slot| {
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                {
                    effect_memory(settings[slot].program.kind).expect("Effect scratch allocation")
                }
                #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
                {
                    let _ = slot;
                    vec![0.0; DELAY_WORDS].into_boxed_slice()
                }
            }),
            contexts: [Default::default(); TIMBRE_COUNT + 1],
            held: [[0; 128]; TIMBRE_COUNT],
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            chains: core::array::from_fn(|_| Vec::new()),
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            active_fixed: [0; TIMBRE_COUNT],
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            active_extra: core::array::from_fn(|_| Vec::new()),
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            master_active: false,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            any_active: false,
        };
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        rack.refresh_fixed_plan();
        rack
    }
    pub fn programs(&self) -> [EffectAudioProgram; EFFECT_SLOTS] {
        core::array::from_fn(|i| self.instances[i].settings().program)
    }
    pub fn active(&self) -> bool {
        #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
        {
            self.instances
                .iter()
                .any(|fx| fx.settings().program.kind != 0 && fx.settings().program.enabled)
        }
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        {
            self.any_active
        }
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn refresh_active(&mut self) {
        // Keep the original global predicate, including a paired odd insert.
        // It selects the browser's bus/PCM saturation path independently of
        // whether that particular insert is visited by the rack.
        self.any_active = self
            .instances
            .iter()
            .any(|fx| fx.settings().program.kind != 0 && fx.settings().program.enabled)
            || self.active_extra.iter().any(|chain| !chain.is_empty());
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn refresh_fixed_plan(&mut self) {
        for t in 0..TIMBRE_COUNT {
            let mut mask = 0;
            for role in 0..TIMBRE_EFFECTS {
                if role == 1 && matches!(self.instances[2 * t].settings().program.kind, 29 | 30) {
                    continue;
                }
                let program = self.instances[timbre_effect_slot(t, role)]
                    .settings()
                    .program;
                if program.kind != 0 && program.enabled {
                    mask |= 1 << role;
                }
            }
            self.active_fixed[t] = mask;
        }
        let master = self.instances[MASTER_EFFECT_SLOT].settings().program;
        self.master_active = master.kind != 0 && master.enabled;
        self.refresh_active();
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn refresh_extra_plan(&mut self, t: usize) {
        let active = &mut self.active_extra[t];
        active.clear();
        // configure_chain reserves the full chain size before publication;
        // enable/bypass edits cannot allocate on this path.
        for (i, fx) in self.chains[t].iter().enumerate() {
            let program = fx.processor.settings().program;
            if program.kind != 0 && program.enabled {
                active.push(i);
            }
        }
        self.refresh_active();
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    pub fn chain_programs(&self) -> [Vec<(String, EffectAudioProgram)>; TIMBRE_COUNT] {
        core::array::from_fn(|t| {
            self.chains[t]
                .iter()
                .map(|fx| (fx.id.clone(), fx.processor.settings().program))
                .collect()
        })
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    pub fn chain_program_entries(&self) -> impl Iterator<Item = (usize, &str, EffectAudioProgram)> + '_ {
        self.chains.iter().enumerate().flat_map(|(t, chain)| {
            chain.iter().map(move |fx| (t, fx.id.as_str(), fx.processor.settings().program))
        })
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    pub fn configure_chain(
        &mut self,
        timbre: usize,
        settings: Vec<(String, EffectAudioSettings)>,
    ) -> Result<(), &'static str> {
        let chain = self.chains.get_mut(timbre).ok_or("Invalid effect timbre")?;
        // Allocate before changing the live rack. Stable IDs preserve tails on reorder.
        let mut memory = Vec::new();
        memory
            .try_reserve_exact(settings.len())
            .map_err(|_| "Not enough memory for this effect chain")?;
        for (id, setting) in &settings {
            let needed = chain.iter().find(|fx| fx.id == *id).is_none_or(|fx| {
                fx.memory.len()
                    < radias_synth_domain::effect_audio::delay_storage_words(setting.program.kind)
            });
            memory.push(if needed {
                Some(effect_memory(setting.program.kind)?)
            } else {
                None
            });
        }
        let mut next = Vec::new();
        next.try_reserve_exact(settings.len())
            .map_err(|_| "Not enough memory for this effect chain")?;
        let mut active = Vec::new();
        active
            .try_reserve_exact(settings.len())
            .map_err(|_| "Not enough memory for this effect chain")?;
        let mut old = core::mem::take(chain);
        for ((id, setting), storage) in settings.into_iter().zip(memory) {
            let mut fx = if let Some(index) = old.iter().position(|fx| fx.id == id) {
                old.swap_remove(index)
            } else {
                ChainEffect {
                    id,
                    processor: EffectAudioProcessor::new(setting),
                    memory: Vec::new().into_boxed_slice(),
                }
            };
            if let Some(storage) = storage {
                fx.memory = storage;
            }
            if fx.processor.configure(setting) {
                fx.memory.fill(0.0);
            }
            if radias_synth_domain::effect_audio::delay_storage_words(setting.program.kind) == 0 {
                fx.memory = Vec::new().into_boxed_slice();
            }
            if setting.program.kind != 0 && setting.program.enabled {
                active.push(next.len());
            }
            next.push(fx);
        }
        *chain = next;
        self.active_extra[timbre] = active;
        self.refresh_active();
        Ok(())
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    pub fn configure_chain_effect(
        &mut self,
        timbre: usize,
        id: &str,
        setting: EffectAudioSettings,
    ) -> Result<(), &'static str> {
        let fx = self
            .chains
            .get_mut(timbre)
            .and_then(|chain| chain.iter_mut().find(|fx| fx.id == id))
            .ok_or("Unknown timbre effect")?;
        let previous = fx.processor.settings().program;
        let words = radias_synth_domain::effect_audio::delay_storage_words(setting.program.kind);
        if fx.memory.len() < words {
            fx.memory = effect_memory(setting.program.kind)?;
        }
        if fx.processor.configure(setting) {
            fx.memory.fill(0.0);
        }
        if words == 0 {
            fx.memory = Vec::new().into_boxed_slice();
        }
        if previous.kind != setting.program.kind || previous.enabled != setting.program.enabled {
            self.refresh_extra_plan(timbre);
        }
        Ok(())
    }
    pub fn configure(
        &mut self,
        slot: usize,
        settings: EffectAudioSettings,
    ) -> Result<(), &'static str> {
        if slot >= EFFECT_SLOTS {
            return Err("Invalid effect slot");
        }
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        let previous = self.instances[slot].settings().program;
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        {
            if self.memory[slot].len()
                < radias_synth_domain::effect_audio::delay_storage_words(settings.program.kind)
            {
                self.memory[slot] = effect_memory(settings.program.kind)?;
            }
        }
        let processor = &mut self.instances[slot];
        if processor.configure(settings) {
            self.memory[slot].fill(0.0);
        }
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        {
            if radias_synth_domain::effect_audio::delay_storage_words(settings.program.kind) == 0 {
                self.memory[slot] = Vec::new().into_boxed_slice();
            }
            if previous.kind != settings.program.kind
                || previous.enabled != settings.program.enabled
            {
                self.refresh_fixed_plan();
            }
        }
        Ok(())
    }
    pub fn set_tempo(&mut self, tempo_tenths: u16) {
        for context in &mut self.contexts {
            context.tempo_tenths = tempo_tenths.max(1);
        }
    }
    pub fn set_controllers(&mut self, controllers: [f32; 13]) {
        let values = controllers.map(|v| {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        });
        for context in &mut self.contexts {
            context.controllers = values;
        }
    }
    pub fn set_controller(&mut self, timbre: usize, source: usize, value: f32) {
        if timbre <= TIMBRE_COUNT && source < 13 {
            self.contexts[timbre].controllers[source] = if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
    pub fn note_on(&mut self, timbre: usize, note: u8, velocity: u8) {
        if timbre >= TIMBRE_COUNT || note >= 128 {
            return;
        }
        let first = self.held[timbre].iter().all(|&n| n == 0);
        let first_master = self.held.iter().flatten().all(|&n| n == 0);
        self.held[timbre][usize::from(note)] =
            self.held[timbre][usize::from(note)].saturating_add(1);
        self.contexts[timbre].note = note;
        self.contexts[TIMBRE_COUNT].note = note;
        self.set_controller(timbre, 1, f32::from(velocity) / 127.0);
        self.set_controller(TIMBRE_COUNT, 1, f32::from(velocity) / 127.0);
        if first {
            for role in 0..TIMBRE_EFFECTS {
                self.instances[timbre_effect_slot(timbre, role)].note_on();
            }
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            for fx in &mut self.chains[timbre] {
                fx.processor.note_on();
            }
        }
        if first_master {
            self.instances[MASTER_EFFECT_SLOT].note_on();
        }
    }
    pub fn note_off(&mut self, timbre: usize, note: u8) {
        if timbre < TIMBRE_COUNT && note < 128 {
            self.held[timbre][usize::from(note)] =
                self.held[timbre][usize::from(note)].saturating_sub(1);
        }
    }
    pub fn release_notes(&mut self, timbre: Option<usize>) {
        if let Some(timbre) = timbre {
            if timbre < TIMBRE_COUNT {
                self.held[timbre].fill(0);
            }
        } else {
            self.held = [[0; 128]; TIMBRE_COUNT];
        }
    }
    pub fn process(&mut self, buses: [StereoFrame; TIMBRE_COUNT]) -> StereoFrame {
        let mut left = 0i64;
        let mut right = 0i64;
        for (timbre, input) in buses.into_iter().enumerate() {
            let mut sample = input;
            #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
            for role in 0..TIMBRE_EFFECTS {
                if role == 1
                    && matches!(self.instances[2 * timbre].settings().program.kind, 29 | 30)
                {
                    continue;
                }
                let slot = timbre_effect_slot(timbre, role);
                sample = self.instances[slot]
                    .process(sample, &mut self.memory[slot], &self.contexts[timbre])
                    .expect("Constructed effect storage and type");
            }
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            {
                let mut mask = self.active_fixed[timbre];
                while mask != 0 {
                    let role = mask.trailing_zeros() as usize;
                    mask &= mask - 1;
                    let slot = timbre_effect_slot(timbre, role);
                    sample = self.instances[slot]
                        .process_compact(sample, &mut self.memory[slot], &self.contexts[timbre])
                        .expect("Constructed effect storage and type");
                }
                for &index in &self.active_extra[timbre] {
                    let fx = &mut self.chains[timbre][index];
                    sample = fx
                        .processor
                        .process_compact(sample, &mut fx.memory, &self.contexts[timbre])
                        .expect("Constructed timbre effect storage");
                }
            }
            left += i64::from(sample.left.0);
            right += i64::from(sample.right.0);
        }
        let sample = StereoFrame {
            left: Sample(crate_saturate(left)),
            right: Sample(crate_saturate(right)),
        };
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        {
            if !self.master_active {
                return sample;
            }
            self.instances[MASTER_EFFECT_SLOT]
                .process_compact(
                    sample,
                    &mut self.memory[MASTER_EFFECT_SLOT],
                    &self.contexts[TIMBRE_COUNT],
                )
                .expect("Constructed Master effect storage and type")
        }
        #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
        {
            self.instances[MASTER_EFFECT_SLOT]
                .process(
                    sample,
                    &mut self.memory[MASTER_EFFECT_SLOT],
                    &self.contexts[TIMBRE_COUNT],
                )
                .expect("Constructed Master effect storage and type")
        }
    }
    pub fn delay_storage_bytes(&self) -> usize {
        let bytes = self.memory.iter().map(|memory| memory.len()).sum::<usize>()
            * core::mem::size_of::<f32>();
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        let bytes = bytes
            + self
                .chains
                .iter()
                .flatten()
                .map(|fx| fx.memory.len() * core::mem::size_of::<f32>())
                .sum::<usize>();
        bytes
    }
}
fn crate_saturate(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
