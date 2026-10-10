//! Allocation and routing use case for the audible native reconstruction.
//! Two serial Insert FX per timbre, then the stereo Master FX. This is a
//! native audition topology; original FXD03 bus/arithmetic parity is unproven.
use alloc::{boxed::Box, vec};
use radias_synth_domain::{
    Sample,
    effect_audio::{
        DELAY_WORDS, EffectAudioContext, EffectAudioProcessor, EffectAudioProgram,
        EffectAudioSettings,
    },
    pan::StereoFrame,
};

pub const EFFECT_SLOTS: usize = 9;
pub struct EffectAudioRack {
    instances: [EffectAudioProcessor; EFFECT_SLOTS],
    memory: [Box<[f32]>; EFFECT_SLOTS],
    contexts: [EffectAudioContext; 5],
    held: [[u8; 128]; 4],
}
impl EffectAudioRack {
    /// All delay allocation occurs on the caller's control thread.
    pub fn new(settings: [EffectAudioSettings; EFFECT_SLOTS]) -> Self {
        Self {
            instances: settings.map(EffectAudioProcessor::new),
            memory: core::array::from_fn(|_| vec![0.0; DELAY_WORDS].into_boxed_slice()),
            contexts: [Default::default(); 5],
            held: [[0; 128]; 4],
        }
    }
    pub fn programs(&self) -> [EffectAudioProgram; EFFECT_SLOTS] {
        core::array::from_fn(|i| self.instances[i].settings().program)
    }
    pub fn configure(
        &mut self,
        slot: usize,
        settings: EffectAudioSettings,
    ) -> Result<(), &'static str> {
        let processor = self.instances.get_mut(slot).ok_or("Invalid effect slot")?;
        if processor.configure(settings) {
            self.memory[slot].fill(0.0);
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
        if timbre < 5 && source < 13 {
            self.contexts[timbre].controllers[source] = if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
    pub fn note_on(&mut self, timbre: usize, note: u8, velocity: u8) {
        if timbre >= 4 || note >= 128 {
            return;
        }
        let first = self.held[timbre].iter().all(|&n| n == 0);
        let first_master = self.held.iter().flatten().all(|&n| n == 0);
        self.held[timbre][usize::from(note)] =
            self.held[timbre][usize::from(note)].saturating_add(1);
        self.contexts[timbre].note = note;
        self.contexts[4].note = note;
        self.set_controller(timbre, 1, f32::from(velocity) / 127.0);
        self.set_controller(4, 1, f32::from(velocity) / 127.0);
        if first {
            self.instances[2 * timbre].note_on();
            self.instances[2 * timbre + 1].note_on();
        }
        if first_master {
            self.instances[8].note_on();
        }
    }
    pub fn note_off(&mut self, timbre: usize, note: u8) {
        if timbre < 4 && note < 128 {
            self.held[timbre][usize::from(note)] =
                self.held[timbre][usize::from(note)].saturating_sub(1);
        }
    }
    pub fn release_notes(&mut self, timbre: Option<usize>) {
        if let Some(timbre) = timbre {
            if timbre < 4 {
                self.held[timbre].fill(0);
            }
        } else {
            self.held = [[0; 128]; 4];
        }
    }
    pub fn process(&mut self, buses: [StereoFrame; 4]) -> StereoFrame {
        let mut left = 0i64;
        let mut right = 0i64;
        for (timbre, input) in buses.into_iter().enumerate() {
            let mut sample = input;
            for role in 0..2 {
                if role == 1
                    && matches!(self.instances[2 * timbre].settings().program.kind, 29 | 30)
                {
                    break;
                }
                let slot = 2 * timbre + role;
                sample = self.instances[slot]
                    .process(sample, &mut self.memory[slot], &self.contexts[timbre])
                    .expect("Constructed effect storage and type");
            }
            left += i64::from(sample.left.0);
            right += i64::from(sample.right.0);
        }
        let sample = StereoFrame {
            left: Sample(crate_saturate(left)),
            right: Sample(crate_saturate(right)),
        };
        self.instances[8]
            .process(sample, &mut self.memory[8], &self.contexts[4])
            .expect("Constructed Master effect storage and type")
    }
    pub fn delay_storage_bytes(&self) -> usize {
        EFFECT_SLOTS * DELAY_WORDS * core::mem::size_of::<f32>()
    }
}
fn crate_saturate(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
