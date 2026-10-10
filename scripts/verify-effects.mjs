import {MAX_TIMBRES,EFFECT_SLOTS,MASTER_EFFECT_SLOT} from '../web/limits.js';
import assert from 'node:assert/strict';
import {setEffectCatalog,defaultEffect,emptyEffects,validateEffects,effectsFromRdl} from '../web/effects.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {syntheticRdl} from './verify-rdl.mjs';
import {parseRdl,rdlPatches} from '../web/rdl.js';
export function readEffectCatalog(api){const n=api.rustias_effect_catalog(),catalog=JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_effect_catalog_buffer(),n)));setEffectCatalog(catalog);return catalog;}
export function verifyEffects({api,catalog,render,rms,control,save,load,parameters,module}){
  function effect(slot,p,accepted=1){const b=Buffer.from(JSON.stringify(p));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),b.length).set(b);assert.equal(api.rustias_effect(slot,b.length),accepted);}
  function voice(slot,kind,timbre=0){api.rustias_init();control(3,0,timbre);control(4,127,timbre);control(5,127,timbre);control(6,8,timbre);if(kind){const fx=defaultEffect(kind,slot===MASTER_EFFECT_SLOT);if(kind===6)fx.parameters[6]=76;effect(slot,fx);}api.rustias_note(timbre,69,100);return render(30000);}
  const dry=voice(0,0);for(const master of [false,true]){
    const defs=catalog[master?'master':'insert'];assert.equal(defs.length,31);
    for(let kind=1;kind<=30;kind++){
      const slot=master?MASTER_EFFECT_SLOT:0,audio=voice(slot,kind);assert.ok(rms(audio)>.000001,`${defs[kind].name} produces audio`);assert.ok(rms(audio.map((v,i)=>v-dry[i]))>.000001,`${defs[kind].name} processes the original signal`);
      const p=defaultEffect(kind,master);p.enabled=false;effect(slot,p);const bypass=render(5000);assert.ok(rms(bypass)>.001,'Effect bypass restores the dry sound');
      for(const [i,prop]of defs[kind].properties.entries())for(const value of [prop.min,prop.max]){const boundary=defaultEffect(kind,master);boundary.parameters[i]=value+prop.zero;effect(slot,boundary);render(128);}
    }
  }
  const unaffected=voice(2,7,0);assert.deepEqual(unaffected,dry,'A timbre insert never processes another timbre');
  const sine=Float32Array.from({length:4800},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));
  function pcm(kind,library=false,fxTimbre=library?2:0){api.rustias_init();const values=parameters.map(p=>p.default);for(const [id,v]of [[3,0],[4,127],[5,127],[6,8],[9,127]])values[id]=v;
    if(library){const pointer=api.rustias_library_sample_buffer(99,sine.length);new Float32Array(api.memory.buffer,pointer,sine.length).set(sine);assert.equal(api.rustias_library_sample_commit(99,sine.length),1);assert.equal(api.rustias_library_profile(99,99,2,2),1);values.forEach((v,id)=>api.rustias_library_control(99,id,v));api.rustias_library_sync();}
    else{control(140,1);for(const id of [3,4,5,6,9])control(id,values[id]);const pointer=api.rustias_sample_buffer(0,sine.length);new Float32Array(api.memory.buffer,pointer,sine.length).set(sine);assert.equal(api.rustias_sample_commit(0,sine.length,2),1);}
    if(kind)effect(2*fxTimbre,defaultEffect(kind));library?api.rustias_library_note(2,99,100):api.rustias_drum_pad(0,100);return render(12000);
  }
  const pcmDry=pcm(0),pcmFx=pcm(7);assert.ok(rms(pcmFx.map((v,i)=>v-pcmDry[i]))>.00001,'Drum Kit PCM passes through its owning timbre inserts');
  const libraryDry=pcm(0,true),libraryFx=pcm(7,true);assert.ok(rms(libraryFx.map((v,i)=>v-libraryDry[i]))>.00001,'Direct sequencer samples pass through their own timbre inserts');assert.deepEqual(pcm(7,true,0),libraryDry,'A different timbre insert leaves the sequencer sample intact');
  api.rustias_init();const delay=defaultEffect(14);delay.parameters[0]=100;delay.parameters[2]=0;delay.parameters[4]=10;delay.parameters[5]=10;effect(0,delay);api.rustias_note(0,69,100);render(24000);api.rustias_stop();assert.equal(api.rustias_voices(),0);assert.ok(rms(render(6000))>.00001,'An effect tail survives the end of all voices');
  delay.parameters[0]=70;effect(0,delay);assert.ok(rms(render(128))>.00001,'A live parameter edit preserves the delay state');effect(0,defaultEffect());effect(0,delay);assert.ok(rms(render(6000))<.000001,'Changing the effect type clears obsolete delay state');
  api.rustias_init();const originals=save();const invalid=defaultEffect(14);invalid.parameters[0]=255;effect(0,invalid,0);assert.deepEqual(save(),originals,'Invalid live effect edits are atomic');const wrongBank=defaultEffect(14,true);effect(0,wrongBank,0);assert.deepEqual(save(),originals);
  const valid=defaultEffect(14);effect(0,valid);const edited=save();assert.deepEqual(edited.effects.slots[0],valid);assert.equal(load(edited),1);assert.deepEqual(save(),edited,'All seventeen effect slots survive the Rust program round trip');
  const bad=structuredClone(edited);bad.effects.slots[MASTER_EFFECT_SLOT].master=false;assert.equal(load(bad),0);assert.deepEqual(save(),edited,'Invalid effect bank leaves the current instrument unchanged');
  const legacy=structuredClone(originals);delete legacy.effects;assert.equal(load(legacy),1);assert.deepEqual(save().effects,emptyEffects(),'Legacy native patches load with bypassed effects');
  const program=normalizeProgram(edited,parameters);program.engine.effects.slots[4]=defaultEffect(11);program.engine.effects.slots[5]=defaultEffect(20);program.engine.effects.slots[MASTER_EFFECT_SLOT]=defaultEffect(12,true);
  const sound=captureTimbre(program,2,parameters),applied=applyTimbre(program,1,sound,parameters);
  assert.deepEqual(applied.engine.effects.slots.slice(2,4),sound.effects,'Both insert effects follow a saved timbre');assert.deepEqual(applied.engine.effects.slots[MASTER_EFFECT_SLOT],program.engine.effects.slots[MASTER_EFFECT_SLOT],'A timbre does not replace the master effect');assert.deepEqual(applied.sequencer,program.sequencer);
  const fixture=syntheticRdl(2);for(let slot=0;slot<9;slot++){const master=slot===8,offset=master?1038:168+228*Math.floor(slot/2)+24*(slot%2),fx=defaultEffect(14,master);fixture.programs[0][offset]=128|fx.kind;Buffer.from(fx.parameters).copy(fixture.programs[0],offset+(master?2:4));}
  const chunk=Buffer.alloc(32+fixture.programs[0].length);chunk.write('316p');chunk.writeUInt32LE(32,4);chunk.writeUInt32LE(fixture.programs[0].length,8);fixture.programs[0].copy(chunk,32);
  const imported=parseRdl(new WebAssembly.Instance(module,{}).exports,chunk).programs[0];assert.deepEqual(imported.engine.effects.slots,Array.from({length:EFFECT_SLOTS},(_,i)=>i<8||i===MASTER_EFFECT_SLOT?defaultEffect(14,i===MASTER_EFFECT_SLOT):defaultEffect()),'RDL imports every stored insert/master property');
  assert.deepEqual(effectsFromRdl(imported.rdl.program),imported.engine.effects,'Legacy RDL migration agrees with the native stored-program reader');
  const oldRdl=structuredClone(rdlPatches({programs:[imported]},'Legacy.rdl','b'.repeat(64))[0].snapshot);delete oldRdl.engine.effects;
  assert.deepEqual(normalizeProgram(oldRdl,parameters).engine.effects,imported.engine.effects,'RDL patches already saved in a browser gain their stored effects without reimport');
  assert.deepEqual(validateEffects(JSON.parse(JSON.stringify(edited.effects))),edited.effects);
  api.rustias_stop();return 'Shared native FX: all 30 insert/master algorithms and parameter boundaries, timbre isolation, bypass, atomic edits, program/timbre persistence, RDL import and stored-record migration';
}
