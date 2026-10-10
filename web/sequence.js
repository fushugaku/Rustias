import {MAX_TIMBRES,INITIAL_TIMBRES,timbreArray} from './limits.js';
import {validSampleSource} from "./sample-state.js";
// Browser-only polyphonic clock. It dispatches notes to the native Wasm engine.
export const STEPS = 128;
export const VIEW_STEPS = 16;
export const MAX_EVENTS = 128;
export const stepEvents=step=>[...step.notes,...(step.samples??[])];
// RADIAS P15 COMN Resolutn: a step is a note value relative to BPM.
export const RESOLUTIONS = ["1/32","1/24","3/64","1/16","1/12","3/32","1/8","1/6","3/16","1/4","1/3","3/8","1/2","2/3","3/4","1/1"];
export function stepFrames(bpm,resolution="1/16"){
  if(!RESOLUTIONS.includes(resolution))throw new Error("Invalid sequencer resolution.");
  const [numerator,denominator]=resolution.split("/").map(Number);return 48000*60/Math.max(10,Math.min(300,bpm))*4*numerator/denominator;
}
export function emptySequence() {
  return {version:1,tracks:Array.from({length:MAX_TIMBRES},(_,i)=>({enabled:i<INITIAL_TIMBRES,length:16,resolution:"1/16",steps:Array.from({length:STEPS},()=>({notes:[],samples:[],velocity:100,gate:75}))}))};
}
export function validateSequence(value) {
  if(value?.version!==1||!Array.isArray(value.tracks)||value.tracks.length<INITIAL_TIMBRES||value.tracks.length>MAX_TIMBRES)throw new Error("Invalid sequencer tracks.");
  const tracks=value.tracks.map(track=>{
    if(typeof track.enabled!=="boolean"||!Number.isInteger(track.length)||track.length<1||track.length>STEPS||!Array.isArray(track.steps)||![16,STEPS].includes(track.steps.length))throw new Error("Invalid sequencer track.");
    const resolution=track.resolution??"1/16";if(!RESOLUTIONS.includes(resolution))throw new Error("Invalid sequencer resolution.");
    const steps=track.steps.map(step=>{
      if(!Array.isArray(step.notes)||step.notes.length+(step.samples?.length??0)>MAX_EVENTS||step.notes.some(n=>!Number.isInteger(n)||n<0||n>127)||!Number.isInteger(step.velocity)||step.velocity<1||step.velocity>127||!Number.isInteger(step.gate)||step.gate<1||step.gate>100)throw new Error("Invalid sequencer step.");
      if(step.samples!=null&&(!Array.isArray(step.samples)||step.samples.some(source=>!validSampleSource(source))))throw new Error("Invalid sequence samples.");
      return {samples:[...new Set(step.samples??[])],notes:[...new Set(step.notes)].sort((a,b)=>a-b),velocity:step.velocity,gate:step.gate};
    });
    while(steps.length<STEPS)steps.push({notes:[],samples:[],velocity:100,gate:75});
    return {enabled:track.enabled,length:track.length,resolution,steps};
  });
  while(tracks.length<MAX_TIMBRES)tracks.push(emptySequence().tracks[tracks.length]);
  return {version:1,tracks};
}
export class SequenceClock {
  constructor(emit){this.emit=emit;this.config=emptySequence();this.frame=0;this.nextSteps=timbreArray();this.ticks=timbreArray();this.positions=timbreArray(-1);this.running=false;this.tempo=120;this.releases=[];}
  periodFor(t){return stepFrames(this.tempo,this.config.tracks[t].resolution);}
  rescale(t,ratio){this.nextSteps[t]=this.frame+Math.max(0,this.nextSteps[t]-this.frame)*ratio;for(const release of this.releases)if(release.timbre===t)release.frame=this.frame+Math.max(0,release.frame-this.frame)*ratio;}
  setTempo(bpm){const before=this.tempo;this.tempo=Math.max(10,Math.min(300,bpm));if(this.running)for(let t=0;t<MAX_TIMBRES;t++)this.rescale(t,before/this.tempo);}
  setConfig(config){const next=validateSequence(config);for(let t=0;t<MAX_TIMBRES;t++){
    if(!next.tracks[t].enabled)this.releaseTrack(t);
    if(this.running)this.rescale(t,stepFrames(this.tempo,next.tracks[t].resolution)/this.periodFor(t));
  }this.config=next;}
  releaseTrack(timbre){const keep=[];for(const release of this.releases){if(release.timbre===timbre)this.emit(release.timbre,release.note,0);else keep.push(release);}this.releases=keep;}
  stop(){for(const release of this.releases)this.emit(release.timbre,release.note,0);this.releases=[];this.running=false;this.positions=timbreArray(-1);}
  play(){this.stop();this.ticks=timbreArray();this.nextSteps=timbreArray(this.frame);this.running=true;}
  reset(){const running=this.running;this.stop();this.ticks=timbreArray();this.nextSteps=timbreArray(this.frame);this.running=running;}
  // Native 128-frame boundaries: each lane retains its own fractional timing,
  // while Play starts all four at exactly the same boundary.
  beforeRender(frames=128){
    if(this.running){
      const due=this.releases.filter(r=>r.frame<=this.frame);this.releases=this.releases.filter(r=>r.frame>this.frame);for(const r of due)this.emit(r.timbre,r.note,0);
      for(let timbre=0;timbre<MAX_TIMBRES;timbre++){
        const track=this.config.tracks[timbre],period=this.periodFor(timbre);
        while(this.nextSteps[timbre]<=this.frame){
          this.positions[timbre]=this.ticks[timbre]%track.length;const step=track.steps[this.positions[timbre]];
          if(track.enabled)for(const note of stepEvents(step)){this.emit(timbre,note,step.velocity);this.releases.push({timbre,note,frame:Math.max(this.frame+frames,this.nextSteps[timbre]+period*step.gate/100)});}
          this.ticks[timbre]++;this.nextSteps[timbre]+=period;
        }
      }
    }
    this.frame+=frames;
  }
  status(){return {running:this.running,positions:[...this.positions]};}
}

// Step audition shares the audio clock and plays the complete edited chord.
export class StepAudition {
  constructor(emit){this.emit=emit;this.frame=0;this.notes=[];this.releaseAt=0;}
  play(timbre,step,bpm,resolution="1/16"){
    if(!Number.isInteger(timbre)||timbre<0||timbre>=MAX_TIMBRES)throw new Error("Invalid audition timbre.");
    const checked=emptySequence();checked.tracks[0].steps[0]=step;const chord=validateSequence(checked).tracks[0].steps[0];
    this.stop();this.notes=stepEvents(chord).map(note=>({timbre,note}));
    for(const voice of this.notes)this.emit(voice.timbre,voice.note,chord.velocity);
    this.releaseAt=this.frame+Math.max(128,stepFrames(bpm,resolution)*chord.gate/100);
  }
  stop(){for(const voice of this.notes)this.emit(voice.timbre,voice.note,0);this.notes=[];}
  beforeRender(frames=128){if(this.notes.length&&this.frame>=this.releaseAt)this.stop();this.frame+=frames;}
}
