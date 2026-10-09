//! Optional per-voice modular routing, shared with the native DSP source.
//! Programs are validated/compiled outside rendering; each note owns its states.
use crate::standalone::{StandaloneSynth, Values, default_values};
use radias_synth_application::{
    VoiceCircuit, amplifier::ControllerTables, voice_envelopes::ModEnvelopeProgram,
};
use radias_synth_domain::{
    Sample,
    filter::ResonantFilter,
    filter_routing::{Filter2, Filter2Output},
    fixed::{multiply_q15, saturate},
    mod_envelope::ModEnvelope,
    pitch::PhaseIncrement,
    primary_oscillator::PrimaryOscillator,
    voice::{SignalProcessor, VoiceParameters},
    waveform::WaveformTable,
    waveshaper::{ShaperSignal, Waveshaper},
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};
const LIMIT: usize = 64;
#[derive(Clone, Deserialize)]
pub struct Circuit {
    #[serde(default)]
    pub enabled: bool,
    pub nodes: Vec<Module>,
    pub wires: Vec<Wire>,
}
#[derive(Clone, Deserialize)]
pub struct Module {
    pub id: usize,
    pub kind: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}
#[derive(Clone, Deserialize)]
pub struct Wire {
    pub from: usize,
    pub to: usize,
    pub port: String,
}
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Osc1,
    Osc2,
    Noise,
    Mix,
    Filter1,
    Filter2,
    Drive,
    Amp,
    Eg1,
    Eg2,
    Eg3,
    Lfo1,
    Lfo2,
    Gate,
    Velocity,
    Output,
    Osc,
    Filter,
    Shaper,
    Gain,
    Sum,
    Lfo,
    Envelope,
}
fn kind(name: &str) -> Option<Kind> {
    Some(match name {
        "osc1" => Kind::Osc1,
        "osc2" => Kind::Osc2,
        "noise" => Kind::Noise,
        "mixer" => Kind::Mix,
        "filter1" => Kind::Filter1,
        "filter2" => Kind::Filter2,
        "drive" => Kind::Drive,
        "amp" => Kind::Amp,
        "eg1" => Kind::Eg1,
        "eg2" => Kind::Eg2,
        "eg3" => Kind::Eg3,
        "lfo1" => Kind::Lfo1,
        "lfo2" => Kind::Lfo2,
        "gate" => Kind::Gate,
        "velocity" => Kind::Velocity,
        "output" => Kind::Output,
        "oscillator" => Kind::Osc,
        "filter" => Kind::Filter,
        "shaper" => Kind::Shaper,
        "vca" => Kind::Gain,
        "sum" => Kind::Sum,
        "lfo" => Kind::Lfo,
        "envelope" => Kind::Envelope,
        _ => return None,
    })
}
fn cv(k: Kind) -> bool {
    matches!(
        k,
        Kind::Eg1
            | Kind::Eg2
            | Kind::Eg3
            | Kind::Lfo1
            | Kind::Lfo2
            | Kind::Gate
            | Kind::Velocity
            | Kind::Lfo
            | Kind::Envelope
    )
}
fn port(k: Kind, name: &str) -> Option<(usize, bool)> {
    match (k, name) {
        (Kind::Mix | Kind::Sum, "a") => Some((0, false)),
        (Kind::Mix | Kind::Sum, "b") => Some((1, false)),
        (Kind::Mix | Kind::Sum, "c") => Some((2, false)),
        (
            Kind::Filter1
            | Kind::Filter2
            | Kind::Filter
            | Kind::Drive
            | Kind::Shaper
            | Kind::Amp
            | Kind::Gain
            | Kind::Output,
            "in",
        ) => Some((0, false)),
        (Kind::Filter1 | Kind::Filter2 | Kind::Filter, "cutoff") => Some((3, true)),
        (Kind::Amp | Kind::Gain, "gain") => Some((3, true)),
        (Kind::Osc, "pitch") => Some((3, true)),
        (Kind::Lfo, "rate") => Some((3, true)),
        (Kind::Envelope, "gate") => Some((3, true)),
        _ => None,
    }
}
struct Node {
    id: usize,
    kind: Kind,
    inputs: [Option<usize>; 4],
    values: Values,
    numbers: [f64; 6],
    filters: Vec<radias_synth_domain::filter::FilterCoefficients>,
    program: radias_synth_application::drum_program::DrumInstrumentProgram,
}
struct Plan {
    nodes: Vec<Node>,
    order: Vec<usize>,
    output: usize,
    controllers: Box<ControllerTables>,
    has_amp: bool,
}
pub struct CircuitVoice {
    plan: Arc<Plan>,
    states: Vec<State>,
    controls: [f64; 8],
    frames: u64,
    release_frames: u32,
}
struct State {
    osc: PrimaryOscillator,
    filter: ResonantFilter,
    second: Filter2,
    shaper: Waveshaper,
    shaper_gain: i16,
    comb: Option<Box<radias_synth_domain::comb::Comb>>,
    env: ModEnvelope,
    env_started: bool,
    env_gate: bool,
    phase: f64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            osc: Default::default(),
            filter: Default::default(),
            second: Default::default(),
            shaper: Default::default(),
            shaper_gain: 0,
            comb: None,
            env: Default::default(),
            env_started: false,
            env_gate: false,
            phase: 0.0,
        }
    }
}
impl CircuitVoice {
    pub fn compile(
        circuit: &Circuit,
        synth: &StandaloneSynth,
        base: &Values,
    ) -> Result<Option<Self>, String> {
        if circuit.nodes.is_empty() || circuit.nodes.len() > LIMIT || circuit.wires.len() > 256 {
            return Err("Use 1–64 modules and at most 256 cables.".into());
        }
        let mut ids = [None; LIMIT];
        let mut nodes = Vec::new();
        let mut output = None;
        for module in &circuit.nodes {
            if module.id >= LIMIT || ids[module.id].is_some() {
                return Err("Module IDs must be unique.".into());
            }
            let k = kind(&module.kind).ok_or("Unknown module type.")?;
            let mut values = default_values();
            let mut numbers = [0.0; 6];
            let get = |key: &str, default: f64, min: f64, max: f64| -> Result<f64, String> {
                let v = module.params.get(key).copied().unwrap_or(default);
                if !v.is_finite() || v < min || v > max {
                    return Err(format!("Invalid {key}."));
                }
                Ok(v)
            };
            match k {
                Kind::Osc => {
                    values[0] = get("wave", 0.0, 0.0, 3.0)? as i32;
                    numbers[0] = get("semitone", 0.0, -48.0, 48.0)?;
                    numbers[1] = get("level", 64.0, 0.0, 127.0)? / 127.0;
                }
                Kind::Filter => {
                    values[1] = get("cutoff", 96.0, 0.0, 127.0)? as i32;
                    values[2] = get("resonance", 0.0, 0.0, 127.0)? as i32;
                    values[9] = get("morph", 0.0, 0.0, 127.0)? as i32;
                }
                Kind::Shaper => {
                    values[29] = get("mode", 1.0, 0.0, 2.0)? as i32;
                    values[154] = get("type", 1.0, 0.0, 10.0)? as i32;
                    values[31] = get("depth", 32.0, 0.0, 127.0)? as i32;
                }
                Kind::Gain => numbers[0] = 10.0_f64.powf(get("gain", 0.0, -48.0, 24.0)? / 20.0),
                Kind::Sum => {
                    for (i, key) in ["a", "b", "c"].iter().enumerate() {
                        numbers[i] = get(key, 64.0, 0.0, 127.0)? / 127.0;
                    }
                }
                Kind::Lfo => {
                    numbers[0] = get("rate", 1.0, 0.01, 40.0)?;
                    numbers[1] = get("shape", 0.0, 0.0, 3.0)?;
                    numbers[2] = get("depth", 100.0, 0.0, 100.0)? / 100.0;
                }
                Kind::Envelope => {
                    for (i, key) in ["attack", "decay", "sustain", "release"].iter().enumerate() {
                        numbers[i] = get(key, [0.0, 48.0, 100.0, 32.0][i], 0.0, 127.0)?;
                    }
                }
                _ => values = *base,
            }
            ids[module.id] = Some(nodes.len());
            if k == Kind::Output {
                if output.is_some() {
                    return Err("Use one Output module.".into());
                }
                output = Some(module.id);
            }
            nodes.push(Node {
                id: module.id,
                kind: k,
                inputs: [None; 4],
                program: synth.compile_drum_values(&values),
                values,
                numbers,
                filters: Vec::new(),
            });
        }
        let output = output.ok_or("The patch needs an Output module.")?;
        for wire in &circuit.wires {
            let a = ids
                .get(wire.from)
                .copied()
                .flatten()
                .ok_or("Cable source is missing.")?;
            let b = ids
                .get(wire.to)
                .copied()
                .flatten()
                .ok_or("Cable destination is missing.")?;
            let (p, control) = port(nodes[b].kind, &wire.port).ok_or("Unknown input port.")?;
            if control != cv(nodes[a].kind) || nodes[a].kind == Kind::Output {
                return Err("Connect audio to audio and CV to CV.".into());
            }
            if nodes[b].inputs[p].replace(wire.from).is_some() {
                return Err("Each input accepts one cable; use a Mixer to combine signals.".into());
            }
        }
        let mut order = Vec::new();
        let mut done = [false; LIMIT];
        while order.len() < nodes.len() {
            let before = order.len();
            for (i, n) in nodes.iter().enumerate() {
                if !done[n.id] && n.inputs.iter().flatten().all(|id| done[*id]) {
                    done[n.id] = true;
                    order.push(i);
                }
            }
            if before == order.len() {
                return Err("Feedback loops need a delay and are not supported.".into());
            }
        }
        // Unconnected modules consume no sample-time work and retain no release tail.
        let mut needed = [false; LIMIT];
        let mut stack = vec![output];
        while let Some(id) = stack.pop() {
            if needed[id] {
                continue;
            }
            needed[id] = true;
            stack.extend(nodes[ids[id].unwrap()].inputs.iter().flatten().copied());
        }
        order.retain(|i| needed[nodes[*i].id]);
        for node in &mut nodes {
            if node.kind == Kind::Filter
                || (matches!(node.kind, Kind::Filter1) && node.inputs[3].is_some())
            {
                for cutoff in 0..128 {
                    let mut v = node.values;
                    v[1] = cutoff;
                    node.filters
                        .push(synth.compile_drum_values(&v).graph.filter);
                }
            }
        }
        if !circuit.enabled {
            return Ok(None);
        }
        let has_amp = order
            .iter()
            .any(|i| matches!(nodes[*i].kind, Kind::Amp | Kind::Eg2));
        Ok(Some(Self::new(Arc::new(Plan {
            nodes,
            order,
            output,
            controllers: Box::new(crate::standalone_tables::controllers()),
            has_amp,
        }))))
    }
    fn new(plan: Arc<Plan>) -> Self {
        let mut states: Vec<State> = (0..LIMIT).map(|_| State::default()).collect();
        for n in &plan.nodes {
            if n.kind == Kind::Filter2 {
                states[n.id].comb = Some(Box::new(radias_synth_domain::comb::Comb {
                    feedback: Default::default(),
                    delay: Default::default(),
                }));
            }
        }
        Self {
            plan,
            states,
            controls: [1.0, 1.0, 60.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            frames: 0,
            release_frames: 0,
        }
    }
}
impl VoiceCircuit for CircuitVoice {
    fn fresh(&self) -> Box<dyn VoiceCircuit> {
        Box::new(Self::new(self.plan.clone()))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn reconfigure(&mut self, prototype: &dyn VoiceCircuit) {
        if let Some(next) = prototype.as_any().downcast_ref::<Self>() {
            for n in &next.plan.nodes {
                if !self
                    .plan
                    .nodes
                    .iter()
                    .any(|old| old.id == n.id && old.kind == n.kind)
                {
                    self.states[n.id] = State::default();
                }
            }
            self.plan = next.plan.clone();
            for n in &self.plan.nodes {
                if n.kind == Kind::Filter2 && self.states[n.id].comb.is_none() {
                    self.states[n.id].comb = Some(Box::new(radias_synth_domain::comb::Comb {
                        feedback: Default::default(),
                        delay: Default::default(),
                    }));
                }
            }
        }
    }
    fn controls(&mut self, v: [f64; 8]) {
        self.controls = v;
    }
    fn tail_active(&self) -> bool {
        self.controls[0] == 0.0
            && ((!self.plan.has_amp && self.release_frames < 2400)
                || self
                    .plan
                    .order
                    .iter()
                    .map(|i| &self.plan.nodes[*i])
                    .any(|n| {
                        n.kind == Kind::Envelope && self.states[n.id].env.envelope.segment.level > 0
                    }))
    }
}
impl SignalProcessor for CircuitVoice {
    fn process(
        &mut self,
        sources: [Sample; 3],
        p: VoiceParameters,
        level: i16,
        table: &WaveformTable,
    ) -> Sample {
        let mut out = [0.0; LIMIT];
        let gate = self.controls[0] > 0.0;
        if !gate {
            self.release_frames = self.release_frames.saturating_add(1);
        } else {
            self.release_frames = 0;
        }
        for &i in &self.plan.order {
            let n = &self.plan.nodes[i];
            let state = &mut self.states[n.id];
            let input = n.inputs.map(|id| id.map_or(0.0, |id| out[id]));
            let gain = if n.inputs[3].is_some() { input[3] } else { 1.0 };
            let audio = Sample((input[0] * 2147483647.0) as i32);
            let result = match n.kind {
                Kind::Osc1 => sources[0].0 as f64 / 2147483647.0,
                Kind::Osc2 => sources[1].0 as f64 / 2147483647.0,
                Kind::Noise => sources[2].0 as f64 / 2147483647.0,
                Kind::Mix => {
                    input[0] * p.mix.primary_gain as f64 / 32768.0
                        + input[1] * p.mix.secondary_gain as f64 / 32768.0
                        + input[2] * p.mix.noise_gain as f64 / 32768.0
                }
                Kind::Sum => {
                    input[0] * n.numbers[0] + input[1] * n.numbers[1] + input[2] * n.numbers[2]
                }
                Kind::Filter1 | Kind::Filter => {
                    let c = if n.filters.is_empty() {
                        p.filter
                    } else {
                        n.filters[(n.values[1] as f64 + input[3] * 63.0)
                            .round()
                            .clamp(0.0, 127.0) as usize]
                    };
                    state.filter.next_sample(audio, c).0 as f64 / 2147483647.0
                }
                Kind::Filter2 => {
                    if let Some(r) = p.routing {
                        let mut c = r.second;
                        if n.inputs[3].is_some() {
                            c.integrator_gain = (c.integrator_gain as f64
                                * 2.0_f64.powf(input[3] * 4.0))
                            .clamp(0.0, 2147483647.0)
                                as i32;
                        }
                        let x = if c.output == Filter2Output::Comb {
                            state.comb.as_mut().unwrap().sample(
                                audio,
                                c.feedback,
                                c.integrator_gain as u32,
                            )
                        } else {
                            state.second.sample(audio, c)
                        };
                        x.0 as f64 / 2147483647.0
                    } else {
                        input[0]
                    }
                }
                Kind::Drive | Kind::Shaper => {
                    let sh = if n.kind == Kind::Drive {
                        p.shaper
                    } else {
                        n.program
                            .controls
                            .shaper
                            .parameters_with_pitch(p.primary_pitch_code)
                    };
                    sh.map_or(input[0], |mut s| {
                        if n.kind == Kind::Shaper && s.coefficients.gain_current().is_some() {
                            if self.frames & 3 == 3 {
                                if let Some(target) =
                                    s.coefficients.gain_target(p.primary_pitch_code)
                                {
                                    state.shaper_gain =
                                        radias_synth_domain::control_slew::SlewWeights {
                                            target: 0x1d4,
                                            memory: 0x7e2d,
                                        }
                                        .word(state.shaper_gain, target);
                                }
                            }
                            s.coefficients.set_gain_current(state.shaper_gain);
                        }
                        state
                            .shaper
                            .process(
                                ShaperSignal {
                                    input: audio,
                                    primary_pitch_code: p.primary_pitch_code,
                                    primary_increment: p.primary.base_increment(),
                                },
                                s.coefficients,
                                &table.shapers,
                            )
                            .0 as f64
                            / 2147483647.0
                    })
                }
                Kind::Amp => multiply_q15(audio.0, level) as f64 / 2147483647.0 * gain,
                Kind::Gain => input[0] * n.numbers[0] * gain,
                Kind::Eg1 => self.controls[3],
                Kind::Eg2 => level as f64 / 32768.0,
                Kind::Eg3 => self.controls[4],
                Kind::Lfo1 => self.controls[5],
                Kind::Lfo2 => self.controls[6],
                Kind::Gate => self.controls[0],
                Kind::Velocity => self.controls[1],
                Kind::Output => input[0],
                Kind::Osc => {
                    let ratio = 2.0_f64.powf((n.numbers[0] + input[3] * 24.0) / 12.0);
                    let increment = PhaseIncrement(
                        (p.primary.base_increment().0 as f64 * ratio).clamp(0.0, u32::MAX as f64)
                            as u32,
                    );
                    let primary = n
                        .program
                        .controls
                        .primary()
                        .compile_waveform(increment, 0)
                        .unwrap();
                    state
                        .osc
                        .next_with_modulator_and_bias(table, primary, Sample(0), 0)
                        .0 as f64
                        / 2147483647.0
                        * n.numbers[1]
                }
                Kind::Lfo => {
                    state.phase =
                        (state.phase + n.numbers[0] * 2.0_f64.powf(input[3] * 4.0) / 48000.0) % 1.0;
                    let x = state.phase;
                    let value = match n.numbers[1] as u8 {
                        1 => 1.0 - 4.0 * (x - 0.5).abs(),
                        2 => {
                            if x < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        3 => 2.0 * x - 1.0,
                        _ => (x * std::f64::consts::TAU).sin(),
                    };
                    value * n.numbers[2]
                }
                Kind::Envelope => {
                    let active = gate && (n.inputs[3].is_none() || input[3] > 0.0);
                    let program = ModEnvelopeProgram {
                        adsr: [
                            n.numbers[0] as u8,
                            n.numbers[1] as u8,
                            n.numbers[2] as u8,
                            n.numbers[3] as u8,
                        ],
                        ..Default::default()
                    };
                    let parameters = program.parameters(
                        self.controls[2].clamp(0.0, 127.0) as u8,
                        (self.controls[1] * 127.0) as u8,
                    );
                    if active && (!state.env_started || !state.env_gate) {
                        state.env.note_on(
                            parameters,
                            &self.plan.controllers.curves,
                            &self.plan.controllers.timing,
                            0,
                        );
                        state.env_started = true;
                    }
                    if !active && state.env_gate {
                        state.env.release(parameters, &self.plan.controllers.timing);
                    }
                    state.env_gate = active;
                    if self.frames.is_multiple_of(24) {
                        state.env.publish();
                        state.env.tick(
                            parameters,
                            &self.plan.controllers.curves,
                            &self.plan.controllers.timing,
                        );
                    }
                    state.env.envelope.segment.level as f64 / 65535.0
                }
            };
            out[n.id] = if result.is_finite() {
                result.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        self.frames += 1;
        let release = if !gate
            && !self.plan.has_amp
            && !self
                .plan
                .order
                .iter()
                .any(|i| self.plan.nodes[*i].kind == Kind::Envelope)
        {
            (1.0 - self.release_frames as f64 / 2400.0).max(0.0)
        } else {
            1.0
        };
        Sample(saturate(
            (out[self.plan.output] * 2147483647.0 * release) as i64,
        ))
    }
}
