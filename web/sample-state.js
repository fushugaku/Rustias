import {MAX_TIMBRES} from './limits.js';
import {extendValues} from './parameters.js';
export const validSampleSource=source=>typeof source==='string'&&/^(?:808|909):[a-z0-9-]+$|^custom:[a-zA-Z0-9-]+$/.test(source);
export const emptySamples=()=>({version:3,kitGain:12,slots:Array.from({length:16},()=>({source:'synth',mode:0})),library:[]});
export function validateSamples(value,parameters){
  if(value==null)return emptySamples();
  if(![1,2,3].includes(value.version)||!Array.isArray(value.slots)||value.slots.length!==16)throw new Error('Invalid drum sample assignments.');
  const kitGain=value.kitGain??12;if(!Number.isInteger(kitGain)||kitGain< -24||kitGain>24)throw new Error('Invalid Drum Kit gain.');
  const library=value.library??[];
  if(!Array.isArray(library)||library.length>1024)throw new Error('Invalid sequence sample library.');
  const keys=new Set();
  return {version:3,kitGain,slots:value.slots.map(slot=>{
    if(slot?.source!=='synth'&&!validSampleSource(slot?.source)||![0,1,2].includes(slot?.mode))throw new Error('Invalid drum sample assignment.');
    return {source:slot.source,mode:slot.mode,...(slot.name?{name:String(slot.name).slice(0,100)}:{})};
  }),library:library.map(profile=>{
    const key=`${profile?.timbre}:${profile?.source}`;
    if(!validSampleSource(profile?.source)||!Number.isInteger(profile.timbre)||profile.timbre<0||profile.timbre>=MAX_TIMBRES||![0,1,2].includes(profile.mode)||keys.has(key)||!Array.isArray(profile.values)||![153,154,155,163].includes(profile.values.length)||profile.values.some(v=>!Number.isInteger(v)||v< -32768||v>32767))throw new Error('Invalid sequence sample profile.');
    profile={...profile,values:extendValues(profile.values,parameters)};
    if(parameters&&parameters.some(p=>profile.values[p.id]<p.min||profile.values[p.id]>p.max||p.values&&!p.values.includes(profile.values[p.id])))throw new Error('Invalid sample sound parameters.');
    if(profile.values[0]>=4&&profile.values[10]!==0||profile.values[119]>profile.values[120])throw new Error("Invalid sample sound parameters.");
    keys.add(key);return {source:profile.source,timbre:profile.timbre,mode:profile.mode,name:String(profile.name??profile.source).slice(0,100),values:[...profile.values]};
  })};
}
// Old PCM patches used the owning timbre's manual gain. Preserve that sound
// when moving to per-instrument gains; versions 2/3 keep independent edits.
export function migrateSampleAmplifiers(engine,previous,samples){
  if(previous?.version!==1)return engine;
  const result=structuredClone(engine),gain=result.timbres[result.timbres[0][141]][115];
  samples.slots.forEach((slot,index)=>{if(slot.source!=='synth')result.drums[index][115]=gain;});
  return result;
}
export function sampleValues(defaults){
  const values=[...defaults];
  for(const [id,value] of [[3,0],[4,127],[5,127],[6,32],[9,0],[1,127],[2,0],[114,0],[115,32512],[116,0],[117,127],[147,0]])values[id]=value;
  return values;
}
