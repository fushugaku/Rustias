use radias_synth_application::motion::{Lane, Program};
use radias_synth_domain::modulation::AppliedModulationTargets;
use std::sync::Arc;

#[derive(Default, Clone)]
struct Clock {
    step: usize,
    direction: bool,
    remaining: f64,
    duration: f64,
    cycle: u64,
    accepted: bool,
    done: bool,
    started: bool,
    seed: u32,
    held_value: f64,
}
pub struct VoiceMotion {
    pub program: Arc<Program>,
    clocks: [Clock; 6],
    previous_frame: u64,
    dirty: bool,
    pub seed: u32,
    pub positions: [i16; 6],
    pub offsets: [f64; 207],
}
impl Default for VoiceMotion {
    fn default() -> Self {
        Self {
            program: Arc::default(),
            clocks: Default::default(),
            previous_frame: 0,
            dirty: true,
            seed: 1,
            offsets: [0.0; 207],
            positions: [-1; 6],
        }
    }
}
pub fn supported(id: usize) -> bool {
    matches!(id, 1..=8 | 11 | 12 | 15..=19 | 22 | 23 | 25..=28 | 31..=51 | 53 | 54 | 59 | 75 | 83 | 165 | 171..=206)
}
fn next_step(lane: &Lane, clock: &mut Clock, random: u32) -> usize {
    if lane.start == lane.end {
        if lane.one_shot {
            clock.done = true;
        }
        return lane.start;
    }
    if lane.direction == 4 {
        if lane.one_shot && clock.cycle >= (lane.end - lane.start) as u64 {
            clock.done = true;
            return clock.step;
        }
        return lane.start + random as usize % (lane.end - lane.start + 1);
    }
    let reverse = lane.direction == 1 || lane.direction >= 2 && clock.direction;
    if reverse {
        if clock.step > lane.start {
            clock.step - 1
        } else {
            if lane.one_shot {
                clock.done = true;
                return lane.start;
            }
            if lane.direction >= 2 {
                clock.direction = false;
                if lane.direction == 2 {
                    (lane.start + 1).min(lane.end)
                } else {
                    lane.start
                }
            } else {
                lane.end
            }
        }
    } else if clock.step < lane.end {
        clock.step + 1
    } else {
        if lane.direction >= 2 {
            clock.direction = true;
            if lane.direction == 2 {
                lane.end.saturating_sub(1).max(lane.start)
            } else {
                lane.end
            }
        } else {
            if lane.one_shot {
                clock.done = true;
                return lane.end;
            }
            lane.start
        }
    }
}
fn random(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}
impl VoiceMotion {
    pub fn configure(&mut self, program: Arc<Program>) {
        let previous = self.clocks.clone();
        self.clocks = core::array::from_fn(|i| {
            program
                .lanes
                .get(i)
                .and_then(|lane| self.program.lanes.iter().position(|old| old.id == lane.id))
                .map_or_else(Clock::default, |j| previous[j].clone())
        });
        for (lane, clock) in program.lanes.iter().zip(&mut self.clocks) {
            if clock.started {
                let fraction = (clock.remaining / clock.duration).clamp(0.0, 1.0);
                clock.step = clock.step.clamp(lane.start, lane.end);
                clock.duration = lane.beats * lane.durations[clock.step];
                clock.remaining = clock.duration * fraction;
                if clock.accepted {
                    clock.held_value = lane.values[clock.step];
                }
                if !lane.one_shot {
                    clock.done = false;
                }
            }
        }
        self.program = program;
        self.dirty = true;
        if self.program.lanes.is_empty() {
            self.offsets.fill(0.0);
        }
        self.positions.fill(-1);
    }
    pub fn tick(
        &mut self,
        frame: u64,
        tempo: f64,
        targets: AppliedModulationTargets,
    ) -> AppliedModulationTargets {
        if !self.dirty && frame == self.previous_frame {
            return self.apply(targets);
        }
        self.dirty = false;
        if self.program.lanes.is_empty() {
            self.previous_frame = frame;
            return targets;
        }
        self.offsets.fill(0.0);
        self.positions.fill(-1);
        let elapsed = frame.wrapping_sub(self.previous_frame) as f64 / 48_000.0 * tempo / 60.0;
        self.previous_frame = frame;
        for (lane, clock) in self.program.lanes.iter().zip(&mut self.clocks) {
            if !clock.started {
                clock.seed =
                    (self.seed ^ ((lane.index as u32 + 1).wrapping_mul(2654435761))).max(1);
                clock.step = if lane.direction == 1 {
                    lane.end
                } else {
                    lane.start
                };
                clock.started = true;
                clock.duration = lane.beats * lane.durations[clock.step];
                clock.remaining = clock.duration;
                clock.accepted = ((random(&mut clock.seed) % 10_000) as f64)
                    < lane.probabilities[clock.step] * 100.0;
                if clock.accepted {
                    clock.held_value = lane.values[clock.step];
                }
            }
            clock.remaining -= elapsed;
            for _ in 0..1024 {
                if clock.remaining > 0.0 || clock.done {
                    break;
                }
                let next_random = random(&mut clock.seed);
                clock.step = next_step(lane, clock, next_random);
                clock.step = clock.step.clamp(lane.start, lane.end);
                clock.cycle = clock.cycle.wrapping_add(1);
                clock.duration = lane.beats * lane.durations[clock.step];
                clock.remaining += clock.duration;
                clock.accepted = ((random(&mut clock.seed) % 10_000) as f64)
                    < lane.probabilities[clock.step] * 100.0;
                if clock.accepted {
                    clock.held_value = lane.values[clock.step];
                }
            }
            let mut value = clock.held_value;
            let curve = lane.curves[clock.step];
            if !clock.done && clock.accepted && (lane.slide || curve > 0) && curve != 4 {
                let mut peek = clock.clone();
                let mut next_seed = clock.seed;
                let next = next_step(lane, &mut peek, random(&mut next_seed));
                let f = (1.0 - clock.remaining / clock.duration).clamp(0.0, 1.0);
                let f = match curve {
                    2 => f * f,
                    3 => 1.0 - (1.0 - f) * (1.0 - f),
                    _ => f,
                };
                value += (lane.values[next] - value) * f;
            }
            self.offsets[lane.parameter] = value * lane.amount / 100.0;
            self.positions[lane.index] = clock.step as i16;
        }
        self.apply(targets)
    }
    pub fn apply(&self, mut targets: AppliedModulationTargets) -> AppliedModulationTargets {
        let add = |t: &mut AppliedModulationTargets, dest: usize, value: f64, scale: f64| {
            let x = &mut t.controls[dest - 2];
            *x = (*x as f64 + value * scale).round().clamp(-32767.0, 32767.0) as i16;
        };
        for (id, offset) in self
            .offsets
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, x)| *x != 0.0)
        {
            match id {
                54 => {
                    targets.oscillator_pitch_q16[0] = targets.oscillator_pitch_q16[0]
                        .saturating_add((offset * 65536.0 / 100.0) as i32)
                }
                53 => {
                    targets.oscillator_pitch_q16[0] =
                        targets.oscillator_pitch_q16[0].saturating_add((offset * 65536.0) as i32)
                }
                15 => {
                    targets.oscillator_pitch_q16[1] =
                        targets.oscillator_pitch_q16[1].saturating_add((offset * 65536.0) as i32)
                }
                16 => {
                    targets.oscillator_pitch_q16[1] = targets.oscillator_pitch_q16[1]
                        .saturating_add((offset * 65536.0 / 64.0) as i32)
                }
                1 => add(&mut targets, 7, offset, 128.0),
                2 => add(&mut targets, 8, offset, 1.0),
                7 => add(&mut targets, 11, offset, 128.0),
                8 => add(&mut targets, 12, offset, 128.0),
                11 => add(&mut targets, 2, offset, 128.0),
                12 => add(&mut targets, 16, offset, 1.0),
                17..=19 => add(&mut targets, 3 + id - 17, offset, 128.0),
                22 => add(&mut targets, 9, offset, 128.0),
                23 => add(&mut targets, 19, offset, 1.0),
                25 => add(&mut targets, 17, offset, 1.0),
                26 => add(&mut targets, 18, offset, 1.0),
                27 => add(&mut targets, 20, offset, 1.0),
                28 => add(&mut targets, 21, offset, 1.0),
                31 => add(&mut targets, 10, offset, 128.0),
                59 => add(&mut targets, 15, offset, 1.0),
                75 => add(&mut targets, 13, offset, 128.0),
                83 => add(&mut targets, 14, offset, 128.0),
                165 => add(&mut targets, 42, offset, 1.0),
                171..=178 => {
                    let dest = [46, 47, 44, 45, 48, 49, 50, 51][id - 171];
                    add(
                        &mut targets,
                        dest,
                        offset,
                        if id == 173 { 128.0 } else { 1.0 },
                    );
                }
                _ => {}
            }
        }
        targets
    }
}
