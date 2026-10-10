import assert from 'node:assert/strict';
import {emptySequence} from '../web/sequence.js';
import {emptySamples,sampleValues} from '../web/sample-state.js';
import {emptyCircuits} from '../web/circuit.js';
import {PatchStore,TimbreStore} from '../web/patches.js';
import {normalizeProgram,captureTimbre,applyTimbre,SLOT_PARAMETERS} from '../web/programs.js';
import {parseRdl,rdlPatches,rdlMasks} from '../web/rdl.js';
import {syntheticRdl} from './verify-rdl.mjs';

export function verifyPrograms(parameters,module){
  const defaults=parameters.map(p=>p.default);
  const engine={version:1,timbres:Array.from({length:4},(_,t)=>defaults.map((v,id)=>id===72?t:id===1?25+t*20:v)),drums:Array.from({length:16},(_,i)=>defaults.map((v,id)=>id===146?36+i:v))};
  const sequencer=emptySequence(),samples=emptySamples(),circuits=emptyCircuits(engine.timbres);
  for(let t=0;t<4;t++){
    sequencer.tracks[t].length=128-t*16;sequencer.tracks[t].resolution=['1/16','1/3','1/4','3/8'][t];
    sequencer.tracks[t].steps[127]={notes:[48+t,60+t,72+t],samples:[`808:kick-0${t+1}`],velocity:80+t,gate:50+t};
    samples.library.push({timbre:t,source:`808:kick-0${t+1}`,mode:2,name:`Sound ${t}`,values:sampleValues(engine.timbres[t])});
    circuits.tracks[t].nodes[0].x+=t*100;circuits.tracks[t].enabled=t===2;
  }
  circuits.tracks[2].nodes.push({id:16,kind:'oscillator1',x:3000,y:300,params:{wave:2,mode:3,ctrl1:90,ctrl2:72,semitone:12,fine:17,level:97}});
  circuits.tracks[2].wires=circuits.tracks[2].wires.filter(w=>w.to!==15);
  circuits.tracks[2].wires.push({from:16,to:15,port:'in'});
  const names=['Pad','Bass','Lead','Percussion'];
  const program=normalizeProgram({version:2,engine,sequencer,samples,circuits,volume:61,timbreInfo:names.map(name=>({name,preset:'custom',savedId:null,modified:false}))},parameters);
  const values=new Map(),disk={getItem:key=>values.get(key)??null,setItem:(key,value)=>values.set(key,value)};
  const programs=new PatchStore(disk),sounds=new TimbreStore(disk);
  const saved=programs.save('Four timbres',program),sound=captureTimbre(program,2,parameters);
  const savedSound=sounds.save('Lead sound',sound);
  assert.equal(programs.list().length,1);assert.equal(sounds.list().length,1,'Programs and individual sounds have separate libraries');
  const restored=normalizeProgram(new PatchStore(disk).list()[0].snapshot,parameters);
  assert.deepEqual(restored,program,'All timbres, 128-step tracks, resolutions, sample profiles, circuits, names and volume survive storage');
  const snapshotBefore=structuredClone(program);
  const applied=applyTimbre(program,0,sound,parameters);
  assert.deepEqual(program,snapshotBefore,'Loading a sound does not mutate a stored program');
  assert.deepEqual(applied.sequencer,program.sequencer,'Loading a sound preserves all four patterns');
  assert.deepEqual(applied.engine.timbres.slice(1),program.engine.timbres.slice(1),'Other three sounds remain intact');
  assert.deepEqual(applied.circuits.tracks[0],program.circuits.tracks[2],'Sound includes its actual modular routing and layout');
  assert.deepEqual(applied.circuits.tracks.slice(1),program.circuits.tracks.slice(1));
  for(const p of parameters)assert.equal(applied.engine.timbres[0][p.id],p.scope==='global'||p.readonly||SLOT_PARAMETERS.has(p.id)?program.engine.timbres[0][p.id]:program.engine.timbres[2][p.id],p.label);
  assert.equal(applied.volume,61);assert.equal(applied.samples.library.length,5,'Existing pattern sources remain available alongside the loaded sound');
  assert.deepEqual(applied.samples.library.find(p=>p.timbre===0&&p.source==='808:kick-03').values,sound.library[0].values,'Sample sound profiles follow the new timbre slot');
  const edited=structuredClone(applied);edited.engine.timbres[0][1]=105;
  programs.save(saved.name,edited,saved.id);
  assert.equal(programs.list().length,1,'Save current keeps the program ID');
  const copy=programs.save('Four timbres copy',edited);
  edited.engine.timbres[0][1]=10;programs.save(saved.name,edited,saved.id);
  assert.equal(new PatchStore(disk).list().find(p=>p.id===copy.id).snapshot.engine.timbres[0][1],105,'Saving the original leaves its copy independent');
  const updatedSound=structuredClone(sound);updatedSound.values[1]=11;
  sounds.save(savedSound.name,updatedSound,savedSound.id);
  assert.equal(sounds.list().length,1,'Save current keeps the timbre ID');
  const soundCopy=sounds.save('Lead copy',updatedSound);updatedSound.values[1]=100;sounds.save(savedSound.name,updatedSound,savedSound.id);
  assert.equal(new TimbreStore(disk).list().find(p=>p.id===soundCopy.id).snapshot.values[1],11,'Timbre copies are independent');
  assert.equal(normalizeProgram(new PatchStore(disk).list().find(p=>p.id===copy.id).snapshot,parameters).engine.timbres[2][1],65,'Updating the sound library does not rewrite sounds embedded in programs');
  const withPattern=captureTimbre(program,2,parameters,{sequence:true}),withPatternApplied=applyTimbre(program,0,withPattern,parameters);
  assert.deepEqual(withPatternApplied.sequencer.tracks[0],program.sequencer.tracks[2]);assert.deepEqual(withPatternApplied.sequencer.tracks.slice(1),program.sequencer.tracks.slice(1));
  const kitProgram=structuredClone(program);for(const v of kitProgram.engine.timbres){v[140]=1;v[141]=2;v[143]=90;v[144]=30;v[145]=3;}
  kitProgram.engine.drums[5][1]=31;kitProgram.engine.drums[5][114]=-12;kitProgram.samples.slots[5]={source:'909:bd0a0d1',mode:1,name:'909 kick'};kitProgram.samples.kitGain=20;
  const kit=applyTimbre(program,1,captureTimbre(kitProgram,2,parameters),parameters);
  assert.deepEqual(kit.engine.drums,kitProgram.engine.drums);assert.deepEqual(kit.samples.slots,kitProgram.samples.slots);assert.equal(kit.samples.kitGain,20);
  assert.ok(kit.engine.timbres.every(v=>v[140]===1&&v[141]===1&&v[143]===90));assert.deepEqual(kit.sequencer,program.sequencer,'Moving a drum timbre restores its kit without replacing patterns');
  const old=normalizeProgram(engine,parameters);assert.deepEqual(old.sequencer,emptySequence());assert.deepEqual(old.samples,emptySamples());assert.deepEqual(old.circuits,emptyCircuits(engine.timbres));
  const legacy=structuredClone(engine);for(const v of [...legacy.timbres,...legacy.drums])v.length=153;assert.equal(normalizeProgram(legacy,parameters).engine.timbres[0].length,parameters.length,'Old native programs still migrate');
  const invalid=structuredClone(sound);invalid.values[1]=999;assert.throws(()=>applyTimbre(program,0,invalid,parameters));assert.deepEqual(program,snapshotBefore,'Invalid imports leave the current program untouched');
  const programBytes=disk.getItem('rustias.patches.v2'),soundBytes=disk.getItem('rustias.timbres.v1'),failing=new TimbreStore({...disk,setItem(){throw new Error('Quota exceeded');}});
  assert.throws(()=>failing.save('Too large',sound));assert.equal(disk.getItem('rustias.timbres.v1'),soundBytes);assert.equal(disk.getItem('rustias.patches.v2'),programBytes,'A timbre quota failure cannot destroy programs or previous sounds');
  // Original librarian records and unavailable ROM sources follow the sound slot.
  const api=new WebAssembly.Instance(module,{}).exports,fixture=syntheticRdl(2);
  const imported=rdlPatches(parseRdl(api,fixture.bytes),'Synthetic.rdl','a'.repeat(64))[1].snapshot;
  const missing=applyTimbre(program,3,captureTimbre(imported,0,parameters),parameters);
  assert.equal(missing.rdl.program,imported.rdl.program);assert.equal(rdlMasks(missing.rdl).timbres,8,'An unavailable PCM source stays muted after moving to another slot');
  api.rustias_init();const bytes=Buffer.from(JSON.stringify(restored.engine));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),bytes.length).set(bytes);assert.equal(api.rustias_load(bytes.length),1,'Saved programs remain valid for the shared Rust engine');
  return 'Program/timbre libraries: full four-track round-trip, slot isolation, drum/sample/circuit persistence, current/copy IDs, legacy migration and RDL source masks';
}
