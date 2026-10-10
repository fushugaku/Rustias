import {MAX_TIMBRES,INITIAL_TIMBRES,timbreArray} from './limits.js';
import {validSampleSource} from "./sample-state.js";
// Browser-only polyphonic clock. It dispatches notes to the native Wasm engine.
export const STEPS = 128;
export const VIEW_STEPS = 16;
export const MAX_EVENTS = 128;
export const SEQUENCE_RUN_MODES=['Loop','OneShot','Step'];
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
  function validateTrack(track){
    if(typeof track.enabled!=="boolean"||!Number.isInteger(track.length)||track.length<1||track.length>STEPS||!Array.isArray(track.steps)||![16,STEPS].includes(track.steps.length))throw new Error("Invalid sequencer track.");
    const resolution=track.resolution??"1/16";if(!RESOLUTIONS.includes(resolution))throw new Error("Invalid sequencer resolution.");
    const steps=track.steps.map(step=>{
      if(!Array.isArray(step.notes)||step.notes.length+(step.samples?.length??0)>MAX_EVENTS||step.notes.some(n=>!Number.isInteger(n)||n<0||n>127)||!Number.isInteger(step.velocity)||step.velocity<1||step.velocity>127||!Number.isInteger(step.gate)||step.gate<1||step.gate>100)throw new Error("Invalid sequencer step.");
      if(step.samples!=null&&(!Array.isArray(step.samples)||step.samples.some(source=>!validSampleSource(source))))throw new Error("Invalid sequence samples.");
      const result={samples:[...new Set(step.samples??[])],notes:[...new Set(step.notes)].sort((a,b)=>a-b),velocity:step.velocity,gate:step.gate};
      for(const key of ['trigger','tie'])if(step[key]!=null){if(typeof step[key]!=='boolean')throw new Error('Invalid sequencer trigger.');result[key]=step[key];}
      if(step.velocityMode!=null){if(!['Fixed','Key'].includes(step.velocityMode))throw new Error('Invalid sequence velocity.');result.velocityMode=step.velocityMode;}
      if(step.baseNote!=null){if(!Number.isInteger(step.baseNote)||step.baseNote<0||step.baseNote>127)throw new Error('Invalid sequence base note.');result.baseNote=step.baseNote;}
      return result;
    });
    while(steps.length<STEPS)steps.push({notes:[],samples:[],velocity:100,gate:75});
    const result={enabled:track.enabled,length:track.length,resolution,steps};
    if(track.runMode!=null){if(!SEQUENCE_RUN_MODES.includes(track.runMode))throw new Error('Invalid sequencer run mode.');result.runMode=track.runMode;}
    for(const key of ['latch','transpose','keySync','keyTriggered'])if(track[key]!=null){if(typeof track[key]!=='boolean')throw new Error('Invalid sequence mode.');result[key]=track[key];}
    for(const [key,min,max]of [['gateOffset',-100,100],['swing',-100,100],['baseNote',0,127],['scanBottom',0,127],['scanTop',0,127],['pattern',0,2]])if(track[key]!=null){if(!Number.isInteger(track[key])||track[key]<min||track[key]>max)throw new Error('Invalid sequence settings.');result[key]=track[key];}
    if((result.scanBottom??0)>(result.scanTop??127))throw new Error('Invalid sequence scan zone.');
    return result;
  }
  const tracks=value.tracks.map(validateTrack);
  while(tracks.length<MAX_TIMBRES)tracks.push(emptySequence().tracks[tracks.length]);
  const result={version:1,tracks};
  if(value.patterns!=null){if(!Array.isArray(value.patterns)||value.patterns.length>3)throw new Error('Invalid imported patterns.');result.patterns=value.patterns.map(p=>{if(typeof p.name!=='string'||p.name.length>32)throw new Error('Invalid imported pattern name.');return {name:p.name,track:validateTrack(p.track)};});}
  return result;
}
export class SequenceClock {
  constructor(emit){this.emit=emit;this.config=emptySequence();this.frame=0;this.nextSteps=timbreArray();this.ticks=timbreArray();this.positions=timbreArray(-1);this.running=false;this.tempo=120;this.releases=[];this.held=Array.from({length:MAX_TIMBRES},()=>new Map());this.active=timbreArray(false);this.triggerNotes=timbreArray(null);this.forced=false;}
  periodFor(t){return stepFrames(this.tempo,this.config.tracks[t].resolution);}
  rescale(t,ratio){this.nextSteps[t]=this.frame+Math.max(0,this.nextSteps[t]-this.frame)*ratio;for(const release of this.releases)if(release.timbre===t)release.frame=this.frame+Math.max(0,release.frame-this.frame)*ratio;}
  setTempo(bpm){const before=this.tempo;this.tempo=Math.max(10,Math.min(300,bpm));if(this.running)for(let t=0;t<MAX_TIMBRES;t++)this.rescale(t,before/this.tempo);}
  setConfig(config){const next=validateSequence(config);for(let t=0;t<MAX_TIMBRES;t++){
    if(!next.tracks[t].enabled)this.releaseTrack(t);
    if((next.tracks[t].runMode??'Loop')!==(this.config.tracks[t].runMode??'Loop')){this.releaseTrack(t);this.ticks[t]=0;this.nextSteps[t]=this.frame;this.positions[t]=-1;this.active[t]=this.running&&next.tracks[t].runMode!=='Step';}
    if(this.running)this.rescale(t,stepFrames(this.tempo,next.tracks[t].resolution)/this.periodFor(t));
  }this.config=next;}
  releaseTrack(timbre){const keep=[];for(const release of this.releases){if(release.timbre===timbre)this.emit(release.timbre,release.note,0);else keep.push(release);}this.releases=keep;}
  stop(){for(const release of this.releases)this.emit(release.timbre,release.note,0);this.releases=[];this.running=false;this.positions=timbreArray(-1);this.ticks=timbreArray();this.active=timbreArray(false);this.held.forEach(notes=>notes.clear());this.triggerNotes=timbreArray(null);this.forced=false;}
  play(){this.stop();this.ticks=timbreArray();this.nextSteps=timbreArray(this.frame);this.active=this.config.tracks.map(track=>track.runMode!=='Step');this.forced=true;this.running=true;}
  reset(){const running=this.running,forced=this.forced;this.stop();this.ticks=timbreArray();this.nextSteps=timbreArray(this.frame);this.running=running;this.forced=forced;this.active=this.config.tracks.map(track=>running&&track.runMode!=='Step');}
  trigger(timbre,note,velocity){
    const track=this.config.tracks[timbre],held=this.held[timbre];
    if(!track.enabled||!track.keyTriggered&&track.runMode!=='Step'||note<(track.scanBottom??0)||note>(track.scanTop??127))return false;
    if(velocity){
      const first=held.size===0;held.set(note,velocity);this.triggerNotes[timbre]={note,velocity};this.running=true;
      if(track.runMode==='Step'){this.releaseTrack(timbre);this.playStep(timbre,this.ticks[timbre]%track.length,Infinity);this.ticks[timbre]++;}
      else if(first&&!this.forced){if(track.keySync||track.runMode==='OneShot'||this.ticks[timbre]===0){this.releaseTrack(timbre);this.ticks[timbre]=0;this.nextSteps[timbre]=this.frame;}else this.nextSteps[timbre]=Math.max(this.frame,this.nextSteps[timbre]);this.active[timbre]=true;}
    }else{
      held.delete(note);
      if(!held.size&&(track.runMode==='Step'||!track.latch&&!this.forced)){this.active[timbre]=false;this.releaseTrack(timbre);this.positions[timbre]=-1;}
    }
    return true;
  }
  playStep(timbre,index,period,frames=128){
    const track=this.config.tracks[timbre],step=track.steps[index],trigger=this.triggerNotes[timbre];this.positions[timbre]=index;
    const offset=track.transpose&&trigger?trigger.note-(step.baseNote??track.baseNote??60):0;
    const notes=step.trigger===false?[]:stepEvents(step).map(note=>typeof note==='number'?note+offset:note).filter(note=>typeof note!=='number'||note>=0&&note<=127);
    // TIE carries matching notes without a note-off / retrigger at the boundary.
    const tied=this.releases.filter(r=>r.timbre===timbre&&r.tie),keep=new Set(tied.filter(r=>notes.includes(r.note)).map(r=>r.note));
    for(const release of tied)if(!keep.has(release.note))this.emit(timbre,release.note,0);
    this.releases=this.releases.filter(r=>r.timbre!==timbre||!r.tie);
    const velocity=step.velocityMode==='Key'?(trigger?.velocity??100):step.velocity,gate=Math.max(1,Math.min(100,step.gate+(track.gateOffset??0)));
    for(const note of notes){if(!keep.has(note))this.emit(timbre,note,velocity);this.releases.push({timbre,note,tie:!!step.tie,frame:step.tie?Infinity:Math.max(this.frame+frames,this.nextSteps[timbre]+period*gate/100)});}
  }
  // Native 128-frame boundaries: each lane retains its own fractional timing,
  // while Play starts all four at exactly the same boundary.
  beforeRender(frames=128){
    if(this.running){
      const due=this.releases.filter(r=>r.frame<=this.frame);this.releases=this.releases.filter(r=>r.frame>this.frame);for(const r of due)this.emit(r.timbre,r.note,0);
      for(let timbre=0;timbre<MAX_TIMBRES;timbre++){
        const track=this.config.tracks[timbre],period=this.periodFor(timbre);
        while(this.active[timbre]&&this.nextSteps[timbre]<=this.frame){
          if(track.runMode==='OneShot'&&this.ticks[timbre]>=track.length){this.active[timbre]=false;const ties=this.releases.filter(r=>r.timbre===timbre&&r.tie);for(const r of ties)this.emit(timbre,r.note,0);this.releases=this.releases.filter(r=>r.timbre!==timbre||!r.tie);break;}
          this.positions[timbre]=this.ticks[timbre]%track.length;
          if(track.enabled)this.playStep(timbre,this.positions[timbre],period,frames);
          const swing=(track.swing??0)/100;this.nextSteps[timbre]+=period*(this.ticks[timbre]%2?1-swing:1+swing);this.ticks[timbre]++;
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
