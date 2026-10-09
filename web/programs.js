import {emptySequence,validateSequence} from './sequence.js';
import {emptySamples,validateSamples,migrateSampleAmplifiers} from './sample-state.js';
import {defaultCircuit,validateCircuit,validateCircuits} from './circuit.js';
import {validateRdlSource} from './rdl.js';
// Routing, splits and live performance belong to the program's timbre slot.
export const SLOT_PARAMETERS=new Set([71,72,119,120,137,138,139,150]);
const slot=t=>{if(!Number.isInteger(t)||t<0||t>3)throw new Error('Invalid timbre slot.');};
export function normalizeValues(input,parameters){
  const v=structuredClone(input);
  if(Array.isArray(v)&&v.length===153)v.push(v[151]);
  if(Array.isArray(v)&&v.length===154){const old=v[29];v.push(old===3?0:old===2?1:old>=4?old-2:1);if(old>=2)v[29]=2;}
  if(!Array.isArray(v)||v.length!==parameters.length||parameters.some(p=>!Number.isInteger(v[p.id])||v[p.id]<p.min||v[p.id]>p.max||p.values&&!p.values.includes(v[p.id]))||v[0]>=4&&v[10]!==0||v[119]>v[120])throw new Error('The program contains invalid parameters.');
  return v;
}
export function normalizeEngine(value,parameters){
  if(value?.version!==1||value.timbres?.length!==4||value.drums?.length!==16)throw new Error('Choose a Rustias program file.');
  const engine={version:1,timbres:value.timbres.map(v=>normalizeValues(v,parameters)),drums:value.drums.map(v=>normalizeValues(v,parameters))};
  if(parameters.some(p=>p.scope==='global'&&engine.timbres.some(v=>v[p.id]!==engine.timbres[0][p.id])))throw new Error('Global settings must agree across timbres.');return engine;
}
export function timbreInfo(raw){
  if(raw==null)return Array.from({length:4},()=>({name:'Custom',preset:'custom',savedId:null,modified:false}));
  if(!Array.isArray(raw)||raw.length!==4)throw new Error('Invalid timbre names.');
  return raw.map(info=>{if(typeof info?.name!=='string'||info.name.length>64||typeof info.preset!=='string'||info.preset.length>128||info.savedId!=null&&(typeof info.savedId!=='string'||info.savedId.length>128)||typeof info.modified!=='boolean')throw new Error('Invalid timbre name.');return structuredClone(info);});
}
export function normalizeProgram(raw,parameters){
  const wrapped=raw?.version===2;
  const samples=validateSamples(wrapped?raw.samples:null,parameters);
  const engine=migrateSampleAmplifiers(normalizeEngine(wrapped?raw.engine:raw,parameters),wrapped?raw.samples:null,samples);
  const result={kind:'rustias-program',version:2,engine,sequencer:validateSequence(wrapped?raw.sequencer:emptySequence()),samples,circuits:validateCircuits(wrapped?raw.circuits:null,engine.timbres),timbreInfo:timbreInfo(wrapped?raw.timbreInfo:null)};
  const rdl=validateRdlSource(wrapped?raw.rdl:null);if(rdl)result.rdl=rdl;
  if(wrapped&&raw.volume!=null){if(!Number.isFinite(raw.volume)||raw.volume<0||raw.volume>100)throw new Error('Invalid program volume.');result.volume=raw.volume;}
  return result;
}
export function captureTimbre(raw,t,parameters,{sequence=false}={}){
  slot(t);const program=normalizeProgram(raw,parameters);
  const result={kind:'rustias-timbre',version:1,values:[...program.engine.timbres[t]],circuit:structuredClone(program.circuits.tracks[t]),library:program.samples.library.filter(p=>p.timbre===t).map(p=>({...structuredClone(p),timbre:0}))};
  if(sequence)result.sequence=structuredClone(program.sequencer.tracks[t]);
  if(program.engine.timbres[0][140]&&program.engine.timbres[0][141]===t){const v=program.engine.timbres[0];result.kit={drums:structuredClone(program.engine.drums),slots:structuredClone(program.samples.slots),gain:program.samples.kitGain,level:v[143],pan:v[144],transpose:v[145],instrument:v[142]};}
  if(program.rdl){result.rdl=structuredClone(program.rdl);result.rdl.unavailable=result.rdl.unavailable.filter(s=>s.timbre===t&&(s.drum==null||result.kit)).map(s=>({...s,timbre:0}));}
  return result;
}
export function normalizeTimbre(raw,parameters){
  if(raw?.kind!=='rustias-timbre'||raw.version!==1)throw new Error('Choose a Rustias timbre file.');
  const values=normalizeValues(raw.values,parameters),circuit=validateCircuit(raw.circuit??defaultCircuit(values));
  const library=validateSamples({...emptySamples(),library:raw.library??[]},parameters).library;
  if(library.some(p=>p.timbre!==0))throw new Error('Invalid timbre sample ownership.');
  const result={kind:'rustias-timbre',version:1,values,circuit,library};
  if(raw.sequence!=null){const sequence=emptySequence();sequence.tracks[0]=raw.sequence;result.sequence=validateSequence(sequence).tracks[0];}
  if(raw.kit!=null){const kit=raw.kit;if(kit.drums?.length!==16)throw new Error('Invalid timbre drum kit.');
    const samples=validateSamples({...emptySamples(),slots:kit.slots,kitGain:kit.gain},parameters);
    for(const [key,id]of [['level',143],['pan',144],['transpose',145],['instrument',142]])if(!Number.isInteger(kit[key])||kit[key]<parameters[id].min||kit[key]>parameters[id].max)throw new Error('Invalid timbre drum kit.');
    result.kit={...structuredClone(kit),drums:kit.drums.map(v=>normalizeValues(v,parameters)),slots:samples.slots,gain:samples.kitGain};
  }
  const rdl=validateRdlSource(raw.rdl);if(rdl){if(rdl.unavailable.some(s=>s.timbre!==0||s.drum!=null&&!result.kit))throw new Error('Invalid timbre source information.');result.rdl=rdl;}
  return result;
}
export function applyTimbre(raw,t,saved,parameters){
  slot(t);const program=normalizeProgram(raw,parameters),sound=normalizeTimbre(saved,parameters),destination=program.engine.timbres[t];
  for(const p of parameters)if(p.scope!=='global'&&!p.readonly&&!SLOT_PARAMETERS.has(p.id))destination[p.id]=sound.values[p.id];
  program.circuits.tracks[t]=structuredClone(sound.circuit);
  if(sound.sequence)program.sequencer.tracks[t]=structuredClone(sound.sequence);
  // Preserve existing pattern sources; saved sound profiles override matching sources only.
  const profiles=new Map(program.samples.library.map(p=>[`${p.timbre}:${p.source}`,p]));
  for(const p of sound.library){const profile={...structuredClone(p),timbre:t};profiles.set(`${t}:${p.source}`,profile);}
  program.samples.library=[...profiles.values()];
  const global=(id,value)=>{for(const v of program.engine.timbres)v[id]=value;};
  if(sound.kit){program.engine.drums=structuredClone(sound.kit.drums);program.samples.slots=structuredClone(sound.kit.slots);program.samples.kitGain=sound.kit.gain;global(140,1);global(141,t);for(const [key,id]of [['level',143],['pan',144],['transpose',145],['instrument',142]])global(id,sound.kit[key]);}
  else if(destination[140]&&destination[141]===t)global(140,0);
  if(program.rdl){program.rdl.unavailable=program.rdl.unavailable.filter(s=>s.drum==null?s.timbre!==t:!sound.kit);}
  else if(sound.rdl){program.rdl=structuredClone(sound.rdl);program.rdl.unavailable=[];}
  if(sound.rdl)program.rdl.unavailable.push(...sound.rdl.unavailable.map(s=>({...s,timbre:t})));
  return normalizeProgram(program,parameters);
}
