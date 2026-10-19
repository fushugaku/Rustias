//! Plugin-only modulation clock and temporary parameter automation.
//! The browser keeps its note sequencer; a DAW supplies all native plugin notes.
use super::super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const DIVISIONS: &[&str] = &[
    "1/48", "1/32", "1/24", "3/64", "1/16", "1/12", "3/32", "1/8", "1/6", "3/16", "1/4", "1/3",
    "3/8", "1/2", "2/3", "3/4", "1/1",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum Target {
    Synth {
        timbre: usize,
        parameter: u32,
    },
    Global {
        parameter: u32,
    },
    Drum {
        instrument: usize,
        parameter: u32,
    },
    Sample {
        timbre: usize,
        source: String,
        parameter: u32,
    },
    Effect {
        slot: usize,
        #[serde(rename = "effectKind")]
        effect_kind: u32,
        parameter: usize,
    },
    ChainEffect {
        timbre: usize,
        id: String,
        #[serde(rename = "effectKind")]
        effect_kind: u32,
        parameter: usize,
    },
    Module {
        timbre: usize,
        node: u32,
        #[serde(rename = "moduleKind")]
        module_kind: String,
        control: String,
    },
    KitGain,
    Volume,
    Macro {
        #[serde(rename = "macro")]
        index: usize,
    },
    // Kept for importing browser programs, but never drives DAW note lengths.
    SequenceGate {
        timbre: usize,
    },
}
impl Target {
    fn valid(&self) -> bool {
        match self {
            Self::Synth { timbre, parameter } => {
                *timbre < TIMBRE_COUNT && *parameter < PARAMETER_COUNT as u32
            }
            Self::Global { parameter } => *parameter < PARAMETER_COUNT as u32,
            Self::Drum {
                instrument,
                parameter,
            } => *instrument < 16 && *parameter < PARAMETER_COUNT as u32,
            Self::Sample {
                timbre,
                source,
                parameter,
            } => {
                *timbre < TIMBRE_COUNT && source.len() <= 200 && *parameter < PARAMETER_COUNT as u32
            }
            Self::Effect {
                slot,
                effect_kind,
                parameter,
            } => *slot < EFFECT_SLOTS && (1..=30).contains(effect_kind) && *parameter < 20,
            Self::ChainEffect {
                timbre,
                id,
                effect_kind,
                parameter,
            } => {
                *timbre < TIMBRE_COUNT
                    && valid_id(id)
                    && (1..=30).contains(effect_kind)
                    && *parameter < 20
            }
            Self::Module {
                timbre,
                node,
                module_kind,
                control,
            } => *timbre < TIMBRE_COUNT && *node < 64 && valid_id(module_kind) && valid_id(control),
            Self::Macro { index } => *index < 8,
            Self::SequenceGate { timbre } => *timbre < TIMBRE_COUNT,
            Self::KitGain | Self::Volume => true,
        }
    }
    fn range(&self) -> i32 {
        match self {
            Self::Macro { .. } | Self::SequenceGate { .. } => 100,
            Self::Synth {
                parameter: 53 | 15 | 175 | 183,
                ..
            }
            | Self::Drum {
                parameter: 53 | 15 | 175 | 183,
                ..
            }
            | Self::Sample {
                parameter: 53 | 15 | 175 | 183,
                ..
            } => 24,
            _ => 63,
        }
    }
    fn wave(&self) -> bool {
        matches!(
            self,
            Self::Synth {
                parameter: 0 | 171 | 179,
                ..
            } | Self::Global {
                parameter: 0 | 171 | 179
            } | Self::Drum {
                parameter: 0 | 171 | 179,
                ..
            } | Self::Sample {
                parameter: 0 | 171 | 179,
                ..
            }
        )
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
enum Motion {
    #[default]
    Step,
    Slide,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
enum Direction {
    #[default]
    Forward,
    Reverse,
    Alt1,
    Alt2,
    Random,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
enum RunMode {
    #[default]
    Loop,
    OneShot,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
enum KeySync {
    #[default]
    Off,
    Timbre,
    Voice,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lane {
    #[serde(default)]
    playback: Option<String>,
    id: String,
    enabled: bool,
    target: Option<Target>,
    amount: i32,
    length: usize,
    resolution: String,
    motion: Motion,
    direction: Direction,
    run_mode: RunMode,
    #[serde(default)]
    key_sync: KeySync,
    values: Vec<i32>,
    #[serde(skip)]
    phase: f64,
    #[serde(skip)]
    beats: f64,
    #[serde(skip)]
    position: Option<usize>,
    #[serde(skip)]
    target_index: Option<usize>,
}
impl Lane {
    fn cycle(&self) -> usize {
        match self.direction {
            Direction::Alt1 => (2 * self.length).saturating_sub(2).max(1),
            Direction::Alt2 => 2 * self.length,
            _ => self.length,
        }
    }
    fn step(&self, mut tick: f64) -> usize {
        let cycle = self.cycle();
        if self.run_mode == RunMode::OneShot {
            let last = if self.direction == Direction::Alt1 && self.length > 1 {
                cycle
            } else {
                cycle - 1
            };
            tick = tick.min(last as f64);
            if self.direction == Direction::Alt1 && tick == cycle as f64 {
                return 0;
            }
        }
        let i = tick.rem_euclid(cycle as f64) as usize;
        match self.direction {
            Direction::Reverse => self.length - 1 - i,
            Direction::Alt1 if i >= self.length => 2 * self.length - 2 - i,
            Direction::Alt2 if i >= self.length => 2 * self.length - 1 - i,
            _ => i,
        }
    }
    fn value(&mut self, frames: usize, tempo: f64) -> f64 {
        let tick = (self.phase + 1e-10).floor();
        let index = self.step(tick);
        let mut value = self.values[index] as f64;
        if self.motion == Motion::Slide {
            value +=
                (self.values[self.step(tick + 1.)] as f64 - value) * (self.phase - tick).max(0.);
        }
        self.position = Some(index);
        self.phase += frames as f64 / (48000. * 60. / tempo * self.beats);
        value * self.amount as f64 / 100.
    }
}
#[derive(Default, Deserialize)]
struct Config {
    version: u32,
    tracks: Vec<Vec<Lane>>,
}
#[derive(Deserialize)]
struct Binding {
    target: Target,
    amount: i32,
}
#[derive(Deserialize)]
struct Knob {
    value: f64,
    bindings: Vec<Binding>,
}
#[derive(Deserialize)]
struct Base {
    target: Target,
    value: f64,
}
#[derive(Default, Deserialize)]
struct Macros {
    knobs: Vec<Knob>,
    bases: Vec<Base>,
}
fn one() -> f64 {
    1.
}
fn yes() -> bool {
    true
}
#[derive(Deserialize)]
struct Spec {
    target: Target,
    min: f64,
    max: f64,
    base: f64,
    #[serde(default = "one")]
    step: f64,
    #[serde(default)]
    values: Option<Vec<f64>>,
    #[serde(default = "yes")]
    available: bool,
    #[serde(skip)]
    macro_base: Option<f64>,
    #[serde(skip)]
    output: Option<f64>,
    #[serde(skip)]
    offset: Option<f64>,
    #[serde(skip)]
    macro_active: bool,
}
impl Spec {
    fn bounded(&self, value: f64) -> f64 {
        let value = value.clamp(self.min, self.max);
        if let Some(values) = &self.values {
            // Match JS reduce's stable tie handling and Math.round toward +inf.
            return values
                .iter()
                .copied()
                .reduce(|a, b| {
                    if (b - value).abs() < (a - value).abs() {
                        b
                    } else {
                        a
                    }
                })
                .unwrap_or(value);
        }
        (((value / self.step + 0.5).floor() * self.step) * 1e8).round() / 1e8
    }
}
#[derive(Deserialize)]
struct State {
    config: Config,
    macros: Macros,
    targets: Vec<Spec>,
    #[serde(default)]
    effects: Value,
    #[serde(default)]
    circuits: Vec<Value>,
}

// Prepared at topology edits, never serialized by a modulation quantum. Keep
// invalid parameter entries too: a later edit may repair them, just as the JSON
// boundary did. Headers, integer conversion and length limits retain that ABI.
struct MotionEffect {
    kind: Option<u64>,
    header: Option<effects::Program>,
    parameters: Vec<Option<u8>>,
    parameter_bytes: Vec<usize>,
    bytes: usize,
}
impl MotionEffect {
    fn new(raw: Value) -> Self {
        let kind = raw.get("kind").and_then(Value::as_u64);
        let bytes = serde_json::to_vec(&raw).map_or(usize::MAX, |b| b.len());
        let values = raw.get("parameters").and_then(Value::as_array);
        let parameters = values
            .map(|ps| {
                ps.iter()
                    .map(|p| serde_json::from_value::<u8>(p.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        let parameter_bytes = values
            .map(|ps| {
                ps.iter()
                    .map(|p| serde_json::to_vec(p).unwrap().len())
                    .collect()
            })
            .unwrap_or_default();
        let mut header = raw;
        if let Some(object) = header.as_object_mut() {
            object.insert("parameters".into(), json!(([0u8; 20])));
        }
        Self {
            kind,
            header: serde_json::from_value(header).ok(),
            parameters,
            parameter_bytes,
            bytes,
        }
    }
    fn set(&mut self, kind: u32, parameter: usize, value: i32) -> bool {
        if self.kind != Some(kind as u64) {
            return false;
        }
        let Some(p) = self.parameters.get_mut(parameter) else {
            return false;
        };
        let length = if value == 0 {
            1
        } else {
            value.unsigned_abs().ilog10() as usize + 1 + usize::from(value < 0)
        };
        self.bytes = self
            .bytes
            .saturating_sub(self.parameter_bytes[parameter])
            .saturating_add(length);
        self.parameter_bytes[parameter] = length;
        *p = u8::try_from(value).ok();
        true
    }
    fn program(&self) -> Option<effects::Program> {
        let mut p = self.header?;
        if self.parameters.len() != p.parameters.len() {
            return None;
        }
        for (to, from) in p.parameters.iter_mut().zip(&self.parameters) {
            *to = (*from)?;
        }
        Some(p)
    }
}
struct MotionChain {
    id: Option<String>,
    effect: MotionEffect,
}

pub(super) struct Modulation {
    config: Config,
    macros: Macros,
    specs: Vec<Spec>,
    indices: HashMap<Target, usize>,
    effects: Vec<MotionEffect>,
    chains: Vec<Vec<MotionChain>>,
    circuits: Vec<Value>,
    profiles: [HashMap<String, u32>; TIMBRE_COUNT],
    changes: Vec<(usize, f64)>,
    ordered: Vec<(usize, f64)>,
    dirty_chains: HashSet<(usize, usize)>,
    macro_bindings: Vec<Vec<(usize, i32)>>,
    knob_targets: [Vec<usize>; 8],
    held: Vec<HashSet<u64>>,
    live_macros: [Option<f64>; 8],
    macro_motion: [Option<f64>; 8],
    needs_reapply: bool,
    tempo: f64,
    running: bool,
    gain: f64,
    base_gain: f64,
}
impl Default for Modulation {
    fn default() -> Self {
        Self {
            config: Config::default(),
            macros: Macros::default(),
            specs: vec![],
            indices: HashMap::new(),
            effects: vec![],
            chains: vec![],
            circuits: vec![],
            profiles: core::array::from_fn(|_| HashMap::new()),
            changes: vec![],
            ordered: vec![],
            dirty_chains: HashSet::new(),
            macro_bindings: vec![],
            knob_targets: core::array::from_fn(|_| vec![]),
            held: (0..TIMBRE_COUNT).map(|_| HashSet::new()).collect(),
            live_macros: [None; 8],
            macro_motion: [None; 8],
            needs_reapply: false,
            tempo: 120.,
            running: false,
            gain: 0.3,
            base_gain: 0.3,
        }
    }
}
impl Modulation {
    pub(super) fn set_state(&mut self, bytes: &[u8]) -> bool {
        let Ok(mut state) = serde_json::from_slice::<State>(bytes) else {
            return false;
        };
        if state.config.version != 1
            || !(4..=TIMBRE_COUNT).contains(&state.config.tracks.len())
            || state.macros.knobs.len() != 8
            || state.macros.bases.len() > 96
            || state.targets.len() > 8 * 6 + 96
        {
            return false;
        }
        let mut indices = HashMap::new();
        for (i, spec) in state.targets.iter_mut().enumerate() {
            if !spec.target.valid()
                || ![spec.min, spec.max, spec.base, spec.step]
                    .iter()
                    .all(|v| v.is_finite())
                || spec.min > spec.max
                || spec.step <= 0.
                || spec.values.as_ref().is_some_and(|v| {
                    v.is_empty() || v.len() > 1024 || v.iter().any(|n| !n.is_finite())
                })
                || indices.insert(spec.target.clone(), i).is_some()
            {
                return false;
            }
            if let Some(base) = state.macros.bases.iter().find(|b| b.target == spec.target) {
                spec.macro_base = Some(base.value);
            }
        }
        for knob in &state.macros.knobs {
            if !knob.value.is_finite()
                || !(0. ..=100.).contains(&knob.value)
                || knob.bindings.len() > 12
                || knob.bindings.iter().any(|b| {
                    !b.target.valid()
                        || !(-100..=100).contains(&b.amount)
                        || !state.macros.bases.iter().any(|v| v.target == b.target)
                })
            {
                return false;
            }
        }
        if state
            .macros
            .bases
            .iter()
            .any(|b| !b.target.valid() || !b.value.is_finite())
        {
            return false;
        }
        for (t, lanes) in state.config.tracks.iter_mut().enumerate() {
            let mut ids = HashSet::new();
            if lanes.len() > 6 {
                return false;
            }
            for lane in lanes {
                let range = lane.target.as_ref().map_or(63, Target::range);
                if !valid_id(&lane.id)
                    || !ids.insert(lane.id.clone())
                    || !(-100..=100).contains(&lane.amount)
                    || lane.direction == Direction::Random
                        && lane.playback.as_deref() != Some("Poly")
                    || !(1..=128).contains(&lane.length)
                    || !DIVISIONS.contains(&lane.resolution.as_str())
                    || lane.values.len() != 128
                    || lane.values.iter().any(|v| !(-range..=range).contains(v))
                    || lane.target.as_ref().is_some_and(|target| !target.valid())
                {
                    return false;
                }
                let (n, d) = lane.resolution.split_once('/').unwrap();
                lane.target_index = lane
                    .target
                    .as_ref()
                    .and_then(|target| indices.get(target).copied());
                lane.beats = 4. * n.parse::<f64>().unwrap() / d.parse::<f64>().unwrap();
                lane.phase = self
                    .config
                    .tracks
                    .get(t)
                    .and_then(|ls| ls.iter().find(|l| l.id == lane.id))
                    .map_or(0., |l| l.phase);
            }
        }
        while state.config.tracks.len() < TIMBRE_COUNT {
            state.config.tracks.push(vec![]);
        }
        self.restore();
        self.config = state.config;
        self.macros = state.macros;
        self.specs = state.targets;
        self.indices = indices;
        self.effects = state
            .effects
            .get("slots")
            .and_then(Value::as_array)
            .map(|slots| slots.iter().cloned().map(MotionEffect::new).collect())
            .unwrap_or_default();
        self.chains = state
            .effects
            .get("chains")
            .and_then(Value::as_array)
            .map(|tracks| {
                tracks
                    .iter()
                    .map(|track| {
                        track
                            .as_array()
                            .map(|ps| {
                                ps.iter()
                                    .cloned()
                                    .map(|p| MotionChain {
                                        id: p.get("id").and_then(Value::as_str).map(str::to_owned),
                                        effect: MotionEffect::new(p),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.circuits = state.circuits;
        self.changes = Vec::with_capacity(self.specs.len().max(12));
        self.ordered = Vec::with_capacity(self.specs.len().max(12));
        self.dirty_chains = HashSet::with_capacity(self.specs.len());
        self.macro_bindings = (0..self.specs.len()).map(|_| vec![]).collect();
        self.knob_targets.iter_mut().for_each(Vec::clear);
        for (k, knob) in self.macros.knobs.iter().enumerate() {
            for binding in &knob.bindings {
                if let Some(&i) = self.indices.get(&binding.target) {
                    self.macro_bindings[i].push((k, binding.amount));
                    self.knob_targets[k].push(i);
                }
            }
        }
        self.macro_motion.fill(None);
        self.needs_reapply = false;
        true
    }
    pub(super) fn set_profile(&mut self, timbre: usize, id: u32, source: String) {
        self.profiles[timbre].insert(source, id);
    }
    pub(super) fn clear_profiles(&mut self) {
        self.profiles.iter_mut().for_each(HashMap::clear);
    }
    pub(super) fn release_keys(&mut self, timbre: usize) {
        self.held[timbre].clear();
    }
    pub(super) fn set_tempo(&mut self, value: f64) {
        if value.is_finite() {
            self.tempo = value.clamp(10., 300.);
        }
    }
    pub(super) fn reset(&mut self, beat: f64) {
        for lane in self.config.tracks.iter_mut().flatten() {
            lane.phase = beat.max(0.) / lane.beats;
        }
    }
    pub(super) fn play(&mut self) {
        self.reset(0.);
        self.running = true;
    }
    pub(super) fn stop(&mut self) {
        self.running = false;
        for lane in self.config.tracks.iter_mut().flatten() {
            lane.position = None;
        }
        self.held.iter_mut().for_each(HashSet::clear);
        self.restore();
    }
    pub(super) fn transport(&mut self, playing: bool, tempo: f64, beat: f64, seek: bool) {
        self.set_tempo(tempo);
        if !playing {
            self.stop();
            return;
        }
        if !self.running || seek {
            self.reset(beat);
        }
        self.running = true;
    }
    pub(super) fn note(&mut self, timbre: usize, note: u64, velocity: u32) {
        if timbre >= TIMBRE_COUNT {
            return;
        }
        if velocity > 0 {
            if !self.running {
                self.play();
            }
            let first = self.held[timbre].is_empty();
            self.held[timbre].insert(note);
            if let Some(lanes) = self.config.tracks.get_mut(timbre) {
                for lane in lanes {
                    if lane.key_sync == KeySync::Voice || lane.key_sync == KeySync::Timbre && first
                    {
                        lane.phase = 0.;
                    }
                }
            }
        } else {
            self.held[timbre].remove(&note);
        }
    }
    fn macro_offset(&self, index: usize, min: f64, max: f64, live: bool) -> f64 {
        let mut weight = 0.;
        // Preserve original knob/binding order and every separately rounded
        // operation. Do not pre-sum duplicate weights or factor divisions out.
        for &(i, amount) in &self.macro_bindings[index] {
            let knob = &self.macros.knobs[i];
            let value = if live {
                self.live_macros[i].unwrap_or(knob.value)
            } else {
                knob.value
            };
            weight += value / 100. * amount as f64 / 100.;
        }
        weight * (max - min)
    }
    pub(super) fn parameter(&mut self, index: usize, value: f64) {
        self.restore();
        if index == 8 {
            self.base_gain = value.clamp(0., 100.) / 100.;
            self.gain = self.base_gain;
            if let Some(i) = self.indices.get(&Target::Volume) {
                self.specs[*i].base = value.clamp(0., 100.);
            }
            return;
        }
        let Some(knob) = self.macros.knobs.get_mut(index) else {
            return;
        };
        knob.value = value.clamp(0., 100.);
        let mut changes = std::mem::take(&mut self.changes);
        changes.clear();
        for &i in &self.knob_targets[index] {
            let spec = &self.specs[i];
            let Some(base) = spec.macro_base else {
                continue;
            };
            if !spec.available {
                continue;
            }
            let value = spec.bounded(base + self.macro_offset(i, spec.min, spec.max, false));
            self.specs[i].base = value;
            changes.push((i, value));
        }
        self.write(&mut changes);
        changes.clear();
        self.changes = changes;
    }
    pub(super) fn gain(&self) -> f64 {
        self.gain
    }
    pub(super) fn before_render(&mut self, frames: usize) {
        if self
            .config
            .tracks
            .iter()
            .flatten()
            .any(|lane| lane.playback.as_deref() == Some("Poly"))
        {
            rustias_poly_motion_tempo(self.tempo);
        }
        if !self.running {
            return;
        }
        for spec in &mut self.specs {
            spec.offset = None;
            spec.macro_active = false;
        }
        self.live_macros.fill(None);
        self.macro_motion.fill(None);
        for lanes in &mut self.config.tracks {
            for lane in lanes {
                lane.position = None;
                if !lane.enabled
                    || lane.target.is_none()
                    || lane.playback.as_deref() == Some("Poly")
                {
                    continue;
                }
                let value = lane.value(frames, self.tempo);
                match lane.target.as_ref().unwrap() {
                    Target::Macro { index } => {
                        self.macro_motion[*index] = Some(value);
                        self.live_macros[*index] = self
                            .macros
                            .knobs
                            .get(*index)
                            .map(|knob| (knob.value + value).clamp(0., 100.));
                    }
                    _ => {
                        if let Some(i) = lane.target_index {
                            self.specs[i].offset = Some(value);
                        }
                    }
                }
            }
        }
        self.apply_motion();
    }
    // Restore/rebase transactions must not remove the held modulation from the
    // rest of a partial audio block. Reapply its cached offsets without advancing
    // Step/Slide, probability, RNG or lane phase a second time.
    pub(super) fn resume_render(&mut self) {
        if !self.running || !self.needs_reapply {
            return;
        }
        for i in 0..8 {
            self.live_macros[i] = self.macro_motion[i].and_then(|offset| {
                self.macros
                    .knobs
                    .get(i)
                    .map(|knob| (knob.value + offset).clamp(0., 100.))
            });
        }
        self.apply_motion();
    }
    fn apply_motion(&mut self) {
        self.needs_reapply = false;
        if self.specs.is_empty() {
            return;
        }
        for (i, targets) in self.knob_targets.iter().enumerate() {
            if self.live_macros[i].is_some() {
                for &index in targets {
                    self.specs[index].macro_active = true;
                }
            }
        }
        let mut changes = std::mem::take(&mut self.changes);
        changes.clear();
        for (i, spec) in self.specs.iter().enumerate() {
            let active = spec.offset.is_some() || spec.macro_active;
            if !active && spec.output.is_none() {
                continue;
            }
            let mut value = spec.base;
            if active && spec.available {
                if let Some(base) = spec.macro_base {
                    value = base + self.macro_offset(i, spec.min, spec.max, true);
                }
                value = spec.bounded(value + spec.offset.unwrap_or(0.));
            }
            if value != spec.output.unwrap_or(spec.base) {
                changes.push((i, value));
            }
        }
        self.write(&mut changes);
        for &(i, value) in &changes {
            let spec = &mut self.specs[i];
            spec.output = if spec.offset.is_some() || spec.macro_active {
                Some(value)
            } else {
                None
            };
        }
        changes.clear();
        self.changes = changes;
    }
    pub(super) fn restore(&mut self) {
        self.needs_reapply |= self.specs.iter().any(|s| s.output.is_some());
        let mut changes = std::mem::take(&mut self.changes);
        changes.clear();
        changes.extend(
            self.specs
                .iter()
                .enumerate()
                .filter(|(_, s)| s.output.is_some())
                .map(|(i, s)| (i, s.base)),
        );
        self.write(&mut changes);
        for spec in &mut self.specs {
            spec.output = None;
        }
        self.live_macros.fill(None);
        changes.clear();
        self.changes = changes;
    }
    pub(super) fn accept_values(&mut self) {
        for i in 0..self.specs.len() {
            let spec = &self.specs[i];
            if let Some(value) = self.read(&spec.target) {
                if value != spec.base {
                    let offset = self.macro_offset(i, spec.min, spec.max, false);
                    let spec = &mut self.specs[i];
                    spec.base = value;
                    if spec.macro_base.is_some() {
                        spec.macro_base = Some(value - offset);
                    }
                }
            }
        }
    }
    fn read(&self, target: &Target) -> Option<f64> {
        match target {
            Target::Synth { timbre, parameter } => {
                Some(rustias_value(*timbre as u32, *parameter) as f64)
            }
            Target::Global { parameter } => Some(rustias_value(0, *parameter) as f64),
            Target::Drum {
                instrument,
                parameter,
            } => with_engine(|e| {
                e.borrow()
                    .as_ref()
                    .map(|e| e.synth.drum_settings[*instrument][*parameter as usize] as f64)
            }),
            Target::Sample {
                timbre,
                source,
                parameter,
            } => self.profiles[*timbre]
                .get(source.as_str())
                .map(|id| rustias_library_value(*id, *parameter) as f64),
            _ => None,
        }
    }
    // All destinations use the same Rust controls/FX/circuit compiler as the UI.
    // Only changes are written; oscillator and envelope audio state is preserved.
    fn write(&mut self, changes: &mut Vec<(usize, f64)>) {
        if changes.is_empty() {
            return;
        }
        self.ordered.clear();
        self.ordered.extend(
            changes
                .iter()
                .copied()
                .filter(|(i, _)| self.specs[*i].target.wave()),
        );
        self.ordered.extend(
            changes
                .iter()
                .copied()
                .filter(|(i, _)| !self.specs[*i].target.wave()),
        );
        std::mem::swap(changes, &mut self.ordered);
        let mut fx = [false; EFFECT_SLOTS];
        self.dirty_chains.clear();
        let mut circuits = [false; TIMBRE_COUNT];
        let mut library = false;
        changes.retain_mut(|(i, value)| {
            let target = &self.specs[*i].target;
            match target {
                Target::Synth { parameter, .. } | Target::Global { parameter } => {
                    let timbre = if let Target::Synth { timbre, .. } = target {
                        *timbre as u32
                    } else {
                        0
                    };
                    if let Some(wave) = mode_wave(*parameter) {
                        if rustias_value(timbre, wave) >= 4 {
                            *value = 0.;
                        }
                    }
                    if (171..179).contains(parameter) {
                        rustias_osc3_control(timbre, parameter - 171, *value as i32) != 0
                    } else {
                        rustias_control(timbre, *parameter, *value as i32) != 0
                    }
                }
                Target::Drum {
                    instrument,
                    parameter,
                } => {
                    if let Some(wave) = mode_wave(*parameter) {
                        if self
                            .read(&Target::Drum {
                                instrument: *instrument,
                                parameter: wave,
                            })
                            .unwrap_or(0.)
                            >= 4.
                        {
                            *value = 0.;
                        }
                    }
                    rustias_drum_control(*instrument as u32, *parameter, *value as i32) != 0
                }
                Target::Sample {
                    timbre,
                    source,
                    parameter,
                } => {
                    let Some(id) = self.profiles[*timbre].get(source.as_str()) else {
                        return false;
                    };
                    if let Some(wave) = mode_wave(*parameter) {
                        if rustias_library_value(*id, wave) >= 4 {
                            *value = 0.;
                        }
                    }
                    let ok = rustias_library_control(*id, *parameter, *value as i32) != 0;
                    library |= ok;
                    ok
                }
                Target::Effect {
                    slot,
                    effect_kind,
                    parameter,
                } => {
                    let Some(program) = self.effects.get_mut(*slot) else {
                        return false;
                    };
                    if !program.set(*effect_kind, *parameter, *value as i32) {
                        return false;
                    }
                    fx[*slot] = true;
                    true
                }
                Target::ChainEffect {
                    timbre,
                    id,
                    effect_kind,
                    parameter,
                } => {
                    let Some((index, program)) = self.chains.get_mut(*timbre).and_then(|chain| {
                        chain.iter_mut().enumerate().find(|(_, p)| {
                            p.id.as_deref() == Some(id)
                                && p.effect.kind == Some(*effect_kind as u64)
                        })
                    }) else {
                        return false;
                    };
                    if !program.effect.set(*effect_kind, *parameter, *value as i32) {
                        return false;
                    }
                    // The historical JSON dispatcher selects the first ID after
                    // mutating the first (ID, kind). Preserve that behavior even
                    // for accepted raw modulation states with duplicate IDs.
                    let dispatch = self.chains[*timbre]
                        .iter()
                        .position(|p| p.id.as_deref() == Some(id))
                        .unwrap();
                    let _ = index;
                    self.dirty_chains.insert((*timbre, dispatch));
                    true
                }
                Target::Module {
                    timbre,
                    node,
                    module_kind,
                    control,
                } => {
                    let Some(module) = self
                        .circuits
                        .get_mut(*timbre)
                        .and_then(|c| c.get_mut("nodes"))
                        .and_then(Value::as_array_mut)
                        .and_then(|ns| {
                            ns.iter_mut().find(|n| {
                                n["id"].as_u64() == Some(*node as u64)
                                    && n["kind"].as_str() == Some(module_kind)
                            })
                        })
                    else {
                        return false;
                    };
                    let Some(params) = module.get_mut("params").and_then(Value::as_object_mut)
                    else {
                        return false;
                    };
                    params.insert(control.clone(), json!(*value));
                    if module_kind == "oscillator1"
                        && params.get("wave").and_then(Value::as_f64).unwrap_or(0.) >= 4.
                    {
                        params.insert("mode".into(), json!(0));
                        if control == "mode" {
                            *value = 0.;
                        }
                    }
                    circuits[*timbre] = true;
                    true
                }
                Target::KitGain => {
                    rustias_drum_gain(*value as i32);
                    true
                }
                Target::Volume => {
                    self.gain = *value / 100.;
                    true
                }
                Target::SequenceGate { .. } | Target::Macro { .. } => false,
            }
        });
        if library {
            rustias_library_sync();
        }
        let mut failed = [false; 8 * 6 + 96];
        for (slot, dirty) in fx.into_iter().enumerate() {
            if !dirty {
                continue;
            }
            let ok = self.effects[slot].bytes <= PRESET_CAPACITY
                && self.effects[slot].program().is_some_and(|p| {
                    with_engine(|e| {
                        e.borrow_mut()
                            .as_mut()
                            .is_some_and(|e| apply_effect(e, slot, p))
                    })
                });
            if !ok {
                for &(i, _) in changes.iter() {
                    if matches!(self.specs[i].target, Target::Effect { slot: s, .. } if s == slot) {
                        failed[i] = true;
                    }
                }
            }
        }
        for &(timbre, index) in &self.dirty_chains {
            let fx = &self.chains[timbre][index];
            let id = fx.id.as_deref().unwrap();
            let ok = fx.effect.program().is_some_and(|p| {
                with_engine(|e| {
                    e.borrow_mut()
                        .as_mut()
                        .is_some_and(|e| apply_chain_effect(e, timbre, id, p))
                })
            });
            if !ok {
                for &(i, _) in changes.iter() {
                    if matches!(&self.specs[i].target, Target::ChainEffect { timbre: t, id: s, .. } if *t == timbre && s == id)
                    {
                        failed[i] = true;
                    }
                }
            }
        }
        for (timbre, dirty) in circuits.into_iter().enumerate() {
            if dirty
                && !json_call(&self.circuits[timbre], |len| {
                    rustias_circuit(timbre as u32, len)
                })
            {
                for &(i, _) in changes.iter() {
                    if matches!(self.specs[i].target, Target::Module { timbre: t, .. } if t == timbre)
                    {
                        failed[i] = true;
                    }
                }
            }
        }
        changes.retain(|(i, _)| !failed[*i]);
    }
    pub(super) fn snapshot_layout(&self) -> (Vec<Target>, Vec<(usize, String, u32)>) {
        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .enumerate()
            .flat_map(|(t, ps)| ps.iter().map(move |(source, id)| (t, source.clone(), *id)))
            .collect();
        profiles.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        (
            self.specs.iter().map(|s| s.target.clone()).collect(),
            profiles,
        )
    }
    pub(super) fn capture_status(&self, s: &mut super::snapshot::MotionStatus) {
        s.running = self.running;
        s.macros = self.live_macros;
        s.tracks = self.config.tracks.len();
        s.positions.fill(-1);
        s.lengths.fill(0);
        for (t, lanes) in self.config.tracks.iter().enumerate() {
            s.lengths[t] = lanes.len();
            for (i, lane) in lanes.iter().enumerate() {
                s.positions[t * 6 + i] = lane.position.map_or(-1, |p| p as i32);
            }
        }
        s.outputs.fill(None);
        for (to, spec) in s.outputs.iter_mut().zip(&self.specs) {
            *to = spec.output;
        }
    }
    pub(super) fn status(&self) -> Value {
        let positions: Vec<Vec<i32>> = self
            .config
            .tracks
            .iter()
            .map(|lanes| {
                lanes
                    .iter()
                    .map(|l| l.position.map_or(-1, |n| n as i32))
                    .collect()
            })
            .collect();
        json!({"running":self.running,"positions":positions,"macros":self.live_macros,"values":self.specs.iter().filter_map(|s| s.output.map(|value| json!({"target":s.target,"value":value}))).collect::<Vec<_>>(),"backend":"native-rust"})
    }
}
fn mode_wave(parameter: u32) -> Option<u32> {
    match parameter {
        10 => Some(0),
        172 => Some(171),
        180 => Some(179),
        _ => None,
    }
}
pub(super) fn bytes(length: u32) -> Option<Vec<u8>> {
    with_engine(|s| {
        s.borrow()
            .as_ref()?
            .preset
            .get(..length as usize)
            .map(<[u8]>::to_vec)
    })
}
pub(super) fn stage(bytes: &[u8]) -> bool {
    with_engine(|s| {
        let mut s = s.borrow_mut();
        let Some(e) = s.as_mut() else {
            return false;
        };
        if bytes.len() > e.preset.len() {
            e.preset.resize(bytes.len(), 0);
        }
        e.preset[..bytes.len()].copy_from_slice(bytes);
        true
    })
}
fn json_call(value: &Value, call: impl FnOnce(u32) -> u32) -> bool {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return false;
    };
    stage(&bytes) && call(bytes.len() as u32) != 0
}
