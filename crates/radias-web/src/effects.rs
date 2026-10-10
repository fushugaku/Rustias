//! Browser data port for the same effect rack used by the native player.
use radias_synth_application::{
    effect_audio::{EFFECT_SLOTS, EffectAudioRack, MASTER_EFFECT_SLOT, effect_uses_master},
    polyphony::TIMBRE_COUNT,
};
use radias_synth_domain::effect_audio::EffectAudioProgram;
use radias_synth_infrastructure::effect_audio::{definition, prepare_rack};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Program {
    pub kind: u8,
    pub enabled: bool,
    pub master: bool,
    pub parameters: [u8; 20],
}
impl From<EffectAudioProgram> for Program {
    fn from(p: EffectAudioProgram) -> Self {
        Self {
            kind: p.kind,
            enabled: p.enabled,
            master: p.master,
            parameters: p.parameters,
        }
    }
}
impl From<Program> for EffectAudioProgram {
    fn from(p: Program) -> Self {
        Self {
            kind: p.kind,
            enabled: p.enabled,
            master: p.master,
            parameters: p.parameters,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "InputState")]
pub struct State {
    pub version: u8,
    #[serde(serialize_with = "serialize_slots")]
    pub slots: [Program; EFFECT_SLOTS],
}
fn serialize_slots<S: serde::Serializer>(
    slots: &[Program; EFFECT_SLOTS],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    slots.as_slice().serialize(serializer)
}
#[derive(Deserialize)]
struct InputState {
    version: u8,
    slots: Vec<Program>,
}
impl TryFrom<InputState> for State {
    type Error = &'static str;
    fn try_from(input: InputState) -> Result<Self, Self::Error> {
        if input.version != 1
            || ![9, 2 * TIMBRE_COUNT + 1, EFFECT_SLOTS].contains(&input.slots.len())
            || input.slots.len() > EFFECT_SLOTS
        {
            return Err("Invalid effect rack");
        }
        let mut state = Self::default();
        let master = if input.slots.len() == EFFECT_SLOTS {
            MASTER_EFFECT_SLOT
        } else {
            input.slots.len() - 1
        };
        for (i, p) in input.slots.into_iter().enumerate() {
            state.slots[if i == master { MASTER_EFFECT_SLOT } else { i }] = p;
        }
        Ok(state)
    }
}
impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            slots: core::array::from_fn(|i| Program {
                kind: 0,
                enabled: false,
                master: effect_uses_master(i),
                parameters: [0; 20],
            }),
        }
    }
}
impl State {
    pub fn prepare(&self, tempo: u16) -> Result<Box<EffectAudioRack>, String> {
        if self.version != 1 {
            return Err("Invalid effect program version".into());
        }
        prepare_rack(self.slots.map(Into::into), tempo)
    }
    pub fn from_stored(
        p: &radias_synth_domain::program::Program,
        notices: &mut Vec<String>,
    ) -> Self {
        let mut state = Self {
            version: 1,
            slots: radias_synth_infrastructure::effect_audio::programs_from_stored(p)
                .map(Into::into),
        };
        for (slot, fx) in state.slots.iter_mut().enumerate() {
            let Some(def) = definition(fx.kind, fx.master) else {
                *fx = Self::default().slots[slot];
                notices.push(format!(
                    "FX {} has an unavailable type and was bypassed.",
                    slot + 1
                ));
                continue;
            };
            for (i, property) in def
                .properties
                .iter()
                .enumerate()
                .take(usize::from(def.count))
            {
                let decoded = i16::from(fx.parameters[i]) - i16::from(property.zero);
                let value = decoded.clamp(property.minimum, property.maximum);
                if value != decoded {
                    notices.push(format!(
                        "FX {} {} was limited to its supported range.",
                        slot + 1,
                        property.name
                    ));
                    fx.parameters[i] = (value + i16::from(property.zero)) as u8;
                }
            }
        }
        state
    }
}
pub fn catalog() -> serde_json::Value {
    let bank = |master| {
        (0..31).map(|kind|{
        let d=definition(kind,master).unwrap();
        serde_json::json!({"kind":kind,"name":d.name,"defaults":d.properties.map(|p|p.default),"properties":d.properties.iter().take(usize::from(d.count)).map(|p|serde_json::json!({"name":p.name,"min":p.minimum,"max":p.maximum,"zero":p.zero,"default":p.default})).collect::<Vec<_>>()})
    }).collect::<Vec<_>>()
    };
    serde_json::json!({"version":1,"insert":bank(false),"master":bank(true),"display":radias_synth_infrastructure::effect_audio::display_tables()})
}
