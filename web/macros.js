import {MAX_TIMBRES,EFFECT_SLOTS,PARAMETER_COUNT} from './limits.js';
import {validSampleSource} from './sample-state.js';
export const MACRO_COUNT=8,MACRO_BINDINGS=12;
export const emptyMacros=()=>({version:1,knobs:Array.from({length:MACRO_COUNT},(_,i)=>({name:`Macro ${i+1}`,value:0,bindings:[]})),bases:[]});
const integer=(n,min,max)=>Number.isInteger(n)&&n>=min&&n<=max;
export function macroTarget(raw){
  const t=raw?.timbre,p=raw?.parameter;
  const timbre=()=>integer(t,0,MAX_TIMBRES-1),parameter=()=>integer(p,0,PARAMETER_COUNT-1);
  switch(raw?.kind){
    case 'synth':if(timbre()&&parameter())return {kind:raw.kind,timbre:t,parameter:p};break;
    case 'global':if(parameter())return {kind:raw.kind,parameter:p};break;
    case 'drum':if(integer(raw.instrument,0,15)&&parameter())return {kind:raw.kind,instrument:raw.instrument,parameter:p};break;
    case 'sample':if(timbre()&&parameter()&&validSampleSource(raw.source))return {kind:raw.kind,timbre:t,source:raw.source,parameter:p};break;
    case 'effect':if(integer(raw.slot,0,EFFECT_SLOTS-1)&&integer(raw.effectKind,1,30)&&integer(p,0,19))return {kind:raw.kind,slot:raw.slot,effectKind:raw.effectKind,parameter:p};break;
    case 'module':if(timbre()&&integer(raw.node,0,63)&&typeof raw.moduleKind==='string'&&/^[a-z][a-z0-9]*$/.test(raw.moduleKind)&&typeof raw.control==='string'&&/^[a-z][a-z0-9]*$/.test(raw.control))return {kind:raw.kind,timbre:t,node:raw.node,moduleKind:raw.moduleKind,control:raw.control};break;
    case 'kit-gain':case 'volume':return {kind:raw.kind};
  }
  throw new Error('Invalid macro parameter.');
}
export const targetKey=target=>JSON.stringify(macroTarget(target));
export function normalizeMacros(raw){
  if(raw==null)return emptyMacros();
  if(raw.version!==1||raw.knobs?.length!==MACRO_COUNT||!Array.isArray(raw.bases)||raw.bases.length>MACRO_COUNT*MACRO_BINDINGS)throw new Error('Invalid macro settings.');
  const used=new Set(),bases=new Map();
  const knobs=raw.knobs.map(knob=>{
    if(typeof knob.name!=='string'||!knob.name.trim()||knob.name.length>32||!integer(knob.value,0,100)||!Array.isArray(knob.bindings)||knob.bindings.length>MACRO_BINDINGS)throw new Error('Invalid macro knob.');
    const keys=new Set(),bindings=knob.bindings.map(binding=>{const target=macroTarget(binding.target),key=targetKey(target);
      if(keys.has(key)||!integer(binding.amount,-100,100))throw new Error('Invalid macro influence.');keys.add(key);used.add(key);return {target,amount:binding.amount};
    });return {name:knob.name.trim(),value:knob.value,bindings};
  });
  for(const entry of raw.bases){const target=macroTarget(entry.target),key=targetKey(target);if(bases.has(key)||!Number.isFinite(entry.value)||Math.abs(entry.value)>1e9||typeof entry.label!=='string'||entry.label.length>200)throw new Error('Invalid macro base value.');bases.set(key,{target,value:entry.value,label:entry.label});}
  if([...used].some(key=>!bases.has(key)))throw new Error('Missing macro base value.');
  return {version:1,knobs,bases:[...bases].filter(([key])=>used.has(key)).map(([,entry])=>entry)};
}
export function macroOffset(config,target,spec){
  const key=targetKey(target);let weight=0;
  for(const knob of config.knobs)for(const binding of knob.bindings)if(targetKey(binding.target)===key)weight+=knob.value/100*binding.amount/100;
  return weight*(spec.max-spec.min);
}
export function macroValue(config,entry,spec){
  const value=entry.value+macroOffset(config,entry.target,spec),clamped=Math.max(spec.min,Math.min(spec.max,value));
  if(spec.values)return spec.values.reduce((a,b)=>Math.abs(b-clamped)<Math.abs(a-clamped)?b:a);
  const step=spec.step??1;return Number((Math.round(clamped/step)*step).toFixed(8));
}
export class MacroBank {
  constructor({resolve,write}){this.resolve=resolve;this.write=write;this.config=emptyMacros();}
  getConfig(){return structuredClone(this.config);}
  setConfig(raw){this.config=normalizeMacros(raw);}
  apply(targets){const keys=new Set(targets.map(targetKey)),changes=[];for(const entry of this.config.bases)if(keys.has(targetKey(entry.target))){const spec=this.resolve(entry.target);if(spec&&spec.available!==false)changes.push({target:entry.target,value:macroValue(this.config,entry,spec)});}this.write(changes);}
  assign(index,raw){const target=macroTarget(raw),key=targetKey(target),knob=this.config.knobs[index],spec=this.resolve(target);if(!knob||!spec||spec.available===false)throw new Error('This parameter is unavailable.');
    if(knob.bindings.some(b=>targetKey(b.target)===key))return;
    if(knob.bindings.length===MACRO_BINDINGS)throw new Error('Each macro can control up to 12 parameters.');
    if(!this.config.bases.some(b=>targetKey(b.target)===key))this.config.bases.push({target,value:spec.read(),label:spec.label});
    knob.bindings.push({target,amount:100});this.apply([target]);
  }
  setValue(index,value){if(!integer(value,0,100))throw new Error('Invalid macro value.');const knob=this.config.knobs[index];knob.value=value;this.apply(knob.bindings.map(b=>b.target));}
  setAmount(index,target,amount){if(!integer(amount,-100,100))throw new Error('Invalid macro influence.');this.config.knobs[index].bindings.find(b=>targetKey(b.target)===targetKey(target)).amount=amount;this.apply([target]);}
  remove(index,target){const key=targetKey(target),knob=this.config.knobs[index];knob.bindings=knob.bindings.filter(b=>targetKey(b.target)!==key);this.apply([target]);if(!this.config.knobs.some(k=>k.bindings.some(b=>targetKey(b.target)===key)))this.config.bases=this.config.bases.filter(b=>targetKey(b.target)!==key);}
  rebase(target){if(!target)return;const entry=this.config.bases.find(b=>targetKey(b.target)===targetKey(target)),spec=this.resolve(target);if(entry&&spec&&spec.available!==false)entry.value=spec.read()-macroOffset(this.config,target,spec);}
}
