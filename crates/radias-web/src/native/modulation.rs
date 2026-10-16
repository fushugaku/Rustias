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
                parameter: 53 | 15, ..
            }
            | Self::Drum {
                parameter: 53 | 15, ..
            }
            | Self::Sample {
                parameter: 53 | 15, ..
            } => 24,
            _ => 63,
        }
    }
    fn wave(&self) -> bool {
        matches!(
            self,
            Self::Synth {
                parameter: 0 | 171,
                ..
            } | Self::Global { parameter: 0 | 171 }
                | Self::Drum {
                    parameter: 0 | 171,
                    ..
                }
                | Self::Sample {
                    parameter: 0 | 171,
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

pub(super) struct Modulation {
    config: Config,
    macros: Macros,
    specs: Vec<Spec>,
    indices: HashMap<Target, usize>,
    effects: Vec<Value>,
    chains: Vec<Vec<Value>>,
    circuits: Vec<Value>,
    profiles: HashMap<(usize, String), u32>,
    held: Vec<HashSet<u64>>,
    live_macros: [Option<f64>; 8],
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
            profiles: HashMap::new(),
            held: (0..TIMBRE_COUNT).map(|_| HashSet::new()).collect(),
            live_macros: [None; 8],
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
                    || !(1..=128).contains(&lane.length)
                    || !DIVISIONS.contains(&lane.resolution.as_str())
                    || lane.values.len() != 128
                    || lane.values.iter().any(|v| !(-range..=range).contains(v))
                    || lane.target.as_ref().is_some_and(|target| !target.valid())
                {
                    return false;
                }
                let (n, d) = lane.resolution.split_once('/').unwrap();
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
            .cloned()
            .unwrap_or_default();
        self.chains = state
            .effects
            .get("chains")
            .and_then(Value::as_array)
            .map(|tracks| {
                tracks
                    .iter()
                    .map(|track| track.as_array().cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        self.circuits = state.circuits;
        true
    }
    pub(super) fn set_profile(&mut self, timbre: usize, id: u32, source: String) {
        self.profiles.insert((timbre, source), id);
    }
    pub(super) fn clear_profiles(&mut self) {
        self.profiles.clear();
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
    fn macro_offset(&self, target: &Target, min: f64, max: f64, live: bool) -> f64 {
        let mut weight = 0.;
        for (i, knob) in self.macros.knobs.iter().enumerate() {
            let value = if live {
                self.live_macros[i].unwrap_or(knob.value)
            } else {
                knob.value
            };
            for binding in &knob.bindings {
                if binding.target == *target {
                    weight += value / 100. * binding.amount as f64 / 100.;
                }
            }
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
        let targets: Vec<_> = knob.bindings.iter().map(|b| b.target.clone()).collect();
        let mut changes = Vec::with_capacity(targets.len());
        for target in targets {
            let Some(&i) = self.indices.get(&target) else {
                continue;
            };
            let spec = &self.specs[i];
            let Some(base) = spec.macro_base else {
                continue;
            };
            if !spec.available {
                continue;
            }
            let value = spec.bounded(base + self.macro_offset(&target, spec.min, spec.max, false));
            self.specs[i].base = value;
            changes.push((i, value));
        }
        self.write(&mut changes);
    }
    pub(super) fn gain(&self) -> f64 {
        self.gain
    }
    pub(super) fn before_render(&mut self, frames: usize) {
        if !self.running {
            return;
        }
        for spec in &mut self.specs {
            spec.offset = None;
            spec.macro_active = false;
        }
        self.live_macros.fill(None);
        for lanes in &mut self.config.tracks {
            for lane in lanes {
                lane.position = None;
                if !lane.enabled || lane.target.is_none() {
                    continue;
                }
                let value = lane.value(frames, self.tempo);
                match lane.target.as_ref().unwrap() {
                    Target::Macro { index } => {
                        self.live_macros[*index] = self
                            .macros
                            .knobs
                            .get(*index)
                            .map(|knob| (knob.value + value).clamp(0., 100.))
                    }
                    target => {
                        if let Some(i) = self.indices.get(target) {
                            self.specs[*i].offset = Some(value);
                        }
                    }
                }
            }
        }
        for (i, knob) in self.macros.knobs.iter().enumerate() {
            if self.live_macros[i].is_some() {
                for binding in &knob.bindings {
                    if let Some(i) = self.indices.get(&binding.target) {
                        self.specs[*i].macro_active = true;
                    }
                }
            }
        }
        let mut changes = Vec::new();
        for (i, spec) in self.specs.iter().enumerate() {
            let active = spec.offset.is_some() || spec.macro_active;
            if !active && spec.output.is_none() {
                continue;
            }
            let mut value = spec.base;
            if active && spec.available {
                if let Some(base) = spec.macro_base {
                    value = base + self.macro_offset(&spec.target, spec.min, spec.max, true);
                }
                value = spec.bounded(value + spec.offset.unwrap_or(0.));
            }
            if value != spec.output.unwrap_or(spec.base) {
                changes.push((i, value));
            }
        }
        self.write(&mut changes);
        for (i, value) in changes {
            let spec = &mut self.specs[i];
            spec.output = if spec.offset.is_some() || spec.macro_active {
                Some(value)
            } else {
                None
            };
        }
    }
    pub(super) fn restore(&mut self) {
        let mut changes: Vec<_> = self
            .specs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.output.is_some())
            .map(|(i, s)| (i, s.base))
            .collect();
        self.write(&mut changes);
        for spec in &mut self.specs {
            spec.output = None;
        }
        self.live_macros.fill(None);
    }
    pub(super) fn accept_values(&mut self) {
        for i in 0..self.specs.len() {
            let spec = &self.specs[i];
            if let Some(value) = self.read(&spec.target) {
                if value != spec.base {
                    let offset = self.macro_offset(&spec.target, spec.min, spec.max, false);
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
            } => self
                .profiles
                .get(&(*timbre, source.clone()))
                .map(|id| rustias_library_value(*id, *parameter) as f64),
            _ => None,
        }
    }
    // All destinations use the same Rust controls/FX/circuit compiler as the UI.
    // Only changes are written; oscillator and envelope audio state is preserved.
    fn write(&mut self, changes: &mut Vec<(usize, f64)>) {
        changes.sort_by_key(|(i, _)| !self.specs[*i].target.wave());
        let mut fx = HashSet::new();
        let mut chains = HashSet::new();
        let mut circuits = HashSet::new();
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
                    if *parameter >= 171 {
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
                    let Some(id) = self.profiles.get(&(*timbre, source.clone())) else {
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
                    let Some(program) = self
                        .effects
                        .get_mut(*slot)
                        .filter(|p| p["kind"].as_u64() == Some(*effect_kind as u64))
                    else {
                        return false;
                    };
                    let Some(p) = program
                        .get_mut("parameters")
                        .and_then(Value::as_array_mut)
                        .and_then(|ps| ps.get_mut(*parameter))
                    else {
                        return false;
                    };
                    *p = json!(*value as i32);
                    fx.insert(*slot);
                    true
                }
                Target::ChainEffect {
                    timbre,
                    id,
                    effect_kind,
                    parameter,
                } => {
                    let Some(program) = self.chains.get_mut(*timbre).and_then(|chain| {
                        chain.iter_mut().find(|p| {
                            p["id"].as_str() == Some(id)
                                && p["kind"].as_u64() == Some(*effect_kind as u64)
                        })
                    }) else {
                        return false;
                    };
                    let Some(p) = program
                        .get_mut("parameters")
                        .and_then(Value::as_array_mut)
                        .and_then(|ps| ps.get_mut(*parameter))
                    else {
                        return false;
                    };
                    *p = json!(*value as i32);
                    chains.insert((*timbre, id.clone()));
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
                    circuits.insert(*timbre);
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
        let mut failed = HashSet::new();
        for slot in fx {
            if !json_call(&self.effects[slot], |len| rustias_effect(slot as u32, len)) {
                failed.extend(changes.iter().filter(|(i, _)| matches!(self.specs[*i].target, Target::Effect { slot: s, .. } if s == slot)).map(|(i, _)| *i));
            }
        }
        for (timbre, id) in chains {
            let program = self.chains[timbre]
                .iter()
                .find(|p| p["id"].as_str() == Some(&id))
                .unwrap();
            if !json_call(program, |len| rustias_chain_effect(timbre as u32, len)) {
                failed.extend(changes.iter().filter(|(i, _)| matches!(&self.specs[*i].target, Target::ChainEffect { timbre: t, id: s, .. } if *t == timbre && s == &id)).map(|(i, _)| *i));
            }
        }
        for timbre in circuits {
            if !json_call(&self.circuits[timbre], |len| {
                rustias_circuit(timbre as u32, len)
            }) {
                failed.extend(changes.iter().filter(|(i, _)| matches!(self.specs[*i].target, Target::Module { timbre: t, .. } if t == timbre)).map(|(i, _)| *i));
            }
        }
        changes.retain(|(i, _)| !failed.contains(i));
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
