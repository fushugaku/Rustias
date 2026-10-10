import {MAX_TIMBRES,EFFECT_SLOTS,MASTER_EFFECT_SLOT,EXTRA_EFFECT_START,timbreEffectSlot,timbreEffectSlots,effectUsesMaster} from '../web/limits.js';
import assert from 'node:assert/strict';
import {setEffectCatalog,defaultEffect,emptyEffects,validateEffects,effectsFromRdl} from '../web/effects.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {syntheticRdl} from './verify-rdl.mjs';
import {parseRdl,rdlPatches} from '../web/rdl.js';
import {newModulationLane,emptyModulation} from '../web/modulation.js';
export function readEffectCatalog(api){const n=api.rustias_effect_catalog(),catalog=JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_effect_catalog_buffer(),n)));setEffectCatalog(catalog);return catalog;}
export function verifyEffects({api,catalog,render,rms,control,save,load,parameters,module}){
  function effect(slot,p,accepted=1){const b=Buffer.from(JSON.stringify(p));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),b.length).set(b);assert.equal(api.rustias_effect(slot,b.length),accepted);}
  function voice(slot,kind,timbre=0){api.rustias_init();control(71,1,timbre);control(3,0,timbre);control(4,127,timbre);control(5,127,timbre);control(6,8,timbre);if(kind){const fx=defaultEffect(kind,effectUsesMaster(slot));if(kind===6)fx.parameters[6]=76;effect(slot,fx);}api.rustias_note(timbre,69,100);return render(30000);}
  const masterAudio=new Map();
  const dry=voice(0,0);for(const master of [false,true]){
    const defs=catalog[master?'master':'insert'];assert.equal(defs.length,31);
    for(let kind=1;kind<=30;kind++){
      const slot=master?MASTER_EFFECT_SLOT:0,audio=voice(slot,kind);assert.ok(rms(audio)>.000001,`${defs[kind].name} produces audio`);assert.ok(rms(audio.map((v,i)=>v-dry[i]))>.000001,`${defs[kind].name} processes the original signal`);
      if(master)masterAudio.set(kind,audio);
      const p=defaultEffect(kind,master);p.enabled=false;effect(slot,p);const bypass=render(5000);assert.ok(rms(bypass)>.001,'Effect bypass restores the dry sound');
      for(const [i,prop]of defs[kind].properties.entries())for(const value of [prop.min,prop.max]){const boundary=defaultEffect(kind,master);boundary.parameters[i]=value+prop.zero;effect(slot,boundary);render(128);}
    }
  }
  for(const role of [2,3])for(let kind=1;kind<=30;kind++)assert.deepEqual(voice(timbreEffectSlot(0,role),kind),masterAudio.get(kind),`FX ${role+1} has the same ${catalog.master[kind].name} processing as Master on a single active timbre`);
  for(let t=1;t<MAX_TIMBRES;t++)for(const role of [2,3])assert.deepEqual(voice(timbreEffectSlot(t,role),7),dry,`FX ${role+1} on timbre ${t+1} leaves timbre 1 untouched`);
  const distortion=defaultEffect(7,true),filter=defaultEffect(4,true);distortion.parameters[1]=110;filter.parameters[1]=0;filter.parameters[2]=45;
  function chain(reverse=false){api.rustias_init();control(3,0);control(4,127);control(5,127);control(6,8);effect(timbreEffectSlot(0,2),reverse?filter:distortion);effect(timbreEffectSlot(0,3),reverse?distortion:filter);api.rustias_note(0,69,100);return render(12000);}
  const ordered=chain(),reversed=chain(true);assert.ok(rms(ordered.map((v,i)=>v-reversed[i]))>.00001,'FX 3 and FX 4 run serially in order');
  const silencer=defaultEffect(4,true);silencer.parameters[4]=0;api.rustias_init();control(3,0,1);control(4,127,1);control(5,127,1);control(6,8,1);api.rustias_note(1,72,100);const other=render(12000);
  api.rustias_init();for(const t of [0,1]){control(3,0,t);control(4,127,t);control(5,127,t);control(6,8,t);}effect(timbreEffectSlot(0,2),silencer);api.rustias_note(1,72,100);api.rustias_note(0,69,100);assert.deepEqual(render(12000),other,'FX 3 Trim can silence its own bus while another timbre passes unchanged to the shared Master');
  const unaffected=voice(2,7,0);assert.deepEqual(unaffected,dry,'A timbre insert never processes another timbre');
  const sine=Float32Array.from({length:4800},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));
  function pcm(kind,library=false,fxTimbre=library?2:0,role=0){api.rustias_init();const values=parameters.map(p=>p.default);for(const [id,v]of [[3,0],[4,127],[5,127],[6,8],[9,127]])values[id]=v;
    if(library){const pointer=api.rustias_library_sample_buffer(99,sine.length);new Float32Array(api.memory.buffer,pointer,sine.length).set(sine);assert.equal(api.rustias_library_sample_commit(99,sine.length),1);assert.equal(api.rustias_library_profile(99,99,2,2),1);values.forEach((v,id)=>api.rustias_library_control(99,id,v));api.rustias_library_sync();}
    else{control(140,1);for(const id of [3,4,5,6,9])control(id,values[id]);const pointer=api.rustias_sample_buffer(0,sine.length);new Float32Array(api.memory.buffer,pointer,sine.length).set(sine);assert.equal(api.rustias_sample_commit(0,sine.length,2),1);}
    if(kind){const slot=timbreEffectSlot(fxTimbre,role);effect(slot,defaultEffect(kind,effectUsesMaster(slot)));}library?api.rustias_library_note(2,99,100):api.rustias_drum_pad(0,100);return render(12000);
  }
  const pcmDry=pcm(0),pcmFx=pcm(7);assert.ok(rms(pcmFx.map((v,i)=>v-pcmDry[i]))>.00001,'Drum Kit PCM passes through its owning timbre inserts');
  const libraryDry=pcm(0,true),libraryFx=pcm(7,true);assert.ok(rms(libraryFx.map((v,i)=>v-libraryDry[i]))>.00001,'Direct sequencer samples pass through their own timbre inserts');assert.deepEqual(pcm(7,true,0),libraryDry,'A different timbre insert leaves the sequencer sample intact');
  for(const role of [2,3]){assert.ok(rms(pcm(7,false,0,role).map((v,i)=>v-pcmDry[i]))>.00001,`Kit PCM reaches FX ${role+1}`);assert.ok(rms(pcm(7,true,2,role).map((v,i)=>v-libraryDry[i]))>.00001,`Sequencer PCM reaches FX ${role+1}`);assert.deepEqual(pcm(7,true,0,role),libraryDry,'Extra FX preserve PCM timbre ownership');}
  api.rustias_init();const delay=defaultEffect(14);delay.parameters[0]=100;delay.parameters[2]=0;delay.parameters[4]=10;delay.parameters[5]=10;effect(0,delay);api.rustias_note(0,69,100);render(24000);api.rustias_stop();assert.equal(api.rustias_voices(),0);assert.ok(rms(render(6000))>.00001,'An effect tail survives the end of all voices');
  delay.parameters[0]=70;effect(0,delay);assert.ok(rms(render(128))>.00001,'A live parameter edit preserves the delay state');effect(0,defaultEffect());effect(0,delay);assert.ok(rms(render(6000))<.000001,'Changing the effect type clears obsolete delay state');
  api.rustias_init();const originals=save();const invalid=defaultEffect(14);invalid.parameters[0]=255;effect(0,invalid,0);assert.deepEqual(save(),originals,'Invalid live effect edits are atomic');const wrongBank=defaultEffect(14,true);effect(0,wrongBank,0);assert.deepEqual(save(),originals);
  const valid=defaultEffect(14);effect(0,valid);effect(timbreEffectSlot(7,2),defaultEffect(6,true));effect(timbreEffectSlot(7,3),defaultEffect(11,true));const edited=save();assert.deepEqual(edited.effects.slots[0],valid);assert.equal(edited.effects.slots.length,33);assert.equal(load(edited),1);assert.deepEqual(save(),edited,'All 33 effect slots survive the Rust program round trip');
  const legacy17=structuredClone(edited);legacy17.effects.slots=legacy17.effects.slots.slice(0,EXTRA_EFFECT_START);assert.equal(load(legacy17),1);const upgraded=save();assert.deepEqual(upgraded.effects.slots.slice(0,EXTRA_EFFECT_START),legacy17.effects.slots,'Original 17 slot indices, settings and Master remain unchanged');assert.ok(upgraded.effects.slots.slice(EXTRA_EFFECT_START).every(p=>p.master&&!p.enabled&&p.kind===0),'Legacy programs gain bypassed master-capable timbre slots');assert.equal(load(edited),1);
  const badExtra=structuredClone(edited);badExtra.effects.slots[EXTRA_EFFECT_START].master=false;assert.equal(load(badExtra),0);assert.deepEqual(save(),edited,'An invalid extra FX bank leaves the complete instrument intact');effect(EXTRA_EFFECT_START,defaultEffect(14,false),0);
  const bad=structuredClone(edited);bad.effects.slots[MASTER_EFFECT_SLOT].master=false;assert.equal(load(bad),0);assert.deepEqual(save(),edited,'Invalid effect bank leaves the current instrument unchanged');
  const legacy=structuredClone(originals);delete legacy.effects;assert.equal(load(legacy),1);assert.deepEqual(save().effects,emptyEffects(),'Legacy native patches load with bypassed effects');
  const program=normalizeProgram(edited,parameters);program.engine.effects.slots[4]=defaultEffect(11);program.engine.effects.slots[5]=defaultEffect(20);program.engine.effects.slots[MASTER_EFFECT_SLOT]=defaultEffect(12,true);
  program.engine.effects.slots[timbreEffectSlot(2,2)]=defaultEffect(6,true);program.engine.effects.slots[timbreEffectSlot(2,3)]=defaultEffect(11,true);const lane=newModulationLane('fx3');lane.target={kind:'effect',slot:timbreEffectSlot(2,2),effectKind:6,parameter:6};program.modulation=emptyModulation();program.modulation.tracks[2]=[lane];
  const sound=captureTimbre(program,2,parameters),applied=applyTimbre(program,1,sound,parameters);
  assert.deepEqual(timbreEffectSlots(1).map(slot=>applied.engine.effects.slots[slot]),sound.effects,'All four effects follow a saved timbre');assert.deepEqual(applied.engine.effects.slots[MASTER_EFFECT_SLOT],program.engine.effects.slots[MASTER_EFFECT_SLOT],'A timbre does not replace the master effect');assert.deepEqual(applied.sequencer,program.sequencer);assert.equal(applied.modulation.tracks[1][0].target.slot,timbreEffectSlot(1,2),'Owned FX 3 modulation follows a timbre to its new slot');
  const legacySound={...structuredClone(sound),effects:sound.effects.slice(0,2)},oldApplied=applyTimbre(program,1,legacySound,parameters);assert.ok(timbreEffectSlots(1).slice(2).every(slot=>oldApplied.engine.effects.slots[slot].master&&!oldApplied.engine.effects.slots[slot].enabled),'Older two-insert timbre sounds migrate safely');
  const fixture=syntheticRdl(2);for(let slot=0;slot<9;slot++){const master=slot===8,offset=master?1038:168+228*Math.floor(slot/2)+24*(slot%2),fx=defaultEffect(14,master);fixture.programs[0][offset]=128|fx.kind;Buffer.from(fx.parameters).copy(fixture.programs[0],offset+(master?2:4));}
  const chunk=Buffer.alloc(32+fixture.programs[0].length);chunk.write('316p');chunk.writeUInt32LE(32,4);chunk.writeUInt32LE(fixture.programs[0].length,8);fixture.programs[0].copy(chunk,32);
  const imported=parseRdl(new WebAssembly.Instance(module,{}).exports,chunk).programs[0];assert.deepEqual(imported.engine.effects.slots,Array.from({length:EFFECT_SLOTS},(_,i)=>i<8||i===MASTER_EFFECT_SLOT?defaultEffect(14,effectUsesMaster(i)):defaultEffect(0,effectUsesMaster(i))),'RDL imports every stored insert/master property, with added timbre FX bypassed');
  assert.deepEqual(effectsFromRdl(imported.rdl.program),imported.engine.effects,'Legacy RDL migration agrees with the native stored-program reader');
  const oldRdl=structuredClone(rdlPatches({programs:[imported]},'Legacy.rdl','b'.repeat(64))[0].snapshot);delete oldRdl.engine.effects;
  assert.deepEqual(normalizeProgram(oldRdl,parameters).engine.effects,imported.engine.effects,'RDL patches already saved in a browser gain their stored effects without reimport');
  assert.deepEqual(validateEffects(JSON.parse(JSON.stringify(edited.effects))),edited.effects);
  api.rustias_stop();return 'Shared FX: all 30 algorithms; browser FX 3/4 use Master variants per timbre before the shared sum, serial order, 8-timbre/PCM isolation, 33-slot persistence and 9/17-slot migration';
}
