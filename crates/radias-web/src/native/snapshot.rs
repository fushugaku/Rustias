//! Owned, preallocated snapshots. Only capture touches the live engine; JSON
//! formatting runs later on the message worker with exclusive snapshot ownership.
use super::*;
use modulation::Target;
use serde_json::json;
use std::sync::Arc;

pub(super) struct Layout {
    targets: Vec<Target>,
    profiles: Vec<(usize, String, u32)>,
    chains: Vec<Vec<String>>,
}
pub(super) struct MotionStatus {
    pub running: bool,
    pub tracks: usize,
    pub lengths: [usize; TIMBRE_COUNT],
    pub positions: [i32; TIMBRE_COUNT * 6],
    pub macros: [Option<f64>; 8],
    pub outputs: [Option<f64>; 8 * 6 + 96],
}
#[repr(C)]
pub struct SnapshotMetadata {
    pub callbacks: u64,
    pub audible_frames: u64,
    pub peak: f64,
    pub has_midi: u32,
    pub volume_mask: u32,
    pub midi: [u8; 3],
    pub volumes: [[u8; 3]; 16],
    pub failed: u8,
}
pub struct NativeSnapshot {
    layout: Arc<Layout>,
    kind: u32,
    timbres: [[i32; PARAMETER_COUNT]; TIMBRE_COUNT],
    drums: [[i32; PARAMETER_COUNT]; 16],
    slots: [effects::Program; EFFECT_SLOTS],
    chains: Vec<effects::Program>,
    volumes: Vec<i32>,
    frames: f64,
    voices: f64,
    capacity: f64,
    motion: MotionStatus,
    bytes: Vec<u8>,
}
impl NativeHost {
    pub fn new_snapshot(&mut self) -> NativeSnapshot {
        let _scope = Scope(ACTIVE.with(|active| active.replace(self)));
        let layout = self
            .snapshot_layout
            .get_or_insert_with(|| {
                let (targets, profiles) = self.modulation.borrow().snapshot_layout();
                let mut chains = vec![vec![]; TIMBRE_COUNT];
                if let Some(e) = self.engine.borrow().as_ref() {
                    for (t, id, _) in e.effects.chain_program_entries() {
                        chains[t].push(id.to_owned());
                    }
                }
                Arc::new(Layout {
                    targets,
                    profiles,
                    chains,
                })
            })
            .clone();
        let off = effects::Program {
            kind: 0,
            enabled: false,
            master: false,
            parameters: [0; 20],
        };
        NativeSnapshot {
            volumes: vec![0; layout.profiles.len()],
            chains: vec![off; layout.chains.iter().map(Vec::len).sum()],
            layout,
            kind: 0,
            timbres: [[0; PARAMETER_COUNT]; TIMBRE_COUNT],
            drums: [[0; PARAMETER_COUNT]; 16],
            slots: [off; EFFECT_SLOTS],
            frames: 0.,
            voices: 0.,
            capacity: 0.,
            motion: MotionStatus {
                running: false,
                tracks: 0,
                lengths: [0; TIMBRE_COUNT],
                positions: [-1; TIMBRE_COUNT * 6],
                macros: [None; 8],
                outputs: [None; 8 * 6 + 96],
            },
            bytes: Vec::new(),
        }
    }
    pub fn capture_snapshot(&mut self, s: &mut NativeSnapshot, kind: u32) -> bool {
        if kind > 2
            || !self
                .snapshot_layout
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(p, &s.layout))
        {
            return false;
        }
        self.region = None;
        let _scope = Scope(ACTIVE.with(|active| active.replace(self)));
        s.kind = kind;
        if kind == 0 {
            self.modulation.borrow().capture_status(&mut s.motion);
            for t in 0..TIMBRE_COUNT {
                for lane in 0..6 {
                    let position = rustias_poly_motion_position(t as u32, lane as u32);
                    if position >= 0 {
                        s.motion.positions[t * 6 + lane] = position;
                        s.motion.tracks = s.motion.tracks.max(t + 1);
                        s.motion.lengths[t] = s.motion.lengths[t].max(lane + 1);
                        s.motion.running = true;
                    }
                }
            }
            s.frames = rustias_frames() as f64;
            s.voices = rustias_voices() as f64;
            s.capacity = rustias_voice_capacity() as f64;
        } else {
            // Match rustias_save's restoration exactly, including held offsets
            // being reapplied at the next fragment without advancing any clock.
            self.modulation.borrow_mut().restore();
            let engine = self.engine.borrow();
            let Some(e) = engine.as_ref() else {
                return false;
            };
            s.timbres.copy_from_slice(&e.synth.settings);
            s.drums.copy_from_slice(&e.synth.drum_settings);
            if kind == 1 {
                s.slots = e.effects.programs().map(Into::into);
                for (out, (_, _, program)) in
                    s.chains.iter_mut().zip(e.effects.chain_program_entries())
                {
                    *out = program.into();
                }
            }
            drop(engine);
            for (out, (_, _, id)) in s.volumes.iter_mut().zip(&s.layout.profiles) {
                *out = rustias_library_value(*id, 117);
            }
        }
        true
    }
}
impl NativeSnapshot {
    pub fn format(&mut self, meta: &SnapshotMetadata) -> &[u8] {
        let value = if self.kind == 0 {
            let positions: Vec<Vec<i32>> = (0..self.motion.tracks)
                .map(|t| self.motion.positions[t * 6..t * 6 + self.motion.lengths[t]].to_vec())
                .collect();
            let values: Vec<_> = self
                .layout
                .targets
                .iter()
                .zip(&self.motion.outputs)
                .filter_map(|(target, value)| {
                    value.map(|value| json!({"target":target,"value":value}))
                })
                .collect();
            json!({"type":"stats","frames":self.frames,"voices":self.voices,"capacity":self.capacity,"failed":meta.failed!=0,"backend":"native-rust",
                "modulation":{"running":self.motion.running,"positions":positions,"macros":self.motion.macros,"values":values,"backend":"native-rust"},
                "audibleFrames":meta.audible_frames,"peak":meta.peak,"callbacks":meta.callbacks,"sequence":null})
        } else {
            let mut program = json!({"version":1,"timbres":self.timbres.iter().map(|p|p.as_slice()).collect::<Vec<_>>(),"drums":self.drums.iter().map(|p|p.as_slice()).collect::<Vec<_>>()});
            if self.kind == 1 {
                let mut at = 0;
                let chains: Vec<Vec<_>> = self.layout.chains.iter().map(|chain| chain.iter().map(|id| {
                    let p = self.chains[at]; at += 1;
                    json!({"id":id,"kind":p.kind,"enabled":p.enabled,"master":p.master,"parameters":p.parameters})
                }).collect()).collect();
                program["effects"] =
                    json!({"version":1,"slots":self.slots.as_slice(),"chains":chains});
            }
            let volumes: Vec<_> = self.layout.profiles.iter().zip(&self.volumes).map(|((t,source,_),value)| json!({"timbre":t,"source":source,"value":*value as f64})).collect();
            let mut state = json!({"type":"state","program":program,"libraryVolumes":volumes});
            if meta.has_midi != 0 {
                state["midi"] = json!(meta.midi);
                state["midiBatch"] = json!(
                    (0..16)
                        .filter(|i| meta.volume_mask & (1 << i) != 0)
                        .map(|i| meta.volumes[i])
                        .collect::<Vec<_>>()
                );
            }
            state
        };
        self.bytes.clear();
        serde_json::to_writer(&mut self.bytes, &value).expect("Owned native snapshot JSON");
        &self.bytes
    }
}
