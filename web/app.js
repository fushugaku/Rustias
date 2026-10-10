import {MAX_TIMBRES,INITIAL_TIMBRES,MASTER_EFFECT_SLOT,timbreArray,effectUsesMaster,effectTimbre,effectRole,timbreEffectSlots,LFO3_BASE} from './limits.js';
import {webParameters} from './parameters.js';
import {createMacrosPanel} from './macros-ui.js';
import {createModulationEditor} from './modulation-ui.js';
import {modulationKey} from './modulation.js';
import {targetKey} from './macros.js';
import {bindMacroTarget} from './macro-gesture.js';
import {effectDefinitions} from './effects.js';
import {createCircuitEditor} from "./circuit-ui.js";
import {setEffectCatalog,emptyEffects} from './effects.js';
import {createEffectsPanel} from './effects-ui.js';
import {createRecorder} from './recordings-ui.js';
import {audioCircuit,emptyCircuits,MODULES} from "./circuit.js";
import {createDrumSamples} from "./samples.js";
import {createSequencer} from "./sequencer-ui.js";
import {drumSequenceKit} from "./sequence-labels.js";
import {emptySequence} from "./sequence.js";
import {PatchStore,TimbreStore} from "./patches.js";
import {normalizeEngine as checkEngine,normalizeProgram,captureTimbre,normalizeTimbre,applyTimbre,SLOT_PARAMETERS} from "./programs.js";
import {MAX_RDL_BYTES,readRdl,rdlPatches,validateRdlSource,rdlMasks} from './rdl.js';
import {createPanel,makePicker,isChoosing} from "./panel.js";
import {setupInterfaceNavigation,prepareNewInterface} from './new-interface.js';
const newInterface=setupInterfaceNavigation();
const $ = selector => document.querySelector(selector);
function showError(error) { $("#error").textContent = error.message ?? String(error); $("#error").hidden = false; }
let parameters;
try {
  const response = await fetch(new URL("./parameters.json", import.meta.url));
  if (!response.ok) throw new Error(`Could not load the parameter definitions (${response.status}).`);
  parameters = webParameters(await response.json());
  const fxResponse=await fetch(new URL('./effects.json',import.meta.url));
  if(!fxResponse.ok)throw new Error(`Could not load effects (${fxResponse.status}).`);
  setEffectCatalog(await fxResponse.json());
} catch (error) { showError(error); throw error; }
const defaults = parameters.map(p => p.default);
const timbres = Array.from({length: MAX_TIMBRES}, (_, i) => ({values: defaults.map((v, id) => id === 72 ? i : id === 71 && i >= INITIAL_TIMBRES ? 0 : v), preset: "init",name:"INIT",savedId:null,modified:false}));
const drums = Array.from({length: 16}, (_, i) => defaults.map((v, id) => ({3: 0, 4: 32, 5: 0, 6: 20, 146: 60 + i})[id] ?? v));
const presets = {
  init: {}, pad: {0: 2, 1: 78, 2: 35, 3: 80, 4: 64, 5: 108, 6: 78, 7: 100, 67: 1, 68: 2, 69: 18, 70: 75},
  pulse: {0: 1, 1: 66, 2: 55, 3: 0, 4: 44, 5: 88, 6: 24, 7: 110, 11: 40, 62: 0},
  pluck: {0: 3, 1: 107, 2: 15, 3: 0, 4: 40, 5: 0, 6: 35, 7: 110},
};
const names = ["Saw", "Pulse", "Triangle", "Sine", "Noise", "Formant"];
const keyNames = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const computerKeys = ["a", "w", "s", "e", "d", "f", "t", "g", "y", "h", "u", "j", "k", "o", "l", "p"];
let selected = 0, timbreCount=INITIAL_TIMBRES, octave = 4, context, node, module, midiAccess, audioStarting;
const held = new Map(), noteCounts = new Map();
let sampleUI,circuitUI,fxUI,macroUI,modulationUI,recorder,newUI,circuitTimer,editingSample=null;
let automationQueued=false;
let macroCircuitTimer;const macroCircuits=new Set();
let panel, programPicker, timbrePicker, sequenceUI, activeSavedPatch=null,programState="init",programDirty=false,patchDialogKind="program",patchDialogSlot=0, autosaveTimer, sequenceRequest=0, auditionRequest=0,sequenceConfigRequest=0;
let rdlSource=null,lastRdlMasks;
const patchStore=new PatchStore(window.localStorage),timbreStore=new TimbreStore(window.localStorage);
const INCLUDE_TIMBRE_SEQUENCE=false;
const factorySounds=[{value:"init",label:"INIT"},{value:"pad",label:"Warm pad"},{value:"pulse",label:"Pulse bass"},{value:"pluck",label:"Soft pluck"},{value:"custom",label:"Custom"}];
function markProgram(){programDirty=true;if(!activeSavedPatch)programState="custom";}
function markTimbre(t=selected){timbres[t].modified=true;markProgram();}
const send = message => node?.port.postMessage(message);
const global = id => timbres[0].values[id];
const isDrum = () => global(140) !== 0 && global(141) === selected;
const isSample = () => !!editingSample || isDrum() && sampleUI?.assigned(global(142));
const sampleProfile = () => editingSample?sampleUI?.findProfile(editingSample.timbre,editingSample.source):null;
const instrumentParameter = id => parameters[id].scope !== "global" && (isSample() && id >= 114 && id <= 117 || !(id >= 59 && id <= 72 || id >= 114 && id <= 120 || id >= 137 && id <= 139 || id >= 150 && id <= 151 || id === 153));
const values = () => parameters.map(p => p.id === 118 ? (isSample() ? 0 : timbres[selected].values[67] ? timbres[selected].values[68] - 1 : 0) : editingSample && instrumentParameter(p.id) ? sampleProfile()?.values[p.id]??p.default : isDrum() && instrumentParameter(p.id) ? drums[global(142)][p.id] : timbres[selected].values[p.id]);
function unavailableSource(){if(editingSample)return;return rdlSource?.unavailable.find(s=>isDrum()?s.drum===global(142):s.drum==null&&s.timbre===selected);}
function muteMasks(){const masks=rdlMasks(rdlSource);for(let i=0;i<16;i++)if(sampleUI?.assigned(i))masks.drums&=~(1<<i);return masks;}
function syncRdlMasks(){if(!node)return;const masks=muteMasks(),key=`${masks.timbres}:${masks.drums}`;if(key!==lastRdlMasks){send({type:'rdl-muted',...masks});lastRdlMasks=key;}}
function loadEngine(program=state()){const unavailable=muteMasks();send({type:'load',program,unavailable});sendCircuits();lastRdlMasks=`${unavailable.timbres}:${unavailable.drums}`;}
function sendCircuits(timbre){if(!circuitUI)return;const config=circuitUI.getConfig();for(const [t,circuit]of config.tracks.entries())if(timbre==null||t===timbre)send({type:'circuit',timbre:t,circuit:audioCircuit(circuit)});}
function replaceSource(timbre,drum){if(rdlSource)rdlSource.unavailable=rdlSource.unavailable.filter(s=>drum==null?s.drum!=null||s.timbre!==timbre:s.drum!==drum);}
function format(id, value) {
  const p = parameters[id];
  if (p.options) return p.options[(p.values ?? p.options.map((_, i) => p.min + i)).indexOf(value)];
  if(id===9){const types=['LPF 24','LPF 12','BPF','HPF','Thru'];if(value===127)return 'Thru';const i=Math.floor(value/32),part=value%32;return part?`${types[i]} → ${types[i+1]} ${Math.round(part/32*100)}%`:types[i];}
  if ([1, 22].includes(id)) { const hz = 30 * 200 ** (value / 127); return hz >= 1000 ? `${(hz / 1000).toFixed(1)} kHz` : `${Math.round(hz)} Hz`; }
  if ([3, 4, 6, 32, 33, 35, 36, 37, 39, 59].includes(id)) {
    if (id === 59 && value === 0) return "Off";
    const ms = 3 * 2000 ** (value / 127); return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`;
  }
  if ([8, 144].includes(id)) return value === 64 ? "Center" : `${value < 64 ? "L" : "R"} ${Math.abs(value - 64)}`;
  if ([72, 148, 141, 142].includes(id)) return String(value + 1);
  if (id === 89) return `${(value / 10).toFixed(1)} BPM`;
  if (id === 146 || [119, 120].includes(id)) return noteLabel(value);
  if ([75, 83, LFO3_BASE+2].includes(id)) return `${(0.02 * 1500 ** (value / 127)).toFixed(2)} Hz`;
  if (p.center !== undefined) return `${value - p.center > 0 ? "+" : ""}${value - p.center}`;
  return String(value);
}
function disabled(id, v) {
  if(isSample()&&[0,10,11,12,13,14,15,16,17,18,19].includes(id))return true;
  if(isSample()&&[59,60,61,62,63,64,65,66,67,68,69,70,139].includes(id))return true;
  if(isSample()&&[6,35,39].includes(id)&&(sampleProfile()?.mode??sampleUI.getConfig().slots[global(142)].mode)===0)return true;
  if(unavailableSource()&&[10,11,12].includes(id))return true;
  if (id === 10) return v[0] >= 4;
  if (id === 154) return v[29] !== 2;
  if ([21, 22, 23, 24, 27, 28].includes(id)) return v[20] === 0;
  if ([64, 63].includes(id)) return v[62] === 1;
  if ([68, 69, 70].includes(id)) return v[67] === 0;
  if (id === 115) return v[152] === 1;
  if ([114, 116, 117].includes(id) && isDrum() && !isSample()) return true;
  if (id === 118 && isDrum()) return true;
  if (id === 117) return v[116] === 0;
  if (id >= 125 && id <= 136) return v[122] !== 9;
  if ([75, 83, LFO3_BASE+2].includes(id)) return v[id + 3] !== 0;
  if ([79, 87, LFO3_BASE+6].includes(id)) return v[id - 1] === 0;
  if ([77, 85, LFO3_BASE+4].includes(id)) return v[id - 1] === 0;
  if ([143, 144, 145].includes(id)) return v[140] === 0;
  if(id===146&&editingSample)return true;
  if ([146, 147].includes(id)) return !isDrum()&&!editingSample;
  return false;
}
const macroExcluded=new Set([71,72,118,119,120,140,141,142,148]);
function parameterTarget(id){
  const p=parameters[id];if(p.readonly||macroExcluded.has(id))return null;
  if(p.scope==='global')return {kind:'global',parameter:id};
  if(editingSample&&instrumentParameter(id))return {kind:'sample',...editingSample,parameter:id};
  if(isDrum()&&instrumentParameter(id))return {kind:'drum',instrument:global(142),parameter:id};
  return {kind:'synth',timbre:selected,parameter:id};
}
function resolveMacro(target){
  if(target.kind==='effect'){
    const fx=fxUI?.getSlot(target.slot),p=effectDefinitions(effectUsesMaster(target.slot))[target.effectKind]?.properties[target.parameter],owner=effectTimbre(target.slot),role=effectRole(target.slot);
    if(!p)return null;return {label:`${owner==null?'Master FX':`T${owner+1} · ${role<2?'Insert ': 'FX '}${role+1}`} · ${p.name}`,min:p.min+p.zero,max:p.max+p.zero,read:()=>fx.parameters[target.parameter],available:fx?.kind===target.effectKind&&(owner==null||owner<timbreCount)};
  }
  if(target.kind==='module'){
    const node=circuitUI?.getNode(target.timbre,target.node),def=MODULES[target.moduleKind],control=def?.controls?.[target.control];if(!control)return null;
    return {label:`T${target.timbre+1} · ${def.name} ${target.node+1} · ${control[0]}`,min:control[1],max:control[2],step:target.control==='rate'?.01:1,read:()=>node?.params[target.control]??control[3],available:target.timbre<timbreCount&&node?.kind===target.moduleKind&&!(node?.kind==='oscillator1'&&target.control==='mode'&&node.params.wave>=4)};
  }
  if(target.kind==='kit-gain')return {label:'Drum Kit · Gain',min:-24,max:24,read:()=>sampleUI.getConfig().kitGain};
  if(target.kind==='volume')return {label:'Output · Volume',min:0,max:100,read:()=>Number($('#volume').value)};
  const p=parameters[target.parameter];if(!p||p.readonly||macroExcluded.has(p.id))return null;
  let v,label,available=true;
  if(target.kind==='global'){if(p.scope!=='global')return null;v=timbres[0].values;label=`${p.group} · ${p.label}`;}
  else if(p.scope==='global')return null;
  else if(target.kind==='synth'){v=timbres[target.timbre]?.values;label=`T${target.timbre+1} · ${p.group} · ${p.label}`;available=target.timbre<timbreCount&&!(global(140)&&global(141)===target.timbre&&(!(p.id>=59&&p.id<=72||p.id>=114&&p.id<=120||p.id>=137&&p.id<=139||p.id>=150&&p.id<=151||p.id===153)||p.id>=114&&p.id<=117&&sampleUI?.assigned(global(142))));}
  else if(target.kind==='drum'){v=drums[target.instrument];label=`Drum ${target.instrument+1} · ${p.group} · ${p.label}`;}
  else if(target.kind==='sample'){v=sampleUI?.findProfile(target.timbre,target.source)?.values;label=`T${target.timbre+1} · ${sampleUI?.sampleName(target.source)??target.source} · ${p.group} · ${p.label}`;available=target.timbre<timbreCount;}
  if(!v)return null;if(p.id===10&&v[0]>=4)available=false;
  return {label,min:p.min,max:p.max,values:p.values,read:()=>v[p.id],available};
}
function modulationTargets(t){
  const result=[],samples=sampleUI?.getConfig(),knobs=macroUI?.getConfig().knobs??[];
  const ids=parameters.filter(p=>p.scope!=='global'&&!p.readonly&&!p.options&&!macroExcluded.has(p.id)&&![66,137,138,139,146,147,150].includes(p.id)).map(p=>p.id);
  const add=(target,prefix='')=>{const spec=resolveMacro(target);if(spec&&spec.available!==false)result.push({value:modulationKey(target),label:prefix+spec.label,target});};
  for(const parameter of ids)add({kind:'synth',timbre:t,parameter});
  if(global(140)&&global(141)===t)for(let instrument=0;instrument<16;instrument++)for(const parameter of ids){
    if(parameter>=59&&parameter<=72||parameter>=114&&parameter<=120&&!(parameter<=117&&samples?.slots[instrument].source!=='synth'))continue;
    add({kind:'drum',instrument,parameter});
  }
  for(const profile of samples?.library??[])if(profile.timbre===t)for(const parameter of ids){if(parameter>=59&&parameter<=72||parameter>=118&&parameter<=120)continue;add({kind:'sample',timbre:t,source:profile.source,parameter});}
  for(const slot of [...timbreEffectSlots(t),MASTER_EFFECT_SLOT]){const fx=fxUI?.getSlot(slot);if(fx?.kind)effectDefinitions(effectUsesMaster(slot))[fx.kind].properties.forEach((_,parameter)=>add({kind:'effect',slot,effectKind:fx.kind,parameter}));}
  for(let macro=0;macro<8;macro++){const label='Macro '+(macro+1),name=knobs[macro]?.name;result.push({value:'macro:'+macro,label:label+(name&&name!==label?' · '+name:''),target:{kind:'macro',macro}});}
  return result;
}
function syncAutomation(){
  if(automationQueued)return;automationQueued=true;
  queueMicrotask(()=>{
    automationQueued=false;if(!node||!modulationUI||!macroUI)return;
    const config=modulationUI.getConfig(),macros=macroUI.getConfig(),targets=new Map();
    for(const lane of config.tracks.flat())if(lane.target&&lane.target.kind!=='macro')targets.set(targetKey(lane.target),lane.target);
    for(const knob of macros.knobs)for(const binding of knob.bindings)targets.set(targetKey(binding.target),binding.target);
    const specs=[];for(const target of targets.values()){const spec=resolveMacro(target);if(spec)specs.push({target,min:spec.min,max:spec.max,step:spec.step,values:spec.values,base:spec.read(),available:spec.available!==false});}
    send({type:'modulation',state:{config,macros,targets:specs,engine:state(),effects:fxUI.getConfig(),circuits:circuitUI.getConfig().tracks.map(audioCircuit)}});
  });
}
function writeMacros(changes){
  const circuits=new Set(),effects=new Set();
  for(const {target,value}of changes){
    if(target.kind==='effect'){fxUI.setParameter(target.slot,target.parameter,value);effects.add(target.slot);const owner=effectTimbre(target.slot);if(owner!=null)markTimbre(owner);}
    else if(target.kind==='module'){circuitUI.setParameter(target.timbre,target.node,target.control,value);circuits.add(target.timbre);markTimbre(target.timbre);}
    else if(target.kind==='kit-gain')sampleUI.setGain(value);
    else if(target.kind==='volume'){$('#volume').value=value;send({type:'gain',value:value/100});}
    else{
      const id=target.parameter;let v=target.kind==='global'?timbres[0].values:target.kind==='synth'?timbres[target.timbre].values:target.kind==='drum'?drums[target.instrument]:sampleUI.findProfile(target.timbre,target.source).values;
      const next=id===10&&v[0]>=4?0:value;
      if(target.kind==='global'){for(const timbre of timbres)timbre.values[id]=next;send({type:'control',timbre:0,parameter:id,value:next});for(let t=0;t<timbreCount;t++)circuits.add(t);}
      else if(target.kind==='sample'){sampleUI.controlLibrary(target.timbre,target.source,id,next);markTimbre(target.timbre);}
      else{v[id]=next;if(id===0&&next>=4)v[10]=0;
        if(target.kind==='drum'){send({type:'drum-control',instrument:target.instrument,parameter:id,value:next});circuits.add(global(141));}
        else{if([66,137,138,139,150].includes(id)){const channel=v[72]===16?global(148):v[72];for(const t of timbres)if((t.values[72]===16?global(148):t.values[72])===channel)t.values[id]=next;}send({type:'control',timbre:target.timbre,parameter:id,value:next});circuits.add(target.timbre);markTimbre(target.timbre);}
      }
    }
  }
  for(const slot of effects)send({type:'effect',slot,program:fxUI.getSlot(slot)});
  for(const t of circuits)if(circuitUI.getConfig().tracks[t].enabled)macroCircuits.add(t);
  if(macroCircuits.size&&!macroCircuitTimer)macroCircuitTimer=setTimeout(()=>{macroCircuitTimer=null;for(const t of macroCircuits)sendCircuits(t);macroCircuits.clear();},30);
  fxUI?.refresh();circuitUI?.refresh();updateControls();updateKeys();
}
function setControl(id, value, fromUser = true) {
  if([140,141,142].includes(id))editingSample=null;
  const p = parameters[id]; value = Math.max(p.min, Math.min(id===141?timbreCount-1:p.max, Math.round(value)));
  const v = values(); if (disabled(id, v)) return;
  if (id === 119 && value > v[120] || id === 120 && value < v[119]) return;
  if (p.values && !p.values.includes(value)) return;
  if(id===0)replaceSource(selected,isDrum()?global(142):null);
  if (p.scope === "global") for (const t of timbres) t.values[id] = value;
  else if(editingSample&&instrumentParameter(id))sampleUI.controlLibrary(selected,editingSample.source,id,value);
  else (isDrum() && instrumentParameter(id) ? drums[global(142)] : timbres[selected].values)[id] = value;
  if (id === 0 && value >= 4) (isDrum() ? drums[global(142)] : timbres[selected].values)[10] = 0;
  if ([66,137,138,139,150].includes(id)) {
    const channel = timbres[selected].values[72] === 16 ? global(148) : timbres[selected].values[72];
    for (const t of timbres) if ((t.values[72] === 16 ? global(148) : t.values[72]) === channel) t.values[id] = value;
  }
  if(fromUser){markProgram();if(p.scope!=="global"&&!SLOT_PARAMETERS.has(id))markTimbre();}
  if(circuitUI?.getConfig().tracks[selected].enabled){clearTimeout(circuitTimer);const timbre=selected;circuitTimer=setTimeout(()=>sendCircuits(timbre),60);}
  if(!(editingSample&&instrumentParameter(id)))send(isSample() && id >= 114 && id <= 117 ? {type:"drum-control",instrument:global(142),parameter:id,value} : {type: "control", timbre: selected, parameter: id, value});
  if(id===30)circuitUI?.positionDrive(value);
  macroUI?.rebase(parameterTarget(id));updateControls(); updateKeys(); scheduleSession();
}
function updateControls() {
  document.querySelectorAll('[data-timbre]').forEach(button=>{button.hidden=Number(button.dataset.timbre)>=timbreCount;button.setAttribute('aria-selected',Number(button.dataset.timbre)===selected);});
  $('#add-timbre').disabled=timbreCount===MAX_TIMBRES;
  const v = values();
  const missing=unavailableSource();
  const pcm=isSample(),sourceLabel=pcm?'Sample':missing?`${missing.label} ×`:null;
  panel?.render(v,sourceLabel?{0:sourceLabel}:{}); sampleUI?.render();syncRdlMasks();
  sequenceUI?.setDrumKit(drumSequenceKit(timbres[0].values,drums,sampleUI?.instrumentNames(),sampleUI?.getConfig().slots.map(s=>s.source)));
  $('#rdl-details').hidden=!rdlSource;$('#wave-display').toggleAttribute('hidden',!!missing||pcm);
  document.querySelector('.module-filter1 h2').textContent=v[9]===127?'FILTER 1 · THRU':'FILTER 1';
  $("#wave-display").setAttribute("aria-label", `${names[v[0]]} waveform`);
  const paths = ["M0 60 L60 12 L60 60 L120 12 L120 60 L180 12 L180 60 L240 12", "M0 60 V12 H30 V60 H60 V12 H90 V60 H120 V12 H150 V60 H180 V12 H210 V60 H240", "M0 36 L30 12 L60 36 L90 60 L120 36 L150 12 L180 36 L210 60 L240 36", `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 4 * Math.PI)}`).join(" L")}`, `M${Array.from({length: 61}, (_, i) => `${i * 4} ${12 + (i * 17 % 49)}`).join(" L")}`, `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 10 * Math.PI) * Math.sin(i / 120 * 2 * Math.PI)}`).join(" L")}`];
  $("#wave-path").setAttribute("d", paths[v[0]]);
  $("#edit-context").textContent = editingSample ? `T${selected+1} · ${sampleUI.sampleName(editingSample.source)}` : isDrum() ? `Drum ${String(global(142) + 1).padStart(2, "0")} · Timbre ${String(selected + 1).padStart(2, "0")}` : `Timbre ${String(selected + 1).padStart(2, "0")}`;
  $("#edit-context").title=$("#edit-context").textContent;
  const programLabel=patchStore.list().find(p=>p.id===activeSavedPatch)?.name??(programState==='init'?'INIT':'Unsaved');
  programPicker?.render(activeSavedPatch?`saved:${activeSavedPatch}`:programState,programLabel+(programDirty?' *':''));
  recorder?.programChanged();
  macroUI?.refresh();
  const sound=timbres[selected],saved=timbreStore.list().find(p=>p.id===sound.savedId);
  timbrePicker?.render(saved?`saved:${sound.savedId}`:factorySounds.some(p=>p.value===sound.preset)?sound.preset:'custom',sound.name+(sound.modified?' *':''));
  modulationUI?.refreshTargets();syncAutomation();
  newUI?.refreshContext({selected,timbreCount,values:v});
}
function displayValue(p, value) { return p.id === 89 ? value / 10 : [72, 148, 141, 142].includes(p.id) ? value + 1 : value - (p.center ?? 0); }
function nativeValue(p, value) { return p.id === 89 ? value * 10 : [72, 148, 141, 142].includes(p.id) ? value - 1 : value + (p.center ?? 0); }
panel=createPanel({parameters,readValues:values,setControl,format,disabled,displayValue,nativeValue,macroTarget:parameterTarget,maxValue:id=>id===141?timbreCount-1:parameters[id].max});
if(newInterface)newUI=prepareNewInterface();
programPicker=makePicker({label:"Program",options:[{value:"init",label:"New program"},{value:"custom",label:"Unsaved"}],value:"init",onChange:applyProgram,searchable:true,searchLabel:'Search programs'});
timbrePicker=makePicker({label:"Timbre sound",options:[...factorySounds],value:"init",onChange:applyPreset,searchable:true,searchLabel:'Search timbres'});$('#timbre-preset').append(timbrePicker.button);
$("#preset").append(programPicker.button);
for(let t=INITIAL_TIMBRES;t<MAX_TIMBRES;t++){const button=document.createElement('button');button.type='button';button.setAttribute('role','tab');button.setAttribute('aria-selected','false');button.setAttribute('aria-label',`Timbre ${t+1}`);button.dataset.timbre=t;button.textContent=String(t+1).padStart(2,'0');button.hidden=true;$('.timbres').append(button);}
$('#add-timbre').addEventListener('click',()=>{if(timbreCount>=MAX_TIMBRES)return;const t=timbreCount++;timbres[t].values[71]=1;const config=sequenceUI.getConfig();config.tracks[t].enabled=true;sequenceUI.setConfig(config);sequenceUI.setTimbreCount(timbreCount);send({type:'control',timbre:t,parameter:71,value:1});configureSequence(config);markProgram();$(`[data-timbre="${t}"]`).click();scheduleSession();});
document.querySelectorAll("[data-timbre]").forEach(button => button.addEventListener("click", () => { editingSample=null;selected = Number(button.dataset.timbre); document.querySelectorAll("[data-timbre]").forEach(tab => tab.setAttribute("aria-selected", tab === button)); circuitUI?.select();fxUI?.select(); updateControls(); updateKeys(); scheduleSession(); }));
function applyProgram(value){
  if(value==='init'){
    const engine={version:1,timbres:Array.from({length:4},(_,i)=>defaults.map((v,id)=>id===72?i:v)),drums:Array.from({length:16},(_,i)=>defaults.map((v,id)=>({3:0,4:32,5:0,6:20,146:60+i})[id]??v))};
    loadSnapshot({version:2,engine,sequencer:emptySequence(),timbreInfo:Array.from({length:4},()=>({name:'INIT',preset:'init',savedId:null,modified:false}))});activeSavedPatch=null;programState='init';programDirty=false;updateControls();saveSession();return;
  }
  if(!value.startsWith('saved:'))return;const patch=patchStore.list().find(p=>p.id===value.slice(6));if(!patch)return;
  try{loadSnapshot(patch.snapshot);activeSavedPatch=patch.id;programDirty=false;updateControls();scheduleSession();}catch(error){showError(error);}
}
function loadTimbre(saved,name,id=null,preset='custom'){
  const target=selected,next=applyTimbre(snapshot(),target,saved,parameters);
  next.timbreInfo[target]={name:name.slice(0,64),preset,savedId:id,modified:false};
  loadSnapshot(next);macroUI?.rebaseTimbre(target);markProgram();updateControls();scheduleSession();
}
function applyPreset(preset){
  if(preset.startsWith('saved:')){const saved=timbreStore.list().find(p=>p.id===preset.slice(6));if(saved)try{loadTimbre(saved.snapshot,saved.name,saved.id);}catch(error){showError(error);}return;}
  if(!Object.hasOwn(presets,preset))return;
  const sound={kind:'rustias-timbre',version:1,values:defaults.map((v,id)=>presets[preset][id]??v),circuit:emptyCircuits().tracks[0],library:[]};
  try{loadTimbre(sound,factorySounds.find(s=>s.value===preset).label,null,preset);}catch(error){showError(error);}
}
function state() { return {version: 1, timbres: timbres.map(t => [...t.values]), drums: drums.map(v => [...v]),effects:fxUI?.getConfig()??emptyEffects()}; }
function acceptState(program) {
  const changed=[];for(let t=0;t<MAX_TIMBRES;t++)for(const p of parameters)if(timbres[t].values[p.id]!==program.timbres[t][p.id])changed.push({kind:p.scope==='global'?'global':'synth',...(p.scope==='global'?{}:{timbre:t}),parameter:p.id});
  for(let instrument=0;instrument<16;instrument++)for(const p of parameters)if(drums[instrument][p.id]!==program.drums[instrument][p.id])changed.push({kind:'drum',instrument,parameter:p.id});
  for (let i = 0; i < MAX_TIMBRES; i++) timbres[i].values = [...program.timbres[i]];
  for (let i = 0; i < 16; i++) drums[i] = [...program.drums[i]];
  if(program.effects)fxUI?.setConfig(program.effects);
  for(const target of changed)macroUI?.rebase(target);
  updateControls(); updateKeys();
}
function normalizeEngine(value){return checkEngine(value,parameters);}
function snapshot(){return {kind:'rustias-program',version:2,timbreCount,macros:macroUI?.getConfig()??null,modulation:modulationUI?.getConfig()??null,engine:state(),sequencer:sequenceUI.getConfig(),samples:sampleUI?.getConfig?.()??null,circuits:circuitUI?.getConfig()??emptyCircuits(timbres.map(t=>t.values)),timbreInfo:timbres.map(({name,preset,savedId,modified})=>({name,preset,savedId,modified})),volume:Number($('#volume').value),...(rdlSource?{rdl:structuredClone(rdlSource)}:{})};}
function loadSnapshot(value){
  const checked=normalizeProgram(value,parameters),{engine:program,samples,sequencer:sequence,circuits}=checked;
  stop();timbreCount=checked.timbreCount;selected=Math.min(selected,timbreCount-1);sequenceUI.setTimbreCount(timbreCount);editingSample=null;$('#error').hidden=true;rdlSource=checked.rdl??null;
  checked.timbreInfo.forEach((info,i)=>Object.assign(timbres[i],info));
  programState='custom';programDirty=false;
  circuitUI?.setConfig(circuits);fxUI?.setConfig(program.effects);acceptState(program);sequenceUI.setConfig(sequence);macroUI?.setConfig(checked.macros);modulationUI?.setConfig(checked.modulation);circuitUI?.select();fxUI?.select();
  if(checked.volume!=null){$('#volume').value=checked.volume;send({type:'gain',value:checked.volume/100});}
  const samplesReady=sampleUI?.setConfig(samples);loadEngine(program);send({type:'sequencer',config:sequence});
  samplesReady?.then(updateControls).catch(showError);
}
function scheduleSession(){clearTimeout(autosaveTimer);autosaveTimer=setTimeout(saveSession,300);}
function saveSession(){try{patchStore.saveSession({snapshot:snapshot(),activeSavedPatch,programState,programDirty,selected,volume:Number($("#volume").value)});}catch(error){showError(new Error(`Could not save this patch in the browser: ${error.message}`));}}
function refreshLibrary(){
  programPicker.options=[{value:'init',label:'New program'},{value:'custom',label:'Unsaved'},...patchStore.list().map(p=>({value:`saved:${p.id}`,label:p.snapshot.rdl?`${String(p.snapshot.rdl.slot+1).padStart(3,'0')} · ${p.name}`:p.name}))];
  timbrePicker.options=[...factorySounds,...timbreStore.list().map(p=>({value:`saved:${p.id}`,label:p.name}))];updateControls();
}
sequenceUI=createSequencer({inline:newInterface,onChange:configureSequence,onPlay:playSequence,onStop:stopSequence,onReset:()=>send({type:"sequence-reset"}),onSelectTimbre:t=>{$(`[data-timbre="${t}"]`).click();},onSelectDrum:(instrument,timbre)=>{if(selected!==timbre)$(`[data-timbre="${timbre}"]`).click();setControl(142,instrument,false);},onAudition:auditionStep,onError:showError,
  onSample:(timbre,source)=>sampleUI.ensureLibrary(timbre,source),onEditSample:(timbre,source)=>{if(selected!==timbre)$(`[data-timbre="${timbre}"]`).click();sampleUI.editProfile(timbre,source);editingSample={timbre,source};updateControls();scheduleSession();},
  onCopySamples:(from,to,sources)=>sampleUI.copyProfiles(from,to,sources),onUploadSample:(timbre,onReady)=>sampleUI.uploadForSequence(timbre,onReady)});
sequenceUI.setReady(false);
modulationUI=createModulationEditor({panels:sequenceUI.modulationPanels,getTargets:modulationTargets,onError:showError,onCounts:sequenceUI.setModulationCounts,onChange:(_config,t)=>{markTimbre(t);updateControls();scheduleSession();}});
function configureSequence(config){const request=++sequenceConfigRequest;markProgram();updateControls();scheduleSession();if(!node)return;if(context?.state!=="running"){send({type:"sequencer",config});return;}prepareSequenceSamples(config).then(()=>{if(request===sequenceConfigRequest)send({type:"sequencer",config});}).catch(error=>{if(request===sequenceConfigRequest)showError(error);});}
async function prepareSequenceSamples(config){await Promise.all(config.tracks.flatMap((track,timbre)=>track.enabled?[...new Set(track.steps.slice(0,track.length).flatMap(step=>step.samples??[]))].map(source=>sampleUI.ensureLibrary(timbre,source)):[]));}
async function playSequence(){const request=++sequenceRequest;try{await startAudio();await prepareSequenceSamples(sequenceUI.getConfig());if(request!==sequenceRequest)return;send({type:"sequencer",config:sequenceUI.getConfig()});send({type:"sequence-play"});sequenceUI.setStatus({running:true,positions:timbreArray()});}catch(error){showError(error);}}
async function auditionStep(timbre,step,resolution){const request=++auditionRequest;try{await startAudio();await Promise.all((step.samples??[]).map(source=>sampleUI.ensureLibrary(timbre,source)));if(request===auditionRequest)send({type:"audition",timbre,step,resolution});}catch(error){showError(error);}}
function stopSequence(){sequenceRequest++;sequenceConfigRequest++;send({type:"sequence-stop"});sequenceUI?.setStatus({running:false,positions:timbreArray(-1)});modulationUI?.setStatus({running:false,positions:timbreArray().map(()=>[])});macroUI?.setLiveValues(null);panel?.setModulations([]);}
function newPatchDialog(copy=false,kind='program'){
  patchDialogKind=kind;patchDialogSlot=selected;const store=kind==='program'?patchStore:timbreStore,records=store.list(),id=kind==='program'?activeSavedPatch:timbres[selected].savedId,existing=records.find(p=>p.id===id),base=existing?.name??(kind==='program'?'Program':timbres[selected].name);
  let name=`${kind==='program'?'Program':'Timbre'} ${String(records.length+1).padStart(2,'0')}`;
  if(copy){const names=new Set(records.map(p=>p.name));let n=1;do{const suffix=` copy${n===1?'':` ${n}`}`;name=base.slice(0,64-suffix.length)+suffix;n++;}while(names.has(name));}
  $('#patch-title').textContent=`Save ${kind}${copy?' copy':''}`;$('#patch-confirm').textContent=copy?'Save copy':`Save ${kind}`;$('#patch-name').value=name;
  $('#patch-editor').showModal();$('#patch-name').focus();$('#patch-name').select();
}
function saveCurrentPatch(){
  const existing=patchStore.list().find(p=>p.id===activeSavedPatch);if(!existing){newPatchDialog();return;}
  try{patchStore.save(existing.name,snapshot(),existing.id);programDirty=false;refreshLibrary();saveSession();}catch(error){showError(error);}
}
function saveCurrentTimbre(){
  const existing=timbreStore.list().find(p=>p.id===timbres[selected].savedId);if(!existing){newPatchDialog(false,'timbre');return;}
  try{timbreStore.save(existing.name,captureTimbre(snapshot(),selected,parameters,{sequence:INCLUDE_TIMBRE_SEQUENCE}),existing.id);timbres[selected].modified=false;markProgram();refreshLibrary();saveSession();}catch(error){showError(error);}
}
$('#save-patch').addEventListener('click',saveCurrentPatch);
const saveOptions=$('#patch-save-options'),saveMenu=$('#patch-save-menu');
function closeSaveMenu(returnFocus=false){if(saveMenu.hidePopover)saveMenu.hidePopover();else saveMenu.hidden=true;saveOptions.setAttribute('aria-expanded','false');if(returnFocus)saveOptions.focus();}
function openSaveMenu(){
  if(saveOptions.getAttribute('aria-expanded')==='true'){closeSaveMenu();return;}
  const rect=saveOptions.getBoundingClientRect(),width=Math.min(220,innerWidth-16);
  saveMenu.style.width=`${width}px`;saveMenu.style.left=`${Math.max(8,Math.min(rect.right-width,innerWidth-width-8))}px`;
  const below=innerHeight-rect.bottom,above=rect.top-12,down=below>=Math.min(240,above);saveMenu.style.top=down?`${rect.bottom+4}px`:'auto';saveMenu.style.bottom=down?'auto':`${innerHeight-rect.top+4}px`;saveMenu.style.maxHeight=`${Math.max(120,Math.min(320,down?below-12:above))}px`;
  if(saveMenu.showPopover)saveMenu.showPopover();else saveMenu.hidden=false;saveOptions.setAttribute('aria-expanded','true');$('#save-current-patch').focus();
}
saveOptions.addEventListener('click',openSaveMenu);saveOptions.addEventListener('keydown',event=>{if(event.key==='ArrowDown'){event.preventDefault();openSaveMenu();}});
saveMenu.addEventListener('toggle',event=>{if(event.newState==='closed')saveOptions.setAttribute('aria-expanded','false');});
saveMenu.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeSaveMenu(true);}else if(['ArrowUp','ArrowDown','Home','End'].includes(event.key)){event.preventDefault();const items=[...saveMenu.querySelectorAll('[role=menuitem]')],i=items.indexOf(document.activeElement);items[event.key==='Home'?0:event.key==='End'?items.length-1:(i+(event.key==='ArrowUp'?-1:1)+items.length)%items.length].focus();}else if(event.key==='Tab')closeSaveMenu(true);});
document.addEventListener('pointerdown',event=>{if(!saveMenu.contains(event.target)&&!saveOptions.contains(event.target))closeSaveMenu();});
$('#save-current-patch').addEventListener('click',()=>{closeSaveMenu(true);saveCurrentPatch();});$('#save-copy').addEventListener('click',()=>{closeSaveMenu();newPatchDialog(true);});$('#save-timbre').addEventListener('click',()=>{closeSaveMenu(true);saveCurrentTimbre();});$('#save-timbre-copy').addEventListener('click',()=>{closeSaveMenu();newPatchDialog(true,'timbre');});
$("#patch-cancel").addEventListener("click",()=>$("#patch-editor").close());$("#patch-form").addEventListener("submit",event=>{event.preventDefault();try{const name=$("#patch-name").value;if(patchDialogKind==='program'){const saved=patchStore.save(name,snapshot());activeSavedPatch=saved.id;programDirty=false;}else{const saved=timbreStore.save(name,captureTimbre(snapshot(),patchDialogSlot,parameters,{sequence:INCLUDE_TIMBRE_SEQUENCE}));Object.assign(timbres[patchDialogSlot],{name:saved.name,preset:'custom',savedId:saved.id,modified:false});markProgram();}refreshLibrary();saveSession();$("#patch-editor").close();}catch(error){showError(error);}});
window.addEventListener("pagehide",saveSession);

let timbreDownloadUrl;
$('#export-timbre').addEventListener('click',event=>{try{if(timbreDownloadUrl)URL.revokeObjectURL(timbreDownloadUrl);timbreDownloadUrl=URL.createObjectURL(new Blob([JSON.stringify({...captureTimbre(snapshot(),selected,parameters,{sequence:INCLUDE_TIMBRE_SEQUENCE}),name:timbres[selected].name},null,2)],{type:'application/json'}));event.currentTarget.href=timbreDownloadUrl;closeSaveMenu();}catch(error){event.preventDefault();showError(error);}});
let programDownloadUrl;
$("#save-program").addEventListener("click", event => {
  if (programDownloadUrl) URL.revokeObjectURL(programDownloadUrl);
  programDownloadUrl = URL.createObjectURL(new Blob([JSON.stringify(snapshot(), null, 2)], {type: "application/json"}));
  event.currentTarget.href = programDownloadUrl;
});
$("#load-program").addEventListener("click", () => $("#program-file").click());
function rdlReport(title,summary,notices){
  $('#rdl-title').textContent=title;$('#rdl-summary').textContent=summary;const list=$('#rdl-notices');list.replaceChildren();
  for(const notice of notices){const item=document.createElement('li');item.textContent=notice;list.append(item);}
  list.hidden=!notices.length;$('#rdl-report').showModal();
}
$('#rdl-close').addEventListener('click',()=>$('#rdl-report').close());$('#rdl-done').addEventListener('click',()=>$('#rdl-report').close());
$('#rdl-details').addEventListener('click',()=>{
  if(!rdlSource)return;
  const missing=rdlSource.unavailable.filter(s=>s.drum==null||!sampleUI?.assigned(s.drum));
  rdlReport('RDL patch',`${rdlSource.file} · Program ${String(rdlSource.slot+1).padStart(3,'0')}${rdlSource.drum_kit_name?` · ${rdlSource.drum_kit_name}`:''}`,[
    ...missing.map(s=>`${s.drum==null?`Timbre ${s.timbre+1}`:`Drum ${s.drum+1}`}: ${s.label} is unavailable. This instrument is muted until you choose an OSC 1 waveform${s.drum==null?'':' or assign a sample'}.`),...rdlSource.notices,
  ]);
});
$("#program-file").addEventListener("change", async event => {
  const button=$('#load-program');
  try {
    const file = event.target.files[0]; if (!file) return;
    if(file.size>MAX_RDL_BYTES)throw new Error('Choose a file up to 8 MiB.');
    button.disabled=true;button.textContent='Importing…';$('#error').hidden=true;const buffer=await file.arrayBuffer(),magic=new TextDecoder().decode(buffer.slice(0,4));
    if(/\.rdl$/i.test(file.name)||['316B','316P','316p'].includes(magic)){
      const digest=[...new Uint8Array(await crypto.subtle.digest('SHA-256',buffer))].map(b=>b.toString(16).padStart(2,'0')).join('');
      const library=await readRdl(module,buffer),imported=rdlPatches(library,file.name.slice(0,256),digest);
      // Validate every patch before the one atomic library write or any UI change.
      for(const patch of imported)normalizeProgram(patch.snapshot,parameters);
      let result;try{result=patchStore.import(imported);}catch(error){if(error.name==='QuotaExceededError')throw new Error('Browser storage is full. The RDL bank was not imported; your existing patches are unchanged.');throw error;}
      const first=patchStore.list().find(p=>p.id===imported[0].id);loadSnapshot(first.snapshot);activeSavedPatch=first.id;refreshLibrary();saveSession();
      const partial=imported.filter(p=>p.snapshot.rdl.unavailable.length||p.snapshot.rdl.notices.length).length;
      rdlReport('RDL imported',`${result.added} ${result.added===1?'program':'programs'} saved${result.duplicates?` · ${result.duplicates} already in your library`:''}. Choose them in Program.`,[
        ...(partial?[`${partial} ${partial===1?'program has':'programs have'} conversion details. Open RDL on the selected program to inspect them.`]:[]),
        ...(imported.some(p=>p.snapshot.rdl.unavailable.length)?['PCM/input-dependent instruments are muted until their source is replaced.']:[]),
      ]);
    }else{
      if(file.size>1024*1024)throw new Error('Choose a JSON patch up to 1 MiB.');
      const imported=JSON.parse(new TextDecoder().decode(buffer));
      if(imported.kind==='rustias-timbre'){const sound=normalizeTimbre(imported,parameters),saved=timbreStore.save(String(imported.name??file.name.replace(/\.json$/i,'')).slice(0,64),sound);loadTimbre(saved.snapshot,saved.name,saved.id);refreshLibrary();}
      else{loadSnapshot(imported);activeSavedPatch=null;programState='custom';programDirty=false;updateControls();scheduleSession();}
    }
  } catch (error) { showError(error); } finally { event.target.value = '';button.textContent='Import';button.disabled=!module; }
});
function noteLabel(note) { return `${keyNames[note % 12]}${Math.floor(note / 12) - 1}`; }
function updateKeys() {
  const drum = isDrum(); $("#keybed-title").textContent = drum ? "DRUM PADS" : "KEYBOARD";
  document.querySelectorAll(".key").forEach((key, index) => {
    const note = (octave + 1) * 12 + index; key.dataset.note = note;
    const label = drum ? `Drum ${String(index + 1).padStart(2, "0")}` : noteLabel(note);
    key.querySelector("span").textContent = drum ? String(index + 1).padStart(2, "0") : label; key.setAttribute("aria-label", label);
    key.setAttribute("aria-pressed", (noteCounts.get(drum ? `drum:${index}` : `${selected}:${note}`) ?? 0) > 0);
    key.classList.toggle('editing',drum&&global(142)===index);
    key.classList.toggle("black", !drum && [1, 3, 6, 8, 10].includes(index % 12));
  });
  $("#octave-label").textContent = drum ? "01–16" : `${noteLabel((octave + 1) * 12)}–${noteLabel((octave + 1) * 12 + 15)}`;
  $("#octave-down").disabled = drum || octave === 0; $("#octave-up").disabled = drum || octave === 7;
}
function startAudio() { if (audioStarting) return audioStarting; audioStarting = setupAudio().finally(() => { audioStarting = null; }); return audioStarting; }
async function setupAudio() {
  if (node && context?.state === "running") {await sampleUI.waitReady();return;}
  $("#error").hidden = true;
  if (context) { await context.resume(); updatePower(); return; }
  const AudioContextClass = window.AudioContext ?? window.webkitAudioContext;
  if (!AudioContextClass || !window.AudioWorkletNode) throw new Error("This browser does not support AudioWorklet. Try a current browser over HTTPS.");
  context = new AudioContextClass({sampleRate: 48000, latencyHint: "interactive"}); const pendingContext = context;
  try {
    await context.audioWorklet.addModule(new URL("./worklet.js", import.meta.url));
    const pendingNode = new AudioWorkletNode(context, "rustias", {numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2], processorOptions: {module, gain: Number($("#volume").value) / 100}});
    const ready = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("The Rust audio engine did not start.")), 10000);
      pendingNode.onprocessorerror = () => { clearTimeout(timeout); reject(new Error("The audio engine stopped unexpectedly.")); showError(new Error("The audio engine stopped unexpectedly.")); recorder?.stop(); };
      pendingNode.port.onmessage = ({data}) => {
        if(sampleUI?.handleMessage(data))return;
        if(recorder?.handleMessage(data))return;
        if (data.type === "ready") { clearTimeout(timeout); resolve(); }
        else if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); showError(new Error(data.message)); recorder?.stop(); }
        else if (data.type === "state") {const changed=(data.libraryVolumes??[]).filter(p=>sampleUI.findProfile(p.timbre,p.source)?.values[117]!==p.value);sampleUI.acceptVolumes(data.libraryVolumes,data.program,data.midi);for(const p of changed)macroUI.rebase({kind:'sample',timbre:p.timbre,source:p.source,parameter:117});acceptState(data.program);scheduleSession();}
        else if (data.type === "warning") showError(new Error(data.message));
        else if (data.type === "stats") {
          if(data.sequence)sequenceUI.setStatus(data.sequence);
          if(data.modulation){modulationUI.setStatus(data.modulation);macroUI.setLiveValues(data.modulation.macros);panel.setModulations(data.modulation.values);}
          $("#voice-capacity").textContent=data.capacity;$("#voices").textContent = data.voices; $("#meter").value = data.peak;
          document.body.dataset.frames = data.frames; document.body.dataset.audibleFrames = data.audibleFrames; document.body.dataset.peak = data.peak; document.body.dataset.callbacks = data.callbacks;
        }
      };
    });
    pendingNode.connect(context.destination); node = pendingNode;lastRdlMasks=null;loadEngine();send({type:"sequencer",config:sequenceUI.getConfig()});
    await context.resume(); await ready; await sampleUI.attach(pendingNode,context);syncAutomation(); context.onstatechange = updatePower; updatePower();
  } catch (error) { sampleUI?.detach(); node?.disconnect(); node = null; context = null; await pendingContext.close(); throw error; }
}
function updatePower() {
  const playing = context?.state === "running";
  $("#power").textContent = playing ? "Pause audio" : "Start audio"; $("#power").setAttribute("aria-pressed", !!playing);
  $("#status").textContent = playing ? `Native engine · ${context.sampleRate / 1000} kHz` : "Ready · no firmware required"; $("#led").classList.toggle("live", !!playing);
}
async function down(source, note, instrument) {
  if (held.has(source) || !module) return;
  if(isDrum())setControl(142,instrument,false);
  const entry = {timbre: selected, note, instrument: isDrum() ? instrument : undefined, pending: true, released: false}; held.set(source, entry);
  try { await startAudio(); } catch (error) { held.delete(source); showError(error); return; }
  if (held.get(source) !== entry) return;
  entry.pending = false; entry.startedAt = performance.now();
  const id = entry.instrument === undefined ? `${entry.timbre}:${entry.note}` : `drum:${entry.instrument}`, count = noteCounts.get(id) ?? 0;
  noteCounts.set(id, count + 1);
  if (!count) send(entry.instrument === undefined ? {type: "note", timbre: entry.timbre, note, velocity: 100} : {type: "drum", instrument: entry.instrument, velocity: 100});
  updateKeys(); if (entry.released) up(source);
}
function up(source, force = false) {
  const entry = held.get(source); if (!entry) return;
  if (entry.pending) { if (force) held.delete(source); else entry.released = true; return; }
  const remaining = 32 - (performance.now() - entry.startedAt);
  if (!force && remaining > 0) { entry.releaseTimer ??= setTimeout(() => up(source, true), remaining); return; }
  clearTimeout(entry.releaseTimer); held.delete(source);
  const id = entry.instrument === undefined ? `${entry.timbre}:${entry.note}` : `drum:${entry.instrument}`, count = Math.max(0, (noteCounts.get(id) ?? 0) - 1);
  if (!count) { noteCounts.delete(id); send(entry.instrument === undefined ? {type: "note", timbre: entry.timbre, note: entry.note, velocity: 0} : {type: "drum", instrument: entry.instrument, velocity: 0}); } else noteCounts.set(id, count);
  updateKeys();
}
function releaseAll() { for (const source of [...held.keys()]) up(source, true); }
function stop() { auditionRequest++; stopSequence(); releaseAll(); send({type: "stop"}); $("#voices").textContent = "0"; $("#meter").value = 0; }
for (let index = 0; index < 16; index++) {
  const key = document.createElement("button"); key.type = "button"; key.className = "key"; key.innerHTML = `<span></span><small>${computerKeys[index].toUpperCase()}</small>`; $("#keys").append(key);
  key.addEventListener("pointerdown", event => { if (event.button !== 0 || !module) return; event.preventDefault(); key.setPointerCapture(event.pointerId); down(`pointer-${event.pointerId}`, Number(key.dataset.note), index); });
  const release = event => up(`pointer-${event.pointerId}`, event.type === "pointercancel"); for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) key.addEventListener(event, release);
  key.addEventListener("keydown", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); if (!event.repeat) down(`pad-${index}`, Number(key.dataset.note), index); } });
  key.addEventListener("focusout", () => up(`pad-${index}`)); key.addEventListener("keyup", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); up(`pad-${index}`); } });
}
document.addEventListener("keydown", event => {
  if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || !module || (isChoosing() || saveOptions.getAttribute('aria-expanded')==='true' || event.target.closest("input,select,dialog,[role=slider],[role=combobox],[role=option],.key"))) return;
  const key = event.key.toLowerCase(), index = computerKeys.indexOf(key); if (index >= 0) { event.preventDefault(); down(`key-${key}`, (octave + 1) * 12 + index, index); } if (event.key === "Escape") stop();
});
document.addEventListener("keyup", event => up(`key-${event.key.toLowerCase()}`)); window.addEventListener("blur", releaseAll);
document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
$("#octave-down").addEventListener("click", () => { releaseAll(); octave = Math.max(0, octave - 1); updateKeys(); });
$("#octave-up").addEventListener("click", () => { releaseAll(); octave = Math.min(7, octave + 1); updateKeys(); });
$("#volume").addEventListener("input", event => {send({type: "gain", value: Number(event.target.value) / 100});macroUI?.rebase({kind:'volume'});markProgram();updateControls();scheduleSession();});
$("#panic").addEventListener("click", stop);
$("#power").addEventListener("click", async () => { try { if (context?.state === "running") { await recorder?.stop(); stop(); await context.suspend(); updatePower(); } else await startAudio(); } catch (error) { showError(error); } });
$("#midi").addEventListener("click", async () => {
  try {
    if (!navigator.requestMIDIAccess) throw new Error("Web MIDI is unavailable in this browser. The keyboard and pads still work.");
    midiAccess ??= await navigator.requestMIDIAccess({sysex: false}); await startAudio();
    const connect = () => { for (const input of midiAccess.inputs.values()) input.onmidimessage = event => { if (event.data.length === 3) send({type: "midi", bytes: [...event.data]}); }; $("#midi").textContent = midiAccess.inputs.size ? `MIDI · ${midiAccess.inputs.size}` : "MIDI · no device"; }; connect(); midiAccess.onstatechange = connect;
  } catch (error) { showError(error); }
});
const fullscreenButton=$("#fullscreen");
function updateFullscreen(){const active=!!(document.fullscreenElement??document.webkitFullscreenElement);fullscreenButton.setAttribute("aria-pressed",active);fullscreenButton.setAttribute("aria-label",active?"Exit full screen":"Full screen");fullscreenButton.title=active?"Exit full screen":"Full screen";document.body.dataset.fullscreen=active?"on":"off";}
fullscreenButton.addEventListener("click",async()=>{
  try {
    if(document.fullscreenElement)await document.exitFullscreen();
    else if(document.webkitFullscreenElement)document.webkitExitFullscreen();
    else if(document.documentElement.requestFullscreen)await document.documentElement.requestFullscreen();
    else if(document.documentElement.webkitRequestFullscreen)document.documentElement.webkitRequestFullscreen();
    else throw new Error("Open this instrument in a browser that supports full screen.");
    updateFullscreen();
  }catch(error){showError(error);}
});
document.addEventListener("fullscreenchange",updateFullscreen);document.addEventListener("webkitfullscreenchange",updateFullscreen);updateFullscreen();
function prepareSampleInstrument(instrument){
  for(const [parameter,value] of [[3,0],[4,127],[5,127],[6,32],[9,0],[1,127],[2,0],[114,0],[115,32512],[116,0],[117,127]]){drums[instrument][parameter]=value;send({type:"drum-control",instrument,parameter,value});macroUI?.rebase({kind:'drum',instrument,parameter});}
}
function enableDrumKit(){if(!global(140))setControl(140,1);if(!isDrum())$(`[data-timbre="${global(141)}"]`).click();}
sampleUI=createDrumSamples({parameters,getDrumValues:i=>drums[i],getEditingSample:()=>editingSample,onLibraryChange:()=>sequenceUI.setSamples(sampleUI?.options()??[]),onPreview:(timbre,source)=>auditionStep(timbre,{notes:[],samples:[source],velocity:100,gate:100},"1/4"),getInstrument:()=>global(142),isDrum,ensureAudio:startAudio,onError:showError,onChange:target=>{macroUI?.rebase(target);markProgram();updateControls();scheduleSession();},onAssign:(instrument,fresh)=>{enableDrumKit();if(fresh)prepareSampleInstrument(instrument);},onKit:bank=>{
  enableDrumKit();for(let i=0;i<16;i++){prepareSampleInstrument(i);for(const [parameter,value] of [[146,60+i],[147,([3,4].includes(i)||bank==='909'&&[14,15].includes(i))?1:0]]){drums[i][parameter]=value;send({type:"drum-control",instrument:i,parameter,value});}}
  setControl(142,0);
}});
sampleUI.ready.then(()=>{sequenceUI.setSamples(sampleUI.options());updateControls();},()=>{});
circuitUI=createCircuitEditor({getValues:values,getSelected:()=>selected,onError:showError,onChange:(_config,audio,target)=>{macroUI?.rebase(target);markTimbre();if(audio){$("#error").hidden=true;sendCircuits(selected);}scheduleSession();}});
fxUI=createEffectsPanel({getSelected:()=>selected,onError:showError,onChange:(slot,program,parameter)=>{if(parameter!=null)macroUI?.rebase({kind:'effect',slot,effectKind:program.kind,parameter});const owner=effectTimbre(slot);owner==null?markProgram():markTimbre(owner);send({type:'effect',slot,program});updateControls();scheduleSession();}});
macroUI=createMacrosPanel({resolve:resolveMacro,write:writeMacros,onError:showError,onChange:()=>{markProgram();updateControls();scheduleSession();}});bindMacroTarget($('#volume'),()=>({kind:'volume'}));
recorder=createRecorder({ensureAudio:startAudio,getAudio:()=>context,send,onError:showError,getProgram:()=>{const saved=patchStore.list().find(p=>p.id===activeSavedPatch);return {key:saved?`program:${saved.id}`:'unsaved',name:saved?.name??'Unsaved program'};}});
newUI?.connect({sequenceUI,fxUI,circuitUI,macroUI,resolveMacro,releaseAll});
refreshLibrary();
try{const session=patchStore.session();if(session?.snapshot){loadSnapshot(session.snapshot);activeSavedPatch=session.activeSavedPatch??null;programState=session.programState??"custom";programDirty=session.programDirty??false;if(Number.isInteger(session.selected)&&session.selected>=0&&session.selected<timbreCount)$(`[data-timbre="${session.selected}"]`).click();if(Number.isFinite(session.volume))$("#volume").value=session.volume;}}catch(error){showError(new Error(`Could not restore the saved session: ${error.message}`));}
updateControls(); updateKeys(); document.body.dataset.parameterCount = parameters.length;
try {
  const response = await fetch(new URL("./rustias.wasm", import.meta.url)); if (!response.ok) throw new Error(`Could not load the Rust engine (${response.status}).`);
  module = await WebAssembly.compile(await response.arrayBuffer()); sequenceUI.setReady(true);recorder.setReady(true); $("#power").disabled = false;$('#load-program').disabled=false;updatePower(); document.body.dataset.engine = "ready";
} catch (error) { $("#status").textContent = "Engine unavailable"; showError(error); }
