//! Firmware-free Q-style envelope modes. Stage behavior follows the Q manual
//! (pp. 122–127); rates/curves are Rustias models, not calibrated hardware data.
//! The existing 2 kHz controller feeds the normal per-sample DSP gain smoothing.
use crate::amp_envelope::AmpEnvelopeParameters;
use crate::envelope_segment::EnvelopeTimingTables;

pub const PARAMETER_BASE: usize = 195;
pub const PARAMETER_COUNT: usize = PARAMETER_BASE + 3 * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// 0 RADIAS, 1 ADSR, 2 ADS1DS2R, 3 One Shot, 4 Loop S1S2, 5 Loop All.
    pub mode: u8,
    pub attack_level: u8,
    pub decay2: u8,
    pub sustain2: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: 0,
            attack_level: 127,
            decay2: 64,
            sustain2: 64,
        }
    }
}
impl Settings {
    pub fn from_controls(mut get: impl FnMut(usize) -> u8, envelope: usize) -> Self {
        let b = PARAMETER_BASE + envelope * 4;
        Self {
            mode: get(b).min(5),
            attack_level: get(b + 1),
            decay2: get(b + 2),
            sustain2: get(b + 3),
        }
    }
    pub fn extended(self) -> bool {
        matches!(self.mode, 2 | 4 | 5)
    }
}

/// Controller ticks: zero really is immediate, then 0.5 ms…60 s. The table
/// keeps transcendental work out of note/parameter/render callbacks.
pub const TIME_TICKS: [u32; 128] = [
    0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 13,
    15, 16, 18, 19, 21, 23, 26, 28, 31, 34, 37, 41, 45, 49, 54, 59, 65, 71, 78, 86, 94, 104, 114,
    125, 137, 150, 165, 181, 198, 218, 239, 262, 288, 316, 346, 380, 417, 458, 502, 551, 605, 663,
    728, 799, 876, 962, 1055, 1158, 1270, 1394, 1530, 1678, 1842, 2021, 2217, 2433, 2670, 2929,
    3214, 3527, 3870, 4246, 4659, 5112, 5609, 6155, 6754, 7411, 8131, 8922, 9790, 10742, 11787,
    12934, 14192, 15572, 17086, 18748, 20572, 22573, 24768, 27177, 29821, 32721, 35904, 39396,
    43228, 47432, 52045, 57108, 62662, 68757, 75444, 82782, 90834, 99669, 109363, 120000,
];
pub fn seconds(value: u8) -> f64 {
    TIME_TICKS[(value & 127) as usize] as f64 / 2000.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    Attack,
    Decay,
    Decay2,
    Sustain,
    Release,
    #[default]
    Finished,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Envelope {
    pub stage: Stage,
    pub level: u16,
    pub settings: Settings,
    held: bool,
    start: u16,
    target: u16,
    elapsed: u32,
    duration: u32,
}
impl Envelope {
    fn expanded(value: u8) -> u16 {
        (u32::from(value & 127) * 32766 / 127) as u16
    }
    fn target(&self, p: AmpEnvelopeParameters) -> u16 {
        Self::expanded(match self.stage {
            Stage::Attack => {
                if self.settings.extended() {
                    self.settings.attack_level
                } else {
                    127
                }
            }
            Stage::Decay => p.sustain,
            Stage::Decay2 => self.settings.sustain2,
            Stage::Sustain => {
                if self.settings.extended() {
                    self.settings.sustain2
                } else {
                    p.sustain
                }
            }
            _ => 0,
        })
    }
    fn duration(&self, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) -> u32 {
        let rate = match self.stage {
            Stage::Attack => p.attack,
            Stage::Decay => p.decay,
            Stage::Decay2 => self.settings.decay2,
            Stage::Release => p.release,
            _ => return 0,
        };
        let ticks = TIME_TICKS[(rate & 127) as usize];
        if ticks == 0 {
            return 0;
        }
        let velocity =
            ((i32::from(p.velocity_sensitivity & 127) - 64) * (i32::from(p.velocity) - 64)) >> 6;
        let key = (i32::from(tables.key_tracking[(p.key_tracking & 127) as usize])
            * (i32::from(p.note) - 60))
            >> 14;
        let factor = (u64::from(tables.scale[(velocity.clamp(-63, 63) + 64) as usize])
            * u64::from(tables.scale[(key.clamp(-63, 63) + 64) as usize]))
            >> 8;
        ((u64::from(ticks) * 256 + factor.max(1) / 2) / factor.max(1)).clamp(1, 16_000_000) as u32
    }
    fn begin(&mut self, stage: Stage, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        self.stage = stage;
        self.start = self.level;
        self.elapsed = 0;
        self.target = self.target(p);
        self.duration = self.duration(p, tables);
        if matches!(stage, Stage::Sustain | Stage::Finished) {
            self.level = self.target;
        }
    }
    fn advance_stage(&mut self, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        let next = match self.stage {
            Stage::Attack => Stage::Decay,
            Stage::Decay if self.settings.extended() => Stage::Decay2,
            Stage::Decay if self.settings.mode == 3 => Stage::Release,
            Stage::Decay => Stage::Sustain,
            Stage::Decay2 if self.settings.mode == 4 && self.held => Stage::Decay,
            Stage::Decay2 if self.settings.mode == 5 => Stage::Release,
            Stage::Decay2 => Stage::Sustain,
            Stage::Release if self.settings.mode == 5 && self.held => Stage::Attack,
            _ => Stage::Finished,
        };
        self.begin(next, p, tables);
    }
    fn settle(&mut self, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        // All-zero looping envelopes must never spin indefinitely on the audio
        // thread. At most one complete cycle is resolved per controller tick.
        for _ in 0..5 {
            if self.duration != 0 || matches!(self.stage, Stage::Sustain | Stage::Finished) {
                break;
            }
            self.level = self.target;
            self.advance_stage(p, tables);
        }
    }
    pub fn note_on(
        &mut self,
        settings: Settings,
        p: AmpEnvelopeParameters,
        tables: &EnvelopeTimingTables,
    ) {
        *self = Self {
            settings,
            held: true,
            ..Self::default()
        };
        self.begin(Stage::Attack, p, tables);
        self.settle(p, tables);
    }
    pub fn adopt(
        &mut self,
        settings: Settings,
        level: u16,
        held: bool,
        p: AmpEnvelopeParameters,
        tables: &EnvelopeTimingTables,
    ) {
        *self = Self {
            settings,
            level,
            held,
            ..Self::default()
        };
        self.begin(if held { Stage::Attack } else { Stage::Release }, p, tables);
        self.settle(p, tables);
    }
    pub fn release(&mut self, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        self.held = false;
        // One Shot always completes Attack/Decay/Release, including short gates.
        if self.settings.mode == 3 || matches!(self.stage, Stage::Finished | Stage::Release) {
            return;
        }
        self.begin(Stage::Release, p, tables);
        self.settle(p, tables);
    }
    pub fn edit(
        &mut self,
        previous: AmpEnvelopeParameters,
        next: AmpEnvelopeParameters,
        settings: Settings,
        tables: &EnvelopeTimingTables,
    ) {
        let old_target = self.target(previous);
        let old_duration = self.duration(previous, tables);
        self.settings = settings;
        let target = self.target(next);
        let duration = self.duration(next, tables);
        if self.stage == Stage::Sustain {
            self.level = target;
            self.target = target;
            return;
        }
        if target != old_target {
            // Retarget from the current value; editing a breakpoint never
            // reconstructs an earlier part of the segment or resets the voice.
            self.start = self.level;
            self.target = target;
            self.duration = if self.duration == 0 {
                duration
            } else {
                (u64::from(duration) * u64::from(self.duration.saturating_sub(self.elapsed))
                    / u64::from(self.duration)) as u32
            };
            self.elapsed = 0;
        } else if duration != old_duration {
            self.elapsed = if self.duration == 0 {
                0
            } else {
                (u64::from(self.elapsed) * u64::from(duration) / u64::from(self.duration)) as u32
            };
            self.duration = duration;
        }
        self.settle(next, tables);
    }
    pub fn tick(&mut self, p: AmpEnvelopeParameters, tables: &EnvelopeTimingTables) {
        if matches!(self.stage, Stage::Sustain | Stage::Finished) {
            return;
        }
        if self.duration == 0 {
            self.settle(p, tables);
            return;
        }
        self.elapsed = self.elapsed.saturating_add(1).min(self.duration);
        let linear = (u64::from(self.elapsed) << 24) / u64::from(self.duration);
        // Linear attack, fast initial decay/release with a smooth cubic tail.
        // This is independent of the RADIAS curve selector and its divider.
        let shaped = if self.stage == Stage::Attack {
            linear
        } else {
            let inverse = (1 << 24) - linear;
            (1 << 24) - (((inverse * inverse) >> 24) * inverse >> 24)
        };
        let delta = i64::from(self.target) - i64::from(self.start);
        self.level =
            (i64::from(self.start) + ((delta * shaped as i64) >> 24)).clamp(0, 32766) as u16;
        if self.elapsed == self.duration {
            self.level = self.target;
            self.advance_stage(p, tables);
            self.settle(p, tables);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controls() -> AmpEnvelopeParameters {
        AmpEnvelopeParameters {
            attack: 0,
            decay: 45,
            sustain: 90,
            release: 24,
            curve: 3,
            velocity_sensitivity: 64,
            key_tracking: 64,
            velocity: 100,
            note: 60,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            q: Default::default(),
        }
    }
    fn tables() -> EnvelopeTimingTables {
        EnvelopeTimingTables {
            increments: [[1; 128]; 8],
            scale: [256; 128],
            key_tracking: [0; 128],
        }
    }
    fn run(e: &mut Envelope, p: AmpEnvelopeParameters, ticks: usize) {
        for _ in 0..ticks {
            e.tick(p, &tables());
            assert!(e.level <= 32766);
        }
    }
    #[test]
    fn instant_attack_and_fast_decay_have_no_radias_divider_delay() {
        let p = controls();
        let mut e = Envelope::default();
        e.note_on(
            Settings {
                mode: 1,
                ..Default::default()
            },
            p,
            &tables(),
        );
        assert_eq!(e.level, 32766);
        assert_eq!(e.stage, Stage::Decay);
        let duration = TIME_TICKS[p.decay as usize];
        run(&mut e, p, (duration / 2) as usize);
        // More than half the drop happens in the first half of Decay.
        assert!(e.level < (32766 + Envelope::expanded(p.sustain)) / 2);
        assert!(TIME_TICKS[12] < 6); // sub-3 ms low Attack values
    }
    #[test]
    fn extended_envelope_holds_sustain_two_and_releases_from_current_level() {
        let p = controls();
        let mut e = Envelope::default();
        e.note_on(
            Settings {
                mode: 2,
                attack_level: 70,
                decay2: 30,
                sustain2: 24,
            },
            p,
            &tables(),
        );
        assert_eq!(e.level, Envelope::expanded(70));
        run(&mut e, p, 1000);
        assert_eq!(e.stage, Stage::Sustain);
        assert_eq!(e.level, Envelope::expanded(24));
        let before = e.level;
        e.release(p, &tables());
        assert_eq!(e.level, before);
        run(&mut e, p, 1000);
        assert_eq!(e.stage, Stage::Finished);
        assert_eq!(e.level, 0);
    }
    #[test]
    fn one_shot_ignores_even_a_note_off_during_attack() {
        let p = AmpEnvelopeParameters {
            attack: 40,
            ..controls()
        };
        let mut held = Envelope::default();
        held.note_on(
            Settings {
                mode: 3,
                ..Default::default()
            },
            p,
            &tables(),
        );
        let mut released = held;
        released.release(p, &tables());
        for _ in 0..2000 {
            held.tick(p, &tables());
            released.tick(p, &tables());
            assert_eq!(held.level, released.level);
            assert_eq!(held.stage, released.stage);
        }
        assert_eq!(held.stage, Stage::Finished);
    }
    #[test]
    fn loops_follow_their_boundaries_and_stop_on_note_off() {
        let p = AmpEnvelopeParameters {
            attack: 12,
            ..controls()
        };
        for mode in [4, 5] {
            let settings = Settings {
                mode,
                attack_level: 127,
                decay2: 30,
                sustain2: 24,
            };
            let mut e = Envelope::default();
            e.note_on(settings, p, &tables());
            run(&mut e, p, 128);
            let mut attacks = 0;
            let mut lows = 0;
            let mut previous = e.stage;
            for _ in 0..4000 {
                e.tick(p, &tables());
                assert_ne!(e.stage, Stage::Finished);
                if e.stage != previous {
                    if e.stage == Stage::Attack {
                        attacks += 1;
                    }
                    if e.stage == Stage::Decay2 {
                        lows += 1;
                    }
                }
                if mode == 4 {
                    assert!(e.level >= Envelope::expanded(settings.sustain2));
                }
                previous = e.stage;
            }
            assert!(lows > 5);
            assert_eq!(attacks > 0, mode == 5);
            e.release(p, &tables());
            run(&mut e, p, 1000);
            assert_eq!(e.stage, Stage::Finished);
        }
    }
    #[test]
    fn zero_time_loops_are_bounded_and_do_not_leak_after_release() {
        let p = AmpEnvelopeParameters {
            decay: 0,
            release: 0,
            ..controls()
        };
        for mode in [1, 2, 3, 4, 5] {
            let mut e = Envelope::default();
            e.note_on(
                Settings {
                    mode,
                    decay2: 0,
                    ..Default::default()
                },
                p,
                &tables(),
            );
            run(&mut e, p, 100);
            e.release(p, &tables());
            run(&mut e, p, 10);
            assert_eq!(e.stage, Stage::Finished);
            assert_eq!(e.level, 0);
        }
    }
    #[test]
    fn edits_keep_the_current_level_and_do_not_restart_attack() {
        let p = controls();
        let mut e = Envelope::default();
        let settings = Settings {
            mode: 2,
            ..Default::default()
        };
        e.note_on(settings, p, &tables());
        run(&mut e, p, 3);
        let before = e.level;
        let next = AmpEnvelopeParameters {
            decay: 55,
            sustain: 40,
            ..p
        };
        e.edit(p, next, settings, &tables());
        assert_eq!(e.level, before);
        assert_eq!(e.stage, Stage::Decay);
        run(&mut e, next, 1000);
        assert_eq!(e.stage, Stage::Sustain);
    }
}
