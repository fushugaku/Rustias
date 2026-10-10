import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import {MAX_TIMBRES,EFFECT_SLOTS,MASTER_EFFECT_SLOT} from '../web/limits.js';
import {defaultEffect} from '../web/effects.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';
import {emptySequence,SequenceClock,StepAudition} from '../web/sequence.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {MacroBank,normalizeMacros,targetKey} from '../web/macros.js';
import {effectChoices,effectValueLabel,effectFieldState} from '../web/effect-display.js';
export function verifyExpanded({api,parameters,catalog,control,render,rms,save,load}){
  assert.equal(api.rustias_timbre_capacity(),MAX_TIMBRES);assert.equal(api.rustias_voice_capacity(),128);
  api.rustias_init();assert.equal(api.rustias_note(MAX_TIMBRES,60,100),0);assert.equal(api.rustias_control(MAX_TIMBRES,71,1),0);
  function sound(t,patch=0){api.rustias_init();control(71,1,t);control(3,0,t);control(4,127,t);control(5,127,t);control(6,8,t);if(patch){const b=patch===7?155:159;control(b,4,t);control(b+1,11,t);control(b+2,110,t);control(83,86,t);}assert.equal(api.rustias_note(t,69,100),1);return render(12000);}
  for(let t=0;t<MAX_TIMBRES;t++)assert.ok(rms(sound(t))>.0001,`Timbre ${t+1} produces audio`);
  const dry=sound(7);for(const route of [7,8]){const audio=sound(7,route);assert.ok(rms(audio.map((v,i)=>v-dry[i]))>.0001,`Patch ${route} affects the eighth timbre's real DSP`);}
  for(const destination of [40,41])control(91,destination,7);
  api.rustias_init();for(let t=0;t<MAX_TIMBRES;t++){control(71,1,t);control(78,1,t);control(79,t,t);control(7,40,t);for(const n of [60,64,67])api.rustias_note(t,n,100);}
  assert.equal(api.rustias_voices(),24,'Eight independent three-note chords share one pool');assert.ok(rms(render())>.0001);api.rustias_midi(0xb7,123,0);render(20000);assert.equal(api.rustias_voices(),21,'MIDI channel eight releases only its own chord');api.rustias_stop();
  // The eighth timbre can own either native synthesis drums or uploaded PCM.
  control(141,7);control(140,1);assert.equal(api.rustias_drum_pad(0,100),1);assert.ok(rms(render())>.0001,'Native drum kit accepts owner eight');api.rustias_stop();
  const pcm=Float32Array.from({length:4800},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));const pointer=api.rustias_sample_buffer(0,pcm.length);new Float32Array(api.memory.buffer,pointer,pcm.length).set(pcm);assert.equal(api.rustias_sample_commit(0,pcm.length,2),1);api.rustias_drum_pad(0,100);assert.ok(rms(render())>.0001,'Kit PCM reaches the eighth output bus');api.rustias_stop();
  const stage=api.rustias_library_sample_buffer(77,pcm.length);new Float32Array(api.memory.buffer,stage,pcm.length).set(pcm);assert.equal(api.rustias_library_sample_commit(77,pcm.length),1);assert.equal(api.rustias_library_profile(77,77,7,2),1);api.rustias_library_sync();assert.equal(api.rustias_library_note(7,77,100),1);assert.ok(rms(render())>.0001,'Direct step samples accept timbre eight');api.rustias_stop();
  function sendJson(fn,args,data){const b=Buffer.from(JSON.stringify(data));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),b.length).set(b);assert.equal(fn(...args,b.length),1);}
  api.rustias_init();control(71,1,7);const circuit=defaultCircuit();circuit.enabled=true;sendJson(api.rustias_circuit,[7],audioCircuit(circuit));api.rustias_note(7,69,100);assert.ok(rms(render())>.0001,'Eighth timbre has its own modular graph');api.rustias_stop();
  const fx=defaultEffect(7);sendJson(api.rustias_effect,[14],fx);sendJson(api.rustias_effect,[15],defaultEffect(20));sendJson(api.rustias_effect,[MASTER_EFFECT_SLOT],defaultEffect(11,true));const engine=save();assert.equal(engine.effects.slots.length,EFFECT_SLOTS);assert.equal(load(engine),1);assert.deepEqual(save(),engine,'Eight native timbres, extra routes and all 33 effect slots round-trip');
  const legacy={version:1,timbres:engine.timbres.slice(0,4).map(v=>v.slice(0,155)),drums:engine.drums.map(v=>v.slice(0,155)),effects:{version:1,slots:[...engine.effects.slots.slice(0,8),engine.effects.slots[MASTER_EFFECT_SLOT]]}};assert.equal(load(legacy),1);const migrated=save();assert.deepEqual(migrated.timbres.slice(0,4).map(v=>v.slice(0,155)),legacy.timbres,'Legacy native values are lossless');assert.ok(migrated.timbres.slice(4).every(v=>v[71]===0),'Unused new timbres remain silent');assert.deepEqual(migrated.effects.slots[MASTER_EFFECT_SLOT],legacy.effects.slots[8],'Legacy Master moves to its stable shared slot');
  const normalized=normalizeProgram(legacy,parameters);assert.equal(normalized.timbreCount,4);assert.equal(normalized.macros.knobs.length,8);normalized.timbreCount=8;normalized.engine.timbres[7][71]=1;normalized.sequencer.tracks[7].enabled=true;normalized.sequencer.tracks[7].length=128;normalized.sequencer.tracks[7].steps[127].notes=[60,64];normalized.circuits.tracks[7]=circuit;
  const moved=applyTimbre(normalized,6,captureTimbre(normalized,7,parameters),parameters);assert.deepEqual(moved.circuits.tracks[6],circuit);assert.deepEqual(moved.sequencer,normalized.sequencer,'Saving/loading timbre eight preserves all eight patterns');
  const sequence=emptySequence();sequence.tracks.forEach((t,i)=>{t.enabled=true;t.steps[0].notes=[60+i,64+i];});const notes=[],clock=new SequenceClock((t,n,v)=>notes.push({t,n,v}));clock.setConfig(sequence);clock.play();clock.beforeRender();assert.equal(notes.length,16);assert.equal(new Set(notes.map(n=>n.t)).size,8,'All eight chords start at the same frame');clock.stop();assert.equal(notes.filter(n=>n.v===0).length,16);const audition=[];new StepAudition((t,n,v)=>audition.push({t,n,v})).play(7,{notes:[60,64],velocity:100,gate:75},120);assert.equal(audition.length,2);assert.ok(audition.every(n=>n.t===7));
  // Every FX choice has a readable label and retains its exact stored index.
  for(const master of [false,true])for(const def of catalog[master?'master':'insert'].slice(1))for(const [index,p]of def.properties.entries()){
    const choices=effectChoices(def,master,index,def.defaults);assert.equal(choices.length,p.max-p.min+1);assert.ok(choices.every(o=>typeof o.label==='string'&&o.label.length&&!/NaN|undefined/.test(o.label)),`${def.name} ${p.name} labels`);assert.deepEqual(choices.map(o=>o.value),Array.from({length:p.max-p.min+1},(_,i)=>p.min+i));
  }
  const delay=catalog.insert[14],params=[...delay.defaults];params[2]=1;assert.equal(effectValueLabel(delay,false,6,3,params),'1/16');assert.equal(effectValueLabel(delay,false,7,1,params),'1/32');assert.equal(effectFieldState(delay,false,4,params).hidden,true);assert.equal(effectFieldState(delay,false,6,params).hidden,false);params[2]=0;assert.equal(effectFieldState(delay,false,6,params).hidden,true);assert.equal(effectValueLabel(delay,false,4,127,params),'500 ms');
  const eq=catalog.master[6];assert.equal(effectValueLabel(eq,true,4,0,eq.defaults),'20 Hz');assert.equal(effectValueLabel(eq,true,6,-36,eq.defaults),'-18.0 dB');assert.equal(effectValueLabel(eq,true,5,95,eq.defaults),'10.0');
  const dynamics=catalog.insert[3];const attack=dynamics.properties.findIndex(p=>p.name==='Attack'),release=dynamics.properties.findIndex(p=>p.name==='Release');assert.equal(effectValueLabel(dynamics,false,attack,0,dynamics.defaults),'0.1 ms');assert.equal(effectValueLabel(dynamics,false,attack,127,dynamics.defaults),'500.0 ms');assert.equal(effectValueLabel(dynamics,false,release,127,dynamics.defaults),'1500.0 ms');
  assert.deepEqual(effectChoices(catalog.master[11],true,1,catalog.master[11].defaults).map(p=>p.label),['Hall','SmoothHall','WetPlate','DryPlate','Room','BritRoom']);assert.equal(effectValueLabel(delay,true,4,127,params),'700 ms');
  // Macro amounts are additive, reversible and preserve a direct knob edit.
  const values=new Map(),resolve=target=>({label:targetKey(target),min:0,max:127,read:()=>values.get(targetKey(target))??64}),bank=new MacroBank({resolve,write:changes=>changes.forEach(c=>values.set(targetKey(c.target),c.value))});
  const a={kind:'synth',timbre:0,parameter:1},b={kind:'synth',timbre:7,parameter:1};bank.assign(0,a);bank.assign(0,b);bank.setAmount(0,b,-50);bank.setValue(0,50);assert.equal(values.get(targetKey(a)),127);assert.equal(values.get(targetKey(b)),32);bank.setValue(0,0);assert.equal(values.get(targetKey(a)),64);assert.equal(values.get(targetKey(b)),64);
  for(let i=0;i<100;i++){bank.setValue(0,100);bank.setValue(0,0);}assert.equal(values.get(targetKey(b)),64,'Repeated movement never accumulates offsets');
  bank.assign(1,b);bank.setAmount(1,b,25);bank.setValue(1,100);assert.equal(values.get(targetKey(b)),96);bank.setValue(0,50);assert.equal(values.get(targetKey(b)),64,'Opposite macros sum around one shared base');
  values.set(targetKey(b),50);bank.rebase(b);bank.setValue(0,0);assert.equal(values.get(targetKey(b)),82,'A manual edit rebases the mapping without a jump');bank.setValue(0,50);assert.equal(values.get(targetKey(b)),50);
  const restored=new MacroBank({resolve,write:bank.write});restored.setConfig(JSON.parse(JSON.stringify(bank.getConfig())));restored.setValue(0,50);assert.equal(values.get(targetKey(b)),50,'Persisted macros do not apply their offsets twice');
  for(let i=2;i<12;i++)bank.assign(0,{kind:'synth',timbre:7,parameter:32+i});const before=bank.getConfig();assert.throws(()=>bank.assign(0,{kind:'synth',timbre:7,parameter:60}),/12/);assert.deepEqual(bank.getConfig(),before,'The thirteenth binding is rejected atomically');bank.assign(0,a);assert.equal(bank.config.knobs[0].bindings.length,12,'Assigning an existing parameter does not duplicate it');
  normalized.macros=bank.getConfig();assert.deepEqual(normalizeProgram(JSON.parse(JSON.stringify(normalized)),parameters).macros,bank.getConfig(),'Program save retains all macro values, strengths and bases');const invalid=bank.getConfig();invalid.knobs[0].bindings[0].amount=-101;assert.throws(()=>normalizeMacros(invalid));
  verifyMacroGesture();api.rustias_stop();return ['Eight browser timbres: independent DSP, synchronized chords, PCM/library drums, MIDI, modular graphs, 33 FX slots, storage and legacy migration','Patch 7/8 real audio and feedback destinations; native parameter encoding retained','All FX value labels/indices, tempo-dependent fractions, Hz/dB/ms and EQ units','Eight program macros: twelve bindings, bipolar strengths, additive bases, manual edits, reversible movement and persistence','Macro assignment: right click, keyboard and touch hold; movement/cancellation and click suppression'];
}
function verifyMacroGesture(){
  const callbacks=new Map();let next=0,assignments=0,released=0;
  const context=vm.createContext({setTimeout:fn=>{callbacks.set(++next,fn);return next;},clearTimeout:id=>callbacks.delete(id)});
  const source=fs.readFileSync(new URL('../web/macro-gesture.js',import.meta.url),'utf8').replaceAll('export ','');vm.runInContext(`${source}\nglobalThis.gesture={bindMacroTarget,setMacroAssignmentHandler};`,context);
  const listeners=new Map(),target={kind:'synth',timbre:7,parameter:1},element={disabled:false,addEventListener(name,fn){if(!listeners.has(name))listeners.set(name,[]);listeners.get(name).push(fn);},hasPointerCapture:()=>true,releasePointerCapture:()=>released++};
  const fire=(name,fields={})=>{const event={preventDefault(){this.prevented=true;},stopImmediatePropagation(){this.stopped=true;},...fields};for(const fn of listeners.get(name)??[])fn(event);return event;};
  const hold=()=>{for(const [id,fn]of [...callbacks]){callbacks.delete(id);fn();}};
  context.gesture.setMacroAssignmentHandler(t=>{assert.equal(t,target);assignments++;});context.gesture.bindMacroTarget(element,()=>target);
  assert.ok(fire('contextmenu').prevented);fire('keydown',{key:'F10',shiftKey:true});assert.equal(assignments,2);
  const pointer={pointerType:'touch',button:0,pointerId:1,clientX:10,clientY:20};fire('pointerdown',pointer);hold();assert.equal(assignments,3);assert.equal(released,1);assert.ok(fire('click').stopped);assert.ok(!fire('click').stopped);
  fire('pointerdown',pointer);fire('pointermove',{clientX:17,clientY:20});hold();assert.equal(assignments,3,'Dragging the dial cancels touch assignment');
  fire('pointerdown',pointer);fire('pointercancel');hold();assert.equal(assignments,3);element.disabled=true;fire('pointerdown',pointer);hold();fire('contextmenu');assert.equal(assignments,3,'Disabled controls cannot be assigned');
}
