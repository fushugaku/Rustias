import {MAX_TIMBRES,INITIAL_TIMBRES,effectTimbre,effectRole,timbreEffectSlot} from './limits.js';
import {RESOLUTIONS} from './sequence.js';
import {macroTarget,targetKey,macroOffset} from './macros.js';

export const MOD_LANES=6,MOD_STEPS=128;
export const MOD_RESOLUTIONS=['1/48',...RESOLUTIONS];
export const MOD_DIRECTIONS=['Forward','Reverse','Alt1','Alt2'];
export const MOD_RUN_MODES=['Loop','OneShot'];
export const MOD_KEY_SYNC=['Off','Timbre','Voice'];
const integer=(value,min,max)=>Number.isInteger(value)&&value>=min&&value<=max;
export const emptyModulation=()=>({version:1,tracks:Array.from({length:MAX_TIMBRES},()=>[])});
export function modulationTarget(raw){
  if(raw==null)return null;
  if(raw.kind==='macro'){
    if(!integer(raw.macro,0,7))throw new Error('Invalid modulation macro.');
    return {kind:'macro',macro:raw.macro};
  }
  return macroTarget(raw);
}
export const modulationKey=target=>target?.kind==='macro'?'macro:'+target.macro:targetKey(target);
export const modulationRange=target=>target?.kind==='macro'?100:['synth','drum','sample'].includes(target?.kind)&&[53,15].includes(target.parameter)?24:63;
export function newModulationLane(id){
  return {id,enabled:true,target:null,amount:100,length:16,resolution:'1/16',motion:'Step',direction:'Forward',runMode:'Loop',values:Array(MOD_STEPS).fill(0)};
}
export function validateModulation(raw){
  if(raw==null)return emptyModulation();
  if(raw.version!==1||!Array.isArray(raw.tracks)||raw.tracks.length<INITIAL_TIMBRES||raw.tracks.length>MAX_TIMBRES)throw new Error('Invalid modulation sequencers.');
  const tracks=raw.tracks.map(lanes=>{
    if(!Array.isArray(lanes)||lanes.length>MOD_LANES)throw new Error('A timbre can have up to six modulation sequences.');
    const ids=new Set();
    return lanes.map(lane=>{
      const target=modulationTarget(lane.target),range=modulationRange(target);
      if(typeof lane.id!=='string'||!/^[a-zA-Z0-9-]{1,64}$/.test(lane.id)||ids.has(lane.id)||typeof lane.enabled!=='boolean'||!integer(lane.amount,-100,100)||!integer(lane.length,1,MOD_STEPS)||!MOD_RESOLUTIONS.includes(lane.resolution)||!['Step','Slide'].includes(lane.motion)||!MOD_DIRECTIONS.includes(lane.direction)||!MOD_RUN_MODES.includes(lane.runMode)||!Array.isArray(lane.values)||lane.values.length!==MOD_STEPS||lane.values.some(v=>!integer(v,-range,range)))throw new Error('Invalid modulation sequence.');
      if(lane.keySync!=null&&!MOD_KEY_SYNC.includes(lane.keySync))throw new Error('Invalid modulation key sync.');
      ids.add(lane.id);
      return {...lane,target,values:[...lane.values]};
    });
  });
  while(tracks.length<MAX_TIMBRES)tracks.push([]);
  return {version:1,tracks};
}
export function modulationPeriod(tempo,resolution){
  const [n,d]=resolution.split('/').map(Number);
  return 48000*60/Math.max(10,Math.min(300,tempo))*4*n/d;
}
export const modulationBars=lane=>lane.length*Number(lane.resolution.split('/')[0])/Number(lane.resolution.split('/')[1]);
export function modulationBarChoices(lane){
  const [n,d]=lane.resolution.split('/').map(Number),current=modulationBars(lane);
  return [...new Set([current,.25,.5,1,2,3,4,6,8,12,16,24,32,48,64,96,128])].sort((a,b)=>a-b).filter(b=>Math.abs(b*d/n-Math.round(b*d/n))<1e-8&&b*d/n>=1&&b*d/n<=MOD_STEPS).map(b=>({value:Math.round(b*d/n),label:String(Number(b.toFixed(3)))}));
}
function cycleLength(lane){
  return lane.direction==='Alt1'?Math.max(1,2*lane.length-2):lane.direction==='Alt2'?2*lane.length:lane.length;
}
export function modulationStep(lane,tick){
  const n=lane.length,cycle=cycleLength(lane);
  if(lane.runMode==='OneShot'){
    const last=lane.direction==='Alt1'&&n>1?cycle:cycle-1;
    tick=Math.min(tick,last);
    if(lane.direction==='Alt1'&&tick===cycle)return 0;
  }
  const i=tick%cycle;
  if(lane.direction==='Reverse')return n-1-i;
  if(lane.direction==='Alt1')return i<n?i:2*n-2-i;
  if(lane.direction==='Alt2')return i<n?i:2*n-1-i;
  return i;
}
export class ModulationClock {
  constructor(){this.config=emptyModulation();this.phases=new Map();this.tempo=120;this.running=false;this.frame=0;this.positions=Array.from({length:MAX_TIMBRES},()=>[]);this.held=Array.from({length:MAX_TIMBRES},()=>new Set());}
  setConfig(raw){const next=validateModulation(raw),keep=new Map();next.tracks.forEach((lanes,t)=>lanes.forEach(lane=>{const key=t+':'+lane.id;keep.set(key,this.phases.get(key)??0);}));this.phases=keep;this.config=next;}
  setTempo(tempo){this.tempo=Math.max(10,Math.min(300,tempo));}
  play(){for(const key of this.phases.keys())this.phases.set(key,0);this.running=true;}
  stop(){this.running=false;this.positions=this.config.tracks.map(lanes=>lanes.map(()=>-1));this.held.forEach(notes=>notes.clear());}
  reset(){for(const key of this.phases.keys())this.phases.set(key,0);}
  note(timbre,note,velocity){
    const held=this.held[timbre],first=!held.size;
    if(velocity){if(!this.running)this.play();held.add(note);for(const lane of this.config.tracks[timbre])if(lane.keySync==='Voice'||lane.keySync==='Timbre'&&first)this.phases.set(timbre+':'+lane.id,0);}
    else held.delete(note);
  }
  beforeRender(frames=128){
    const offsets=new Map();
    this.positions=this.config.tracks.map((lanes,t)=>lanes.map(lane=>{
      if(!this.running||!lane.enabled||!lane.target)return -1;
      const key=t+':'+lane.id,phase=this.phases.get(key)??0,tick=Math.floor(phase+1e-10),index=modulationStep(lane,tick),next=modulationStep(lane,tick+1),fraction=Math.max(0,phase-tick);
      const value=lane.motion==='Slide'?lane.values[index]+(lane.values[next]-lane.values[index])*fraction:lane.values[index];
      // RADIAS uses offsets and gives a later sequence priority on a shared target.
      offsets.set(modulationKey(lane.target),{target:lane.target,value:value*lane.amount/100});
      this.phases.set(key,phase+frames/modulationPeriod(this.tempo,lane.resolution));return index;
    }));
    this.frame+=frames;return offsets;
  }
}
const bounded=(value,spec)=>{
  value=Math.max(spec.min,Math.min(spec.max,value));
  if(spec.values)return spec.values.reduce((a,b)=>Math.abs(b-value)<Math.abs(a-value)?b:a);
  const step=spec.step??1;return Number((Math.round(value/step)*step).toFixed(8));
};
// Ephemeral offsets never replace saved parameter values or macro base positions.
export class ModulationAutomation {
  constructor(write){this.write=write;this.clock=new ModulationClock();this.targets=new Map();this.outputs=new Map();this.macros={knobs:[],bases:[]};this.liveMacros=[];}
  setState(state){
    this.restore();this.clock.setConfig(state.config);this.macros=state.macros;
    this.targets=new Map(state.targets.map(spec=>[modulationKey(spec.target),spec]));
  }
  restore(){if(this.outputs.size)this.write([...this.outputs.keys()].map(key=>this.targets.get(key)).filter(Boolean).map(spec=>({target:spec.target,value:spec.base})));this.outputs.clear();this.liveMacros=[];}
  stop(){this.clock.stop();this.restore();}
  acceptValues(read){for(const [key,spec]of this.targets){const value=read(spec.target);if(value!=null&&value!==spec.base){spec.base=value;const entry=this.macros.bases.find(base=>targetKey(base.target)===key);if(entry)entry.value=value-macroOffset(this.macros,spec.target,spec);}}}
  beforeRender(frames=128){
    const offsets=this.clock.beforeRender(frames),active=new Set(),macroIndices=new Set();
    for(const entry of offsets.values())if(entry.target.kind==='macro')macroIndices.add(entry.target.macro);else active.add(modulationKey(entry.target));
    const knobs=this.macros.knobs.map((knob,index)=>({...knob,value:Math.max(0,Math.min(100,knob.value+(offsets.get('macro:'+index)?.value??0)))}));
    this.liveMacros=knobs.map((knob,index)=>macroIndices.has(index)?knob.value:null);
    for(const index of macroIndices)for(const binding of knobs[index].bindings)active.add(targetKey(binding.target));
    const bases=new Map(this.macros.bases.map(entry=>[targetKey(entry.target),entry])),changes=[];
    for(const key of new Set([...active,...this.outputs.keys()])){
      const spec=this.targets.get(key);if(!spec)continue;
      let value=spec.base;
      if(active.has(key)&&spec.available!==false){
        const base=bases.get(key);if(base)value=base.value+macroOffset({knobs},spec.target,spec);
        value=bounded(value+(offsets.get(key)?.value??0),spec);
      }
      if(value!==(this.outputs.get(key)??spec.base))changes.push({target:spec.target,value});
    }
    const applied=this.write(changes)??changes.map(c=>modulationKey(c.target));
    const changed=new Map(changes.map(c=>[modulationKey(c.target),c.value]));
    for(const key of applied)if(active.has(key))this.outputs.set(key,changed.get(key));else this.outputs.delete(key);
  }
  status(){return {running:this.clock.running,positions:this.clock.positions,macros:this.liveMacros,values:[...this.outputs].map(([key,value])=>({target:this.targets.get(key).target,value}))};}
}

export function remapModulationLane(lane,from,to){
  const copy=JSON.parse(JSON.stringify(lane)),target=copy.target;
  if(target&&['synth','sample','module'].includes(target.kind)&&target.timbre===from)target.timbre=to;
  if(target?.kind==='effect'&&effectTimbre(target.slot)===from)target.slot=timbreEffectSlot(to,effectRole(target.slot));
  return copy;
}
