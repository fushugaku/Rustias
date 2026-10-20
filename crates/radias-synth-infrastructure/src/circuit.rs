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
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
mod osc3;
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
mod expanded;
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
pub mod filters;
#[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
pub mod motion;
mod primary;
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
    #[serde(default = "default_output")]
    pub output: String,
}
fn default_output() -> String {
    "out".into()
}
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Osc1,
    Osc2,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    Osc3,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    Osc4,
    #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
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
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    Lfo3,
    Gate,
    Velocity,
    Output,
    Osc,
    Primary,
    Filter,
    Shaper,
    Gain,
    Sum,
    Lfo,
    Envelope,
    Switch,
}
fn kind(name: &str) -> Option<Kind> {
    Some(match name {
        "osc1" => Kind::Osc1,
        "osc2" => Kind::Osc2,
        "noise" => {
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            {
                Kind::Osc3
            }
            #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
            {
                Kind::Noise
            }
        }
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        "osc3" => Kind::Osc3,
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        "osc4" => Kind::Osc4,
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
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        "lfo3" => Kind::Lfo3,
        "gate" => Kind::Gate,
        "velocity" => Kind::Velocity,
        "output" => Kind::Output,
        "oscillator" => Kind::Osc,
        "oscillator1" => Kind::Primary,
        "filter" => Kind::Filter,
        "shaper" => Kind::Shaper,
        "vca" => Kind::Gain,
        "sum" => Kind::Sum,
        "lfo" => Kind::Lfo,
        "envelope" => Kind::Envelope,
        "switch" => Kind::Switch,
        _ => return None,
    })
}
fn cv(k: Kind) -> bool {
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    if k == Kind::Lfo3 {
        return true;
    }
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
fn output_port(k: Kind, name: &str) -> Option<(usize, bool)> {
    match (k, name) {
        (Kind::Switch, "a") => Some((0, false)),
        (Kind::Switch, "b") => Some((1, false)),
        (Kind::Switch | Kind::Output, _) => None,
        (_, "out") => Some((0, cv(k))),
        _ => None,
    }
}
fn port(k: Kind, name: &str) -> Option<(usize, bool)> {
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    if matches!(k, Kind::Osc3 | Kind::Osc4) {
        return match name {
            "mod" => Some((0, false)),
            "ctrl1" => Some((1, true)),
            "ctrl2" => Some((2, true)),
            "pitch" => Some((3, true)),
            "lfo" => Some((4, true)),
            _ => None,
        };
    }
    match (k, name) {
        (Kind::Mix | Kind::Sum, "a") => Some((0, false)),
        (Kind::Mix | Kind::Sum, "b") => Some((1, false)),
        (Kind::Mix | Kind::Sum, "c") => Some((2, false)),
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        (Kind::Mix, "d") => Some((3, false)),
        (
            Kind::Filter1
            | Kind::Filter2
            | Kind::Filter
            | Kind::Drive
            | Kind::Shaper
            | Kind::Amp
            | Kind::Gain
            | Kind::Switch
            | Kind::Output,
            "in",
        ) => Some((0, false)),
        (Kind::Filter1 | Kind::Filter2 | Kind::Filter, "cutoff") => Some((3, true)),
        (Kind::Amp | Kind::Gain, "gain") => Some((3, true)),
        (Kind::Osc | Kind::Primary, "pitch") => Some((3, true)),
        (Kind::Primary, "mod") => Some((0, false)),
        (Kind::Primary, "ctrl1") => Some((1, true)),
        (Kind::Primary, "ctrl2") => Some((2, true)),
        (Kind::Primary, "lfo") => Some((4, true)),
        (Kind::Lfo, "rate") => Some((3, true)),
        (Kind::Envelope, "gate") => Some((3, true)),
        (Kind::Switch, "select") => Some((3, true)),
        _ => None,
    }
}
#[derive(Clone, Copy)]
struct Signal {
    id: usize,
    output: usize,
}
struct Node {
    id: usize,
    kind: Kind,
    inputs: [Option<Signal>; 5],
    values: Values,
    numbers: [f64; 6],
    filters: Vec<radias_synth_domain::filter::FilterCoefficients>,
    program: radias_synth_application::drum_program::DrumInstrumentProgram,
}
struct Plan {
    fixed: bool,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fine: radias_synth_domain::controller_secondary::FineTuneTable,
    nodes: Vec<Node>,
    order: Vec<usize>,
    output: usize,
    controllers: Box<ControllerTables>,
    has_amp: bool,
    primary_tables: Option<Box<primary::Tables>>,
}
pub struct CircuitVoice {
    plan: Arc<Plan>,
    states: Vec<State>,
    controls: [f64; 8],
    frames: u64,
    release_frames: u32,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    third: State,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    third_program: radias_synth_application::primary::ThirdProgram,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    third_targets: radias_synth_domain::modulation::AppliedModulationTargets,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    source_taps: radias_synth_domain::voice::OscillatorTaps,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    previous_phases: Option<[radias_synth_domain::Phase; 2]>,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    source_wraps: [bool; 2],
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    expanded: expanded::Expanded,
}
struct State {
    osc: PrimaryOscillator,
    primary: Option<Box<primary::Controller>>,
    filter: ResonantFilter,
    second: Filter2,
    shaper: Waveshaper,
    shaper_gain: i16,
    comb: Option<Box<radias_synth_domain::comb::Comb>>,
    env: ModEnvelope,
    env_started: bool,
    env_gate: bool,
    phase: f64,
    switch_mix: Option<f64>,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    third_pitch: Option<osc3::Pitch>,
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    modeled: filters::Model,
}
impl Default for State {
    fn default() -> Self {
        Self {
            osc: Default::default(),
            primary: None,
            filter: Default::default(),
            second: Default::default(),
            shaper: Default::default(),
            shaper_gain: 0,
            comb: None,
            env: Default::default(),
            env_started: false,
            env_gate: false,
            phase: 0.0,
            switch_mix: None,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            third_pitch: None,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            modeled: Default::default(),
        }
    }
}
impl CircuitVoice {
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    pub fn fixed() -> Self {
        Self::new(Arc::new(Plan {
            fixed: true,
            fine: crate::standalone_tables::fine(),
            nodes: Vec::new(),
            order: Vec::new(),
            output: 0,
            controllers: Box::new(crate::standalone_tables::controllers()),
            has_amp: true,
            primary_tables: Some(Box::new(primary::Tables::new())),
        }))
    }
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
                Kind::Primary => {
                    for (id, key, default, max) in [
                        (0, "wave", 0.0, 5.0),
                        (10, "mode", 0.0, 3.0),
                        (11, "ctrl1", 0.0, 127.0),
                        (12, "ctrl2", 0.0, 127.0),
                    ] {
                        let v = get(key, default, 0.0, max)?;
                        if v.fract() != 0.0 {
                            return Err(format!("Invalid {key}."));
                        }
                        values[id] = v as i32;
                    }
                    if values[0] >= 4 && values[10] != 0 {
                        return Err("Noise/Formant use Waveform mode.".into());
                    }
                    numbers[0] = get("semitone", 0.0, -48.0, 48.0)?;
                    numbers[1] = get("level", 64.0, 0.0, 127.0)? / 127.0;
                    numbers[2] = get("fine", 0.0, -100.0, 100.0)?;
                }
                Kind::Filter => {
                    values[1] = get("cutoff", 96.0, 0.0, 127.0)? as i32;
                    values[2] = get("resonance", 0.0, 0.0, 127.0)? as i32;
                    values[9] = get("morph", 0.0, 0.0, 127.0)? as i32;
                    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                    { values[189] = get("model",0.0,0.0,17.0)? as i32;
                      values[191] = get("drive",0.0,0.0,127.0)? as i32;
                      values[193] = get("modelMorph",0.0,0.0,127.0)? as i32; }
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
                Kind::Switch => {
                    numbers[0] = get("route", 0.0, 0.0, 1.0)?;
                    if numbers[0].fract() != 0.0 {
                        return Err("Invalid route.".into());
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
                inputs: [None; 5],
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
            let (channel, source_control) =
                output_port(nodes[a].kind, &wire.output).ok_or("Unknown output port.")?;
            if control != source_control {
                return Err("Connect audio to audio and CV to CV.".into());
            }
            if nodes[b].inputs[p]
                .replace(Signal {
                    id: wire.from,
                    output: channel,
                })
                .is_some()
            {
                return Err("Each input accepts one cable; use a Mixer to combine signals.".into());
            }
        }
        let mut order = Vec::new();
        let mut done = [false; LIMIT];
        while order.len() < nodes.len() {
            let before = order.len();
            for (i, n) in nodes.iter().enumerate() {
                if !done[n.id] && n.inputs.iter().flatten().all(|s| done[s.id]) {
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
            stack.extend(
                nodes[ids[id].unwrap()]
                    .inputs
                    .iter()
                    .flatten()
                    .map(|s| s.id),
            );
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
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            {
                let mut voice = Self::fixed();
                voice.expanded.parameters(base);
                return Ok(Some(voice));
            }
            #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
            {
                return Ok(None);
            }
        }
        let has_amp = order
            .iter()
            .any(|i| matches!(nodes[*i].kind, Kind::Amp | Kind::Eg2));
        let primary_tables = nodes
            .iter()
            .any(|n| n.kind == Kind::Primary || kind("osc3").is_some_and(|k| n.kind == k) || kind("osc4").is_some_and(|k| n.kind == k))
            .then(|| Box::new(primary::Tables::new()));
        let mut voice = Self::new(Arc::new(Plan {
            fixed: false,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            fine: crate::standalone_tables::fine(),
            nodes,
            order,
            output,
            controllers: Box::new(crate::standalone_tables::controllers()),
            has_amp,
            primary_tables,
        }));
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        voice.expanded.parameters(base);
        Ok(Some(voice))
    }
    fn new(plan: Arc<Plan>) -> Self {
        let mut states: Vec<State> = if plan.fixed {
            Vec::new()
        } else {
            (0..LIMIT).map(|_| State::default()).collect()
        };
        for n in &plan.nodes {
            if n.kind == Kind::Primary || kind("osc3").is_some_and(|k| n.kind == k) {
                states[n.id].primary = Some(Box::default());
            }
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
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            third: State {
                primary: Some(Box::default()),
                ..Default::default()
            },
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            third_program: Default::default(),
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            third_targets: Default::default(),
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            source_taps: Default::default(),
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            previous_phases: None,
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            source_wraps: [false; 2],
            #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
            expanded: Default::default(),
        }
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn third_reference(&self, sources: [Sample; 3]) -> (Sample, bool) {
        let (_, reference) = osc3::interaction(self.third_program, self.third_targets);
        let sample = match reference {
            0 => sources[0],
            1 if self.previous_phases.is_some() => self.source_taps.secondary_before_ring,
            _ => sources[1],
        };
        (sample, self.source_wraps[reference.min(1)])
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn expanded_fixed(&mut self, sources: [Sample;3], p:VoiceParameters, level:i16, table:&WaveformTable) -> Sample {
        let sources = self.prepare_sources(sources,p,table);
        let reference_id = self.expanded.value(186,self.third_targets).round().clamp(0.0,3.0) as usize;
        let reference = match reference_id {0=>sources[0],1=>self.source_taps.secondary_before_ring,2=>sources[1],_=>sources[2]};
        let wrap = if reference_id == 3 {
            let phase = self.third.osc.phase.0;
            let wrap = phase < self.expanded.previous_third_phase;
            self.expanded.previous_third_phase = phase;
            wrap
        } else {self.source_wraps[reference_id.min(1)]};
        let fourth_gain = self.expanded.next_gain(self.third_targets);
        let fourth = if fourth_gain != 0 {
            self.expanded.oscillator(self.third_targets,p,reference,wrap,sources[1],[0.0,0.0,0.0,0.0,self.controls[5]],self.plan.primary_tables.as_ref().unwrap(),&self.plan.fine,table,self.frames)
        } else {Sample(0)};
        let a = multiply_q15(sources[0].0,p.mix.primary_gain);
        let b = multiply_q15(sources[1].0,p.mix.secondary_gain) + multiply_q15(sources[2].0,p.mix.noise_gain)
            + multiply_q15(fourth.0,fourth_gain);
        let mixed = Sample(saturate(a+b));
        let shape = |state:&mut Waveshaper, x:Sample, before:bool| {
            p.shaper.filter(|s| (s.position == radias_synth_domain::waveshaper::ShaperPosition::PreFilter) == before).map_or(x, |s| state.process(ShaperSignal {input:x,primary_pitch_code:p.primary_pitch_code,primary_increment:p.primary.base_increment()},s.coefficients,&table.shapers))
        };
        let route = p.routing.map(|r|r.route);
        let first_input = shape(&mut self.third.shaper, if route == Some(radias_synth_domain::filter_routing::FilterRouting::Individual) {Sample(saturate(a))} else {mixed},true);
        let model1 = self.expanded.value(189,self.third_targets).round().clamp(0.0,17.0) as u8;
        let model2 = self.expanded.value(190,self.third_targets).round().clamp(0.0,17.0) as u8;
        let first = if model1 == 0 { self.third.filter.next_sample(first_input,p.filter) } else {
            let resonance = (self.expanded.values[2] as f64 + self.third_targets.controls[6] as f64).clamp(0.0,127.0)/127.0;
            let x = self.expanded.models[0].sample_coefficient(first_input.0 as f64/2147483648.0,model1,p.filter.integrator_gain,resonance,
                self.expanded.value(191,self.third_targets)/127.0,self.expanded.value(193,self.third_targets)/127.0);
            Sample((x.clamp(-1.0,1.0)*2147483647.0) as i32)
        };
        let filtered = if let Some(r) = p.routing {
            use radias_synth_domain::filter_routing::FilterRouting;
            let input = match r.route {FilterRouting::Serial=>first,FilterRouting::Parallel=>mixed,FilterRouting::Individual=>Sample(saturate(b))};
            let second = if model2 == 0 {
                if r.second.output == Filter2Output::Comb {
                    self.third.comb.get_or_insert_with(||Box::new(radias_synth_domain::comb::Comb {feedback:Default::default(),delay:Default::default()})).sample(input,r.second.feedback,r.second.integrator_gain as u32)
                } else {self.third.second.sample(input,r.second)}
            } else {
                let resonance = (self.expanded.values[23] as f64+self.third_targets.controls[17] as f64).clamp(0.0,127.0)/127.0;
                let x = self.expanded.models[1].sample_coefficient(input.0 as f64/2147483648.0,model2,r.second.integrator_gain,resonance,
                    self.expanded.value(192,self.third_targets)/127.0,self.expanded.value(194,self.third_targets)/127.0);
                Sample((x.clamp(-1.0,1.0)*2147483647.0) as i32)
            };
            match r.route {FilterRouting::Serial=>second,FilterRouting::Parallel=>Sample(saturate((first.0 as i64+second.0 as i64)>>1)),FilterRouting::Individual=>Sample(saturate(first.0 as i64+second.0 as i64))}
        } else {first};
        Sample(saturate(multiply_q15(shape(&mut self.third.shaper,filtered,false).0,level)))
    }
}
impl VoiceCircuit for CircuitVoice {
    fn fresh(&self) -> Box<dyn VoiceCircuit> {
        let mut next = Self::new(self.plan.clone());
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        { next.expanded = self.expanded.fresh(); }
        Box::new(next)
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn extension_parameters(&mut self, values: &[i32]) { self.expanded.parameters(values); }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn extension_instrument(&mut self, values:&[i32;207]) {
        let tempo=self.expanded.values[89];self.expanded.parameters(values);self.expanded.values[89]=tempo;
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn extension_tempo(&mut self, tempo:i32) {self.expanded.values[89]=tempo;}
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn inherit_extensions(&mut self, other: &dyn VoiceCircuit) {
        if let Some(other) = other.as_any().downcast_ref::<Self>() { self.expanded.inherit(&other.expanded); }
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_program(&mut self, program: Arc<radias_synth_application::motion::Program>) { self.expanded.motion(program); }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_seed(&mut self, seed:u32) { self.expanded.motion.seed=seed.max(1); }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_tempo(&mut self, tempo:f64) {self.expanded.tempo=Some(tempo);}
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_status(&self)->(u64,[i16;6]) { (self.frames,self.expanded.motion.positions) }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_resonance(&self, offset:i16)->Option<i32> {
        Some(((self.expanded.values[2] as i32+offset as i32).clamp(0,127) as f64/127.0*0.95*2147483647.0) as i32)
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_envelopes(&mut self)->Option<[ModEnvelopeProgram;3]> {self.expanded.envelopes()}
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn prepare_motion(&mut self) {
        let _=self.motion_targets(Default::default());
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn set_motion_offsets(&mut self,offsets:&[f64;207],positions:[i16;6]) {
        self.expanded.motion.offsets=*offsets;self.expanded.motion.positions=positions;
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn motion_targets(&mut self, targets: radias_synth_domain::modulation::AppliedModulationTargets) -> radias_synth_domain::modulation::AppliedModulationTargets {
        self.expanded.motion.tick(self.frames, self.expanded.tempo.unwrap_or(self.expanded.values[89] as f64 / 10.0), targets)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn reconfigure(&mut self, prototype: &dyn VoiceCircuit) {
        if let Some(next) = prototype.as_any().downcast_ref::<Self>() {
            let from_fixed = self.plan.fixed;
            if next.plan.fixed {
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                if !from_fixed
                    && let Some(n) = self.plan.nodes.iter().find(|n| n.kind == Kind::Osc3)
                {
                    self.third = std::mem::take(&mut self.states[n.id]);
                }
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                if self.third.primary.is_none() {
                    self.third.primary = Some(Box::default());
                }
                self.states.clear();
                self.plan = next.plan.clone();
                return;
            }
            if from_fixed {
                self.states.resize_with(LIMIT, State::default);
            }
            for n in &next.plan.nodes {
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                if from_fixed && n.kind == Kind::Osc3 && self.third.primary.is_some() {
                    self.states[n.id] = std::mem::take(&mut self.third);
                    continue;
                }
                if !self
                    .plan
                    .nodes
                    .iter()
                    .any(|old| old.id == n.id && old.kind == n.kind)
                {
                    self.states[n.id] = State::default();
                    if n.kind == Kind::Primary || kind("osc3").is_some_and(|k| n.kind == k) {
                        self.states[n.id].primary = Some(Box::default());
                    }
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
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn oscillator3(
        &mut self,
        program: radias_synth_application::primary::ThirdProgram,
        targets: radias_synth_domain::modulation::AppliedModulationTargets,
    ) {
        self.third_program = program;
        self.third_targets = targets;
    }
    fn tail_active(&self) -> bool {
        if self.plan.fixed {
            return false;
        }
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
    fn fixed_graph(&self) -> bool {
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        if self.expanded.enabled() || self.third_targets.controls[59..].iter().any(|v|*v!=0) { return false; }
        self.plan.fixed
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn needs_oscillator_taps(&self) -> bool {
        let fourth_mode = self.expanded.value(185,self.third_targets).round() as u8;
        let fourth_source = self.expanded.value(186,self.third_targets).round() as u8;
        if fourth_mode & 1 != 0 && fourth_source == 1 && (self.expanded.gain(self.third_targets)!=0 || self.plan.order.iter().any(|i|self.plan.nodes[*i].kind==Kind::Osc4)) {return true;}
        let (mode, reference) = osc3::interaction(self.third_program, self.third_targets);
        mode.ring
            && reference == 1
            && (self.plan.fixed
                || self
                    .plan
                    .order
                    .iter()
                    .any(|i| self.plan.nodes[*i].kind == Kind::Osc3))
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn oscillator_taps(&mut self, taps: radias_synth_domain::voice::OscillatorTaps) {
        let sync = radias_synth_domain::secondary_control::SecondaryModulation {
            ring: false,
            sync: true,
        };
        self.source_wraps = core::array::from_fn(|i| {
            self.previous_phases
                .is_some_and(|previous| sync.wraps(taps.phases[i], previous[i]))
        });
        self.previous_phases = Some(taps.phases);
        self.source_taps = taps;
    }
    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
    fn prepare_sources(
        &mut self,
        mut sources: [Sample; 3],
        p: VoiceParameters,
        table: &WaveformTable,
    ) -> [Sample; 3] {
        let (reference, wrap) = self.third_reference(sources);
        sources[2] = osc3::next(
            &mut self.third.osc,
            self.third.primary.as_mut().unwrap(),
            &mut self.third.third_pitch,
            self.third_program,
            self.third_targets,
            p,
            [0.0, 0.0, 0.0, 0.0, self.controls[5]],
            sources[1],
            reference,
            wrap,
            self.plan.primary_tables.as_ref().unwrap(),
            &self.plan.fine,
            table,
            self.frames,
            2,
            2,
        );
        self.frames = self.frames.wrapping_add(1);
        sources
    }
    fn process(
        &mut self,
        sources: [Sample; 3],
        p: VoiceParameters,
        level: i16,
        table: &WaveformTable,
    ) -> Sample {
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        if self.plan.fixed { return self.expanded_fixed(sources, p, level, table); }
        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
        let (reference, wrap) = self.third_reference(sources);
        let mut out = [[0.0; 2]; LIMIT];
        let gate = self.controls[0] > 0.0;
        if !gate {
            self.release_frames = self.release_frames.saturating_add(1);
        } else {
            self.release_frames = 0;
        }
        for &i in &self.plan.order {
            let n = &self.plan.nodes[i];
            let state = &mut self.states[n.id];
            let input = n.inputs.map(|s| s.map_or(0.0, |s| out[s.id][s.output]));
            let gain = if n.inputs[3].is_some() { input[3] } else { 1.0 };
            let audio = Sample((input[0] * 2147483647.0) as i32);
            let result = match n.kind {
                Kind::Osc1 => sources[0].0 as f64 / 2147483647.0,
                Kind::Osc2 => sources[1].0 as f64 / 2147483647.0,
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                Kind::Osc3 => {
                    let mut cv = input;
                    if n.inputs[4].is_none() {
                        cv[4] = self.controls[5];
                    }
                    osc3::next(
                        &mut state.osc,
                        state.primary.as_mut().unwrap(),
                        &mut state.third_pitch,
                        self.third_program,
                        self.third_targets,
                        p,
                        cv,
                        if n.inputs[0].is_some() {
                            audio
                        } else {
                            sources[1]
                        },
                        reference,
                        wrap,
                        self.plan.primary_tables.as_ref().unwrap(),
                        &self.plan.fine,
                        table,
                        self.frames,
                        n.id,
                        2,
                    )
                    .0 as f64
                        / 2147483647.0
                }
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                Kind::Osc4 => {
                    let ref_id = self.expanded.value(186,self.third_targets).round().clamp(0.0,3.0) as usize;
                    let third_node = self.plan.nodes.iter().find(|n|n.kind==Kind::Osc3).map(|n|n.id);
                    let source = match ref_id {0=>sources[0],1=>self.source_taps.secondary_before_ring,2=>sources[1],_=>Sample((third_node.map_or(0.0,|id|out[id][0])*2147483647.0) as i32)};
                    let wrap = if ref_id==3 {
                        let phase=third_node.map_or(0,|id|self.states[id].osc.phase.0);
                        let wrap=phase<self.expanded.previous_third_phase;self.expanded.previous_third_phase=phase;wrap
                    } else {self.source_wraps[ref_id.min(1)]};
                    self.expanded.oscillator(self.third_targets,p,source,wrap,if n.inputs[0].is_some() {audio}else{sources[1]},input,self.plan.primary_tables.as_ref().unwrap(),&self.plan.fine,table,self.frames).0 as f64 / 2147483647.0
                }
                #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
                Kind::Noise => sources[2].0 as f64 / 2147483647.0,
                Kind::Mix => {
                    let mix = input[0] * p.mix.primary_gain as f64 / 32768.0
                        + input[1] * p.mix.secondary_gain as f64 / 32768.0
                        + input[2] * p.mix.noise_gain as f64 / 32768.0;
                    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                    let mix = mix + input[3] * self.expanded.next_gain(self.third_targets) as f64 / 32768.0;
                    mix
                }
                Kind::Sum => {
                    input[0] * n.numbers[0] + input[1] * n.numbers[1] + input[2] * n.numbers[2]
                }
                Kind::Switch => {
                    let target = if n.inputs[3].is_some() {
                        if input[3] >= 0.5 { 1.0 } else { 0.0 }
                    } else {
                        n.numbers[0]
                    };
                    // Start on the chosen route. Live changes crossfade over 5 ms,
                    // preserving all oscillator, filter and envelope states.
                    let mix = state.switch_mix.get_or_insert(target);
                    *mix += (target - *mix).clamp(-1.0 / 240.0, 1.0 / 240.0);
                    out[n.id][1] = input[0] * *mix;
                    input[0] * (1.0 - *mix)
                }
                Kind::Filter1 | Kind::Filter => {
                    let c = if n.filters.is_empty() {
                        p.filter
                    } else {
                        n.filters[(n.values[1] as f64 + input[3] * 63.0)
                            .round()
                            .clamp(0.0, 127.0) as usize]
                    };
                    #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                    {
                        let (model,drive,morph,resonance) = if n.kind == Kind::Filter {
                            (n.values[189] as u8,n.values[191] as f64/127.0,n.values[193] as f64/127.0,n.values[2] as f64/127.0)
                        } else { (self.expanded.value(189,self.third_targets).round().clamp(0.0,17.0) as u8,self.expanded.value(191,self.third_targets)/127.0,self.expanded.value(193,self.third_targets)/127.0,(self.expanded.values[2] as f64+self.third_targets.controls[6] as f64).clamp(0.0,127.0)/127.0) };
                        if model != 0 {state.modeled.sample_coefficient(input[0],model,c.integrator_gain,resonance,drive,morph)}
                        else {state.filter.next_sample(audio,c).0 as f64/2147483647.0}
                    }
                    #[cfg(not(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin))))]
                    { state.filter.next_sample(audio, c).0 as f64 / 2147483647.0 }
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
                        #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                        if self.expanded.value(190,self.third_targets)>=0.5 {
                            out[n.id][0]=state.modeled.sample_coefficient(input[0],self.expanded.value(190,self.third_targets).round().clamp(0.0,17.0) as u8,c.integrator_gain,(self.expanded.values[23] as f64+self.third_targets.controls[17] as f64).clamp(0.0,127.0)/127.0,self.expanded.value(192,self.third_targets)/127.0,self.expanded.value(194,self.third_targets)/127.0);
                            continue;
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
                #[cfg(all(feature = "web-expanded", any(target_arch = "wasm32", rustias_plugin)))]
                Kind::Lfo3 => self.controls[7],
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
                Kind::Primary => {
                    let semitones = n.numbers[0] + n.numbers[2] / 100.0 + input[3] * 24.0;
                    let increment = PhaseIncrement(
                        (p.primary.base_increment().0 as f64 * 2.0_f64.powf(semitones / 12.0))
                            .clamp(0.0, i32::MAX as f64) as u32,
                    );
                    let code = radias_synth_domain::pitch::PitchCode::new(
                        (p.primary_pitch_code as f64 + semitones * 256.0)
                            .round()
                            .clamp(0.0, 32767.0) as u16,
                    )
                    .unwrap();
                    state
                        .primary
                        .as_mut()
                        .unwrap()
                        .next(
                            &mut state.osc,
                            n.program.controls.primary(),
                            increment,
                            code,
                            ((input[4] * 32768.0).round().clamp(-32768.0, 32767.0) as i16) >> 8,
                            [
                                (input[1] * 127.0).round().clamp(-127.0, 127.0) as i16,
                                (input[2] * 127.0).round().clamp(-127.0, 127.0) as i16,
                            ],
                            audio,
                            self.plan.primary_tables.as_ref().unwrap(),
                            table,
                            self.frames,
                            n.id,
                        )
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
            out[n.id][0] = if result.is_finite() {
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
            (out[self.plan.output][0] * 2147483647.0 * release) as i64,
        ))
    }
}
