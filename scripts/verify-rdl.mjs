import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {parseRdl,rdlPatches,validateRdlSource,rdlMasks} from '../web/rdl.js';
import {PatchStore} from '../web/patches.js';
import {validateSequence,SequenceClock,emptySequence} from '../web/sequence.js';
import {validateModulation,ModulationClock,modulationKey} from '../web/modulation.js';
import {normalizeProgram} from '../web/programs.js';
import {webParameters} from '../web/parameters.js';
import {compressStorage,decompressStorage} from '../web/storage-codec.js';
import vm from 'node:vm';
import {ModulationAutomation} from '../web/modulation.js';
import {ModulationHost} from '../web/modulation-host.js';
import {emptyMacros} from '../web/macros.js';
import {RecordingTap} from '../web/recording-tap.js';
import {MAX_TIMBRES,INITIAL_TIMBRES,timbreArray} from '../web/limits.js';
import {validSampleSource} from '../web/sample-state.js';

// Synthetic librarian data only. No hardware/user programs enter the repo.
function chunk(kind,payload,header=32){
  const result=Buffer.alloc(header+payload.length);result.write(kind);result.writeUInt32LE(header,4);result.writeUInt32LE(payload.length,8);payload.copy(result,header);return result;
}
function program(seed=0){
  const bytes=Buffer.alloc(2304);bytes.write(`Patch ${String(seed).padStart(3,'0')}`);
  bytes[25]=127;bytes[26]=64;bytes[27]=64;bytes.writeUInt16LE(1200+seed,1060);bytes.fill((seed+17)%256,1790);
  for(let t=0;t<4;t++){
    const common=bytes.subarray(48+t*228,48+(t+1)*228),p=common.subarray(16);
    common[0]=128;common[4]=t===0?16:t;common[5]=0xfc;common[7]=127;common[8]=seed%7;common[9]=32;common[10]=96;common[11]=66;common[12]=45;common[13]=3;
    p[0x10]=128|64|2;p[0x13]=64;p[0x14]=64;p[0x15]=70;
    p[0x16]=(seed+t)%4;p[0x17]=32;p[0x18]=96;p[0x1b]=(t&3)|((t&3)<<4);p[0x1c]=76;p[0x1d]=63;
    p.set([100,80,20],0x1e);p[0x21]=0xa3;p[0x22]=42;p[0x23]=110;p[0x24]=25;p[0x25]=90;p[0x26]=64;
    p[0x28]=75;p[0x29]=33;p[0x2a]=70;p[0x2b]=82;p[0x2d]=101;p[0x2e]=18;p[0x2f]=seed%11;p[0x30]=70;p[0x31]=64+seed%31;p[0x32]=65;
    for(let i=0;i<3;i++)p.set([2+i,43,97,33,1+i,90,66,68],0x34+i*8);
    p.set([3,65,62,0xc5,15,2,66,72,0xa7,16],0x4c);
    for(let i=0;i<6;i++)p.set([i,[16,39,36,9,13,20][i],77+i],0x56+i*3);
  }
  return bytes;
}
function drumKit(seed){
  const bytes=Buffer.alloc(2144);bytes.write(`Test kit ${seed}`);bytes.fill(0x5a,1792);
  for(let i=0;i<16;i++){
    bytes[18+i]=i%4;bytes[36+i]=36+i;
    const body=bytes.subarray(52+i*104,52+(i+1)*104);program(seed+i).subarray(64,168).copy(body);
    body[0x16]=4;body[0x17]=72;body[0x18]=30;body[0x2d]=100-i;
  }
  return bytes;
}
export function syntheticRdl(count=256){
  const programs=Array.from({length:count},(_,i)=>program(i));
  if(count>1){programs[1][48+16+0x16]=7;programs[1][48+228+16+0x16]=6;programs[1][48+16+0x56]=9;}
  if(count>7){programs[7][24]=(2<<5)|5;programs[7][25]=95;programs[7][26]=80;programs[7][27]=67;}
  const global=Buffer.alloc(736);global[6]=5;global[15]=1;
  const kits=Array.from({length:32},(_,i)=>drumKit(i));
  return {programs,kits,global,bytes:chunk('316B',Buffer.concat([
    chunk('316P',Buffer.concat(programs.map(p=>chunk('316p',p)))),
    chunk('316D',Buffer.concat(kits.map(p=>chunk('316d',p)))),chunk('316G',chunk('316g',global)),
  ]),64)};
}
export function sequencedRdl(linked=false){
  const raw=program(0);raw.write(linked?'RDL Linked':'RDL Sequence');raw[1062]=linked?0xd0:0xc0;raw[1063]=63;
  for(let t=0;t<4;t++){
    raw[48+t*228]=(raw[48+t*228]&~12)|([2,3,2,0][t]<<2);
    const m=raw.subarray(48+t*228+172,48+(t+1)*228);m.fill(64);m[0]=0xaf;m[1]=0x96;
    for(let i=0;i<3;i++){m[2+i*18]=[10,19,41][i];m[3+i*18]=i===1?0:128;for(let j=0;j<16;j++)m[4+i*18+j]=i===0?1+j*8:i===1?64-j:127-j;}
  }
  // A: all 32 eight-note chords, muted step retaining data, TIE, KEY velocity.
  // B: different rate / OneShot / BaseNote / scan zone / signed gate and swing.
  for(let seq=0;seq<2;seq++){
    const p=raw.subarray(1134+seq*328,1462+seq*328);p.fill(128,72);p[0]=seq?0x40:0x21;p[1]=seq?6:0x9f;p[2]=seq?20:246;p[3]=seq?206:25;p[4]=128|(seq?48:60);p[6]=36;p[7]=96;
    for(let s=0;s<32;s++){p[8+s]=128+(s===0?100:75);p[40+s]=s===1?128:90+s;for(let n=0;n<8;n++)p[72+s*8+n]=Math.min(127,(seq?36:60)+s+n);}
    p[8+2]&=127;
  }
  // Voice-sync OneShot pitch lane and an inactive mod bank still survive import.
  raw[48+228+172]=0x91;raw[48+228+173]=0x26;raw[48+228+174]=1;raw[48+228+176]=40;raw[48+228+177]=88;raw[48+3*228+172]&=127;
  return {raw,bytes:chunk('316p',raw)};
}
function verifySequences(api){
  for(const text of ['', 'RDL · Программа 🎹', 'patch'.repeat(100000),JSON.stringify(Array.from({length:70000},(_,i)=>String.fromCharCode((i*997)%65536)))])assert.equal(decompressStorage(compressStorage(text)),text,'Compression survives Unicode and dictionary resets');assert.throws(()=>decompressStorage('AQ=='));assert.throws(()=>decompressStorage('AQD//w=='));
  const fixture=sequencedRdl(),digest=createHash('sha256').update(fixture.bytes).digest('hex'),patch=rdlPatches(parseRdl(api,fixture.bytes),'Sequences.rdl',digest)[0],sequence=validateSequence(patch.snapshot.sequencer),modulation=validateModulation(patch.snapshot.modulation);
  assert.equal(sequence.patterns.length,2,'Both source note sequencers are retained, including unassigned ones');
  const a=sequence.tracks[0],b=sequence.tracks[1];assert.equal(a.length,32);assert.equal(a.resolution,'1/16');assert.equal(b.length,7);assert.equal(b.resolution,'1/8');assert.equal(b.runMode,'OneShot');assert.equal(a.gateOffset,-10);assert.equal(b.swing,-50);assert.equal(a.transpose,true);assert.equal(a.latch,true);assert.deepEqual([a.scanBottom,a.scanTop,a.baseNote],[36,96,60]);
  assert.deepEqual(a.steps[0].notes,[60,61,62,63,64,65,66,67]);assert.equal(a.steps[0].tie,true);assert.equal(a.steps[1].velocityMode,'Key');assert.equal(a.steps[2].trigger,false);assert.equal(a.steps[2].notes.length,8,'Muted steps retain their complete recorded chord');assert.equal(a.steps[31].notes[0],91);assert.deepEqual(sequence.tracks[2],a,'A pattern assigned to multiple timbres is copied in full');assert.equal(sequence.tracks[3].enabled,false);
  assert.deepEqual(modulation.tracks.map(t=>t.length),[3,3,3,3,0,0,0,0]);
  const m=modulation.tracks[0];assert.deepEqual(m.map(l=>l.target.parameter),[1,7,112],'SYS 2.00 destinations: Cutoff, Amp and Patch 6');assert.equal(m[0].direction,'Alt1');assert.equal(m[0].resolution,'1/8');assert.equal(m[0].motion,'Step');assert.equal(m[1].motion,'Slide');assert.equal(m[0].keySync,'Timbre');assert.deepEqual(m[0].values.slice(0,16),Array.from({length:16},(_,i)=>-63+i*8));assert.equal(modulation.tracks[1][0].target.parameter,53);assert.deepEqual(modulation.tracks[1][0].values.slice(0,2),[-24,24]);assert.equal(modulation.tracks[1][0].runMode,'OneShot');assert.equal(modulation.tracks[3][0].enabled,false);
  assert.ok(patch.snapshot.rdl.notices.some(n=>n.includes('independent per-voice phases')));
  const linked=sequencedRdl(true),joined=rdlPatches(parseRdl(api,linked.bytes),'Linked.rdl',digest)[0].snapshot;assert.equal(joined.sequencer.patterns.length,3);assert.equal(joined.sequencer.tracks[0].length,64);assert.equal(joined.sequencer.tracks[1].resolution,'1/16','Sequence 1 common settings take priority when linked');assert.equal(joined.sequencer.tracks[0].steps[32].notes[0],36);assert.equal(joined.sequencer.tracks[0].steps[32].baseNote,48);assert.equal(joined.sequencer.tracks[0].steps[63].notes[0],67,'The last linked chord is not truncated');
  const params=webParameters(JSON.parse(fs.readFileSync(new URL('../crates/radias-synth-infrastructure/src/parameters.json',import.meta.url)))),normalized=normalizeProgram(patch.snapshot,params);assert.deepEqual(normalized.modulation,modulation);assert.deepEqual(normalized.sequencer,sequence);
  const legacy=structuredClone(patch.snapshot);legacy.sequencer=emptySequence();delete legacy.modulation;delete legacy.rdl.sequenceImportVersion;assert.deepEqual(normalizeProgram(legacy,params).sequencer,sequence,'Old imports with empty patterns upgrade on load');legacy.sequencer.tracks[0].steps[0].notes=[69];legacy.sequencer.tracks[0].length=128;const preserved=normalizeProgram(legacy,params);assert.deepEqual(preserved.sequencer.tracks[0].steps[0].notes,[69]);assert.equal(preserved.sequencer.tracks[0].length,128);assert.equal(preserved.sequencer.patterns.length,2,'Migration preserves edits while making original patterns available');
  const disk=storage(),store=new PatchStore(disk);store.save('RDL sequences',normalized);assert.deepEqual(new PatchStore(disk).list()[0].snapshot,normalized,'Every converted sequence and setting survives compact persistence');

  const notes=emptySequence();notes.tracks.forEach((t,i)=>{t.enabled=i===0;});Object.assign(notes.tracks[0],{length:3,runMode:'OneShot',swing:50});notes.tracks[0].steps[0]={notes:[60],samples:[],velocity:90,gate:100,tie:true};notes.tracks[0].steps[1]={notes:[60,64],samples:[],velocity:100,gate:100,tie:true};notes.tracks[0].steps[2]={notes:[67],samples:[],velocity:100,gate:50,trigger:false};
  const events=[],clock=new SequenceClock((t,n,v)=>events.push([clock.frame,t,n,v]));clock.setConfig(notes);clock.play();for(let i=0;i<150;i++)clock.beforeRender();assert.deepEqual(events.filter(e=>e[3]).map(e=>[e[2],e[3]]),[[60,90],[64,100]],'Tied common notes are not retriggered and disabled chords never sound');const swung=events.find(e=>e[2]===64&&e[3])?.[0];assert.ok(swung>=9000&&swung<9128,'Swing +50 delays the even onset by half of one step');assert.equal(events.filter(e=>e[3]===0).length,2);clock.stop();
  Object.assign(notes.tracks[0],{runMode:'Step',keyTriggered:true,transpose:true,baseNote:60,scanBottom:36,scanTop:96});notes.tracks[0].steps[0].tie=false;notes.tracks[0].steps[0].velocityMode='Key';clock.setConfig(notes);assert.equal(clock.trigger(0,100,100),false);assert.equal(clock.trigger(0,62,71),true);assert.deepEqual(events.at(-1).slice(2),[62,71],'Keyboard Step mode uses transpose and KEY velocity');clock.trigger(0,62,0);assert.equal(events.at(-1)[3],0);clock.stop();
  const mc=new ModulationClock();mc.setConfig(modulation);mc.play();mc.beforeRender(12000);mc.beforeRender(0);assert.equal(mc.positions[0][0],1);mc.note(0,60,100);mc.beforeRender(0);assert.equal(mc.positions[0][0],0,'Timbre KeySync resets after all keys were released');mc.beforeRender(12000);mc.note(0,64,100);mc.beforeRender(0);assert.equal(mc.positions[0][0],1,'A second held note does not reset Timbre sync');mc.note(0,60,0);mc.note(0,64,0);mc.note(0,67,100);mc.beforeRender(0);assert.equal(mc.positions[0][0],0);
  mc.note(1,60,100);mc.beforeRender(12000);mc.note(1,64,100);mc.beforeRender(0);assert.equal(mc.positions[1][0],1,'Imported Voice sync resets the reverse lane to its last step');assert.equal(mc.beforeRender(0).get(modulationKey(modulation.tracks[1][0].target)).value,24);
  return 'RDL sequences: both 32-step polyphonic patterns, 64-step link, rests/TIE/KEY, rates/swing/transposition, 12 mod lanes, SYS 2.00 targets, Step/Slide, sync, migration and storage';
}
function storage(limit=Infinity){
  const values=new Map();return {values,getItem:key=>values.get(key)??null,setItem(key,value){
    if(value.length*2>limit){const error=new Error('Quota exceeded');error.name='QuotaExceededError';throw error;}values.set(key,value);
  }};
}
export function verifyRdlPlayback({module,parameters,sequenceSource,workletSource}){
  for(const sampleRate of [48000,44100]){
    let Processor;const messages=[];
    vm.runInNewContext(sequenceSource+'\n'+workletSource.replace(/^import .*;\n/gm,''),{sampleRate,WebAssembly,Float32Array,Uint8Array,validSampleSource,RecordingTap,MAX_TIMBRES,INITIAL_TIMBRES,timbreArray,ModulationAutomation,ModulationHost,AudioWorkletProcessor:class{constructor(){this.port={postMessage:data=>messages.push(data)};}},registerProcessor:(_name,p)=>Processor=p});
    const p=new Processor({processorOptions:{module,gain:.3}}),fixture=sequencedRdl(),patch=rdlPatches(parseRdl(p.wasm,fixture.bytes),'Playback.rdl','a'.repeat(64))[0],program=normalizeProgram(patch.snapshot,parameters);
    for(const v of program.engine.timbres)for(const [id,value]of [[0,0],[10,0],[1,100],[2,0],[9,0],[3,0],[4,127],[5,127],[6,8],[7,100],[17,127],[18,0],[19,0]])v[id]=value;
    const state={config:program.modulation,macros:emptyMacros(),targets:program.modulation.tracks.flatMap(lanes=>lanes.filter(l=>l.target).map(l=>{const spec=parameters[l.target.parameter];return {target:l.target,min:spec.min,max:spec.max,base:program.engine.timbres[l.target.timbre][spec.id],available:true};})),engine:program.engine,effects:program.engine.effects,circuits:program.circuits.tracks};
    const send=data=>p.port.onmessage({data});send({type:'load',program:program.engine});send({type:'sequencer',config:program.sequencer});send({type:'modulation',state});send({type:'sequence-play'});
    let peak=0;const block=()=>{const left=new Float32Array(128),right=new Float32Array(128);p.process([],[[left,right]]);assert.equal(p.failed,false);for(const value of left){assert.ok(Number.isFinite(value));peak=Math.max(peak,Math.abs(value));}};
    for(let i=0;i<4;i++)block();assert.ok(p.wasm.rustias_voices()>0,'Imported polyphonic notes reach real WASM voices');assert.equal(p.wasm.rustias_value(0,1),37,'Imported Cutoff offsets reach the Rust filter');while(p.wasm.rustias_frames()<14000)block();assert.ok(peak>.0001,'Imported patterns produce audible output at the device rate');assert.equal(p.sequencer.positions[0],2);send({type:'sequence-stop'});assert.equal(p.wasm.rustias_value(0,1),100,'Stopping an imported sequence restores the base filter');send({type:'stop'});
    const step=structuredClone(program.sequencer);step.tracks.forEach((track,t)=>track.enabled=t===0);step.tracks[0].runMode='Step';send({type:'sequencer',config:step});send({type:'midi',bytes:[0x90,62,71]});for(let i=0;i<2;i++)block();assert.equal(p.sequencer.positions[0],0);assert.ok([...p.sequenceNotes.keys()].includes('0:62'),'Global-channel MIDI drives the imported Step pattern with transpose');assert.ok(![...p.manualNotes.keys()].some(key=>key.startsWith('2:')),'Disabled sequencers do not acquire manual notes from another channel');send({type:'midi',bytes:[0x80,62,0]});assert.equal(p.sequenceNotes.size,0,'MIDI release stops a keyboard-gated Step chord');send({type:'midi',bytes:[0x90,64,90]});assert.equal(p.sequencer.positions[0],1,'The next keyboard note advances one Step');send({type:'stop'});
    assert.ok(!messages.some(m=>m.type==='error'));
  }
  return 'Imported RDL playback in real WASM at 48/44.1 kHz: chords, native Cutoff modulation, base restoration and transposed MIDI Step mode';
}
function load(api,engine){const bytes=Buffer.from(JSON.stringify(engine));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),bytes.length).set(bytes);return api.rustias_load(bytes.length);}
export function verifyRdl(module){
  const api=new WebAssembly.Instance(module,{}).exports,fixture=syntheticRdl(),library=parseRdl(api,fixture.bytes);
  const sequenceFeature=verifySequences(api);
  assert.equal(library.programs.length,256);assert.equal(library.drum_kits,32);assert.equal(library.programs[0].name,'Patch 000');
  const p=library.programs[0],v=p.engine.timbres[0];
  const expected={0:0,10:0,11:32,12:96,13:0,14:0,15:76,16:63,17:100,18:80,19:20,7:101,8:64,9:42,1:110,2:25,25:90,26:64,20:3,21:2,24:1,22:75,23:33,27:70,28:82,29:2,30:1,31:70,154:0,52:65,53:64,54:64,55:70,56:66,57:1,58:1,59:45,60:3,61:1,62:1,63:1,64:2,65:1,67:0,68:2,69:32,70:96,71:1,72:16,119:0,120:127,151:1,153:1,73:3,74:65,75:62,76:2,77:5,78:1,79:15,81:2,82:66,83:72,84:1,85:7,86:1,87:16,89:1200,148:5,149:1,90:0,91:16,92:77};
  for(const [id,value] of Object.entries(expected))assert.equal(v[id],value,`RDL control ${id}`);
  assert.deepEqual(v.slice(32,36),[2,43,97,33]);assert.deepEqual(v.slice(3,7),[3,43,97,33]);assert.deepEqual(v.slice(40,44),[1,90,66,68]);
  assert.deepEqual(v.slice(48,52),[3,90,66,68]);assert.deepEqual(p.engine.timbres.map(t=>t[13]),[0,1,2,3]);assert.deepEqual(p.engine.timbres.map(t=>t[14]),[0,1,2,3]);
  assert.deepEqual(Buffer.from(p.rdl.program,'base64'),fixture.programs[0],'The entire original record survives, including librarian metadata');
  assert.deepEqual(Buffer.from(p.rdl.global,'base64'),fixture.global);assert.deepEqual(p.rdl.unavailable,[]);
  const missing=library.programs[1];assert.equal(missing.rdl.unavailable.length,2);assert.deepEqual(missing.rdl.unavailable.map(s=>s.label),['PCM','Audio In']);assert.equal(missing.engine.timbres[0][92],64,'Unsupported active modulation is disabled');
  assert.ok(missing.rdl.notices.some(n=>n.includes('route is disabled')));
  const drum=library.programs[7];assert.equal(drum.engine.timbres[0][140],1);assert.equal(drum.engine.timbres[0][141],1);assert.equal(drum.engine.timbres[0][143],95);assert.equal(drum.engine.timbres[0][144],80);assert.equal(drum.engine.timbres[0][145],67);
  assert.deepEqual(Buffer.from(drum.rdl.drum_kit,'base64'),fixture.kits[5]);assert.equal(drum.rdl.drum_kit_name,'Test kit 5');
  drum.engine.drums.forEach((d,i)=>{assert.equal(d[0],4);assert.equal(d[7],100-i);assert.equal(d[146],36+i);assert.equal(d[147],i%4);});
  api.rustias_init();for(const p of library.programs)assert.equal(load(api,p.engine),1,`Converted program ${p.slot} is accepted by the real engine`);
  const digest=createHash('sha256').update(fixture.bytes).digest('hex'),patches=rdlPatches(library,'Synthetic.rdl',digest);
  patches.forEach(p=>validateRdlSource(p.snapshot.rdl));assert.deepEqual(rdlMasks(patches[1].snapshot.rdl),{timbres:3,drums:0});
  const badMetadata=structuredClone(patches[1].snapshot.rdl);badMetadata.unavailable[0].drum=16;assert.throws(()=>validateRdlSource(badMetadata));
  assert.equal(load(api,missing.engine),1);api.rustias_rdl_mute(3,0);api.rustias_note(0,60,100);api.rustias_note(1,60,100);api.rustias_midi(0x95,60,100);assert.equal(api.rustias_voices(),0,'Missing sources cannot trigger replacement audio via direct notes or MIDI');
  api.rustias_rdl_mute(0,0);api.rustias_note(0,60,100);assert.ok(api.rustias_voices()>0,'Replacing the source makes the instrument playable');
  assert.equal(load(api,drum.engine),1);api.rustias_rdl_mute(2,1);api.rustias_drum_pad(0,100);assert.equal(api.rustias_voices(),0,'Missing drum is blocked');api.rustias_note(1,37+3,100);assert.ok(api.rustias_voices()>0,'Drum owner dispatch is independent of its unused timbre OSC 1');
  api.rustias_stop();
  assert.equal(parseRdl(api,chunk('316p',fixture.programs[0])).programs.length,1);assert.equal(parseRdl(api,chunk('316P',chunk('316p',fixture.programs[0]))).programs.length,1);
  const noKit=Buffer.from(fixture.programs[7]);const incomplete=parseRdl(api,chunk('316p',noKit));assert.equal(incomplete.programs[0].rdl.unavailable.filter(s=>s.drum!=null).length,16,'Absent kit does not become a default sounding kit');
  assert.throws(()=>parseRdl(api,fixture.bytes.subarray(0,fixture.bytes.length-1)),/length/);assert.throws(()=>parseRdl(api,Buffer.concat([fixture.bytes,Buffer.from([0])])),/complete/);
  assert.throws(()=>parseRdl(api,chunk('316p',Buffer.alloc(1789))),/Truncated/);
  assert.throws(()=>parseRdl(api,chunk('316B',Buffer.concat([chunk('316P',chunk('316p',fixture.programs[0])),chunk('316P',chunk('316p',fixture.programs[1]))]))),/Duplicate/);
  assert.throws(()=>parseRdl(api,chunk('316B',chunk('316G',chunk('316g',Buffer.alloc(12))))),/Global/);
  assert.equal(api.rustias_rdl_buffer(8*1024*1024+1),0);
  const disk=storage(4.5*1024*1024),store=new PatchStore(disk);
  const oldDisk=storage(),oldStore=new PatchStore(oldDisk),oldPatch={id:'format2',name:'Before compression',snapshot:{version:1},updatedAt:1};oldDisk.setItem('rustias.patches.v2',JSON.stringify({format:2,pool:[['o',[['id','format2'],['name','Before compression'],['snapshot',{$:1}],['updatedAt',1]]],['o',[['version',1]]],['a',[{$:0}]]],root:{$:2}}));assert.deepEqual(oldStore.list(),[oldPatch],'Pre-compression pooled libraries remain readable');
  const original={id:'old-patch',name:'Existing patch',updatedAt:1,snapshot:patches[0].snapshot};disk.setItem('rustias.patches.v1',JSON.stringify([original]));
  assert.deepEqual(store.import(patches),{added:256,duplicates:0});assert.equal(store.list().length,257);assert.deepEqual(store.list()[0],original,'Legacy saved patches survive bank import');
  const bytes=disk.getItem('rustias.patches.v2').length*2;assert.ok(bytes<4.5*1024*1024,'A complete bank fits browser storage with room for a session');
  const restored=new PatchStore(disk).list();assert.deepEqual(restored.find(p=>p.id===patches[7].id).snapshot,patches[7].snapshot,'Kits, sequencers and original records hydrate completely');
  const edited=structuredClone(restored.find(p=>p.id===patches[0].id));edited.snapshot.engine.timbres[0][1]=31;edited.snapshot.sequencer.tracks[0].steps[127].notes=[60,64,67];edited.snapshot.sequencer.tracks[0].length=128;
  store.save('Edited RDL',edited.snapshot,edited.id);assert.deepEqual(store.import(patches),{added:0,duplicates:256});assert.equal(store.list().find(p=>p.id===edited.id).name,'Edited RDL');assert.deepEqual(store.list().find(p=>p.id===edited.id).snapshot,edited.snapshot,'Reimport preserves user edits and does not duplicate programs');
  const full=storage(200000),fullStore=new PatchStore(full);fullStore.save('Original',patches[0].snapshot,'existing');const before=full.getItem('rustias.patches.v2');assert.throws(()=>fullStore.import(patches),e=>e.name==='QuotaExceededError');assert.equal(full.getItem('rustias.patches.v2'),before);assert.equal(fullStore.list().length,1,'Quota failure leaves the complete existing library unchanged');
  return {programs:256,kits:32,storageBytes:bytes,features:['RDL: full/partial bank and single program, all native controls, linked drum kit, original records, PCM/input mute, malformed file rejection, atomic deduplicated storage',sequenceFeature]};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  const module=await WebAssembly.compile(fs.readFileSync(process.argv[2]??'dist/rustias.wasm'));const result=verifyRdl(module);
  if(process.argv[3]){
    const api=new WebAssembly.Instance(module,{}).exports,raw=fs.readFileSync(process.argv[3]),library=parseRdl(api,raw);
    api.rustias_init();library.programs.forEach(p=>assert.equal(load(api,p.engine),1,`Real RDL program ${p.slot}`));
    const disk=storage(4.5*1024*1024),store=new PatchStore(disk),patches=rdlPatches(library,'Local-backup.rdl',createHash('sha256').update(raw).digest('hex')),parameters=webParameters(JSON.parse(fs.readFileSync(new URL('../crates/radias-synth-infrastructure/src/parameters.json',import.meta.url))));patches.forEach(p=>normalizeProgram(p.snapshot,parameters));store.import(patches);
    result.localBackup={programs:library.programs.length,kits:library.drum_kits,storageBytes:disk.getItem('rustias.patches.v2').length*2};
  }
  console.log(JSON.stringify(result,null,2));
}
