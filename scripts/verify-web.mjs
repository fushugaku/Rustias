import fs from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {SequenceClock,StepAudition,emptySequence,validateSequence,RESOLUTIONS,stepFrames} from "../web/sequence.js";
import {drumSequenceKit,sequenceLabels} from "../web/sequence-labels.js";
import {emptySamples,validateSamples,migrateSampleAmplifiers,sampleValues,validSampleSource} from '../web/sample-state.js';
import {copySteps,pasteSteps} from '../web/sequence-edit.js';
import {PatchStore} from "../web/patches.js";
import {defaultCircuit,validateCircuits,connect,audioCircuit} from "../web/circuit.js";
import {verifyRdl} from './verify-rdl.mjs';
import {verifyPrograms} from './verify-programs.mjs';

const path = process.argv[2] ?? "dist/rustias.wasm";
const module = await WebAssembly.compile(fs.readFileSync(path));
assert.deepEqual(WebAssembly.Module.imports(module), [], "Standalone Wasm must have no runtime imports");
const api = new WebAssembly.Instance(module, {}).exports;
api.rustias_init();
assert.equal(api.rustias_note(4, 69, 100), 0);
assert.equal(api.rustias_control(0, 0, 6), 0);
assert.equal(api.rustias_note(0, 69, 100), 1);
let peak = 0;
for (let block = 0; block < 100; block++) {
  const samples = new Float32Array(api.memory.buffer, api.rustias_render(), 256);
  for (const sample of samples) { assert.ok(Number.isFinite(sample)); peak = Math.max(peak, Math.abs(sample)); }
}
assert.ok(peak > 0.001, "The native voice must produce audio");
api.rustias_note(0, 69, 0);
for (let block = 0; block < 400; block++) api.rustias_render();
assert.equal(api.rustias_voices(), 0, "Released notes must retire");

const schema = JSON.parse(fs.readFileSync(new URL("../crates/radias-synth-infrastructure/src/parameters.json", import.meta.url)));
assert.equal(api.rustias_parameter_count(), schema.length);
for (const p of schema) {
  assert.equal(api.rustias_control(0, p.id, p.default), 1, `${p.group} ${p.label}`);
  assert.equal(api.rustias_control(0, p.id, p.max + 1), 0, `${p.id} validation`);
}
function control(id, value, timbre = 0) { assert.equal(api.rustias_control(timbre, id, value), 1, `control ${id}`); }
function render(frames = 12000) {
  const result=[];
  for(let i=0;i<frames;i+=128){ const pointer=api.rustias_render(); const b=new Float32Array(api.memory.buffer,pointer,256); for(let j=0;j<256;j+=2){assert.ok(Number.isFinite(b[j])&&Number.isFinite(b[j+1]));result.push(b[j]);} }
  return result;
}
function rms(samples) {return Math.sqrt(samples.reduce((sum,x)=>sum+x*x,0)/samples.length);}
function save() { const length=api.rustias_save(); return JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),length)).toString()); }
function load(program) {
  const data=Buffer.from(JSON.stringify(program));
  new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),data.length).set(data);
  return api.rustias_load(data.length);
}
const features=[];
features.push(...verifyRdl(module).features);
for(let mode=0;mode<4;mode++)for(let wave=0;wave<(mode===0?6:4);wave++) {
  api.rustias_init();control(0,wave);control(10,mode);control(11,64);api.rustias_note(0,60,100);
  assert.ok(rms(render(6000))>0.0001, `OSC1 ${wave}/${mode} must render`);
}
features.push("all 18 OSC1 waveform/mode combinations");
for(let mode=0;mode<4;mode++) {
  api.rustias_init();control(17,0);control(18,127);control(14,mode);api.rustias_note(0,64,100);
  assert.ok(rms(render())>0.0001, `OSC2 mode ${mode}`);
}
features.push("OSC2 / ring / sync / mixer");
for(let route=1;route<4;route++)for(let type=0;type<4;type++) {
  api.rustias_init();control(20,route);control(21,type);control(22,80);control(23,32);api.rustias_note(0,60,100);
  assert.ok(rms(render())>0.0001, `Filter2 ${route}/${type}`);
}
for(let shaper=0;shaper<12;shaper++)for(let position=0;position<2;position++){
  api.rustias_init();control(29,shaper===0?1:2);if(shaper>0)control(154,shaper-1);control(30,position);control(31,70);api.rustias_note(0,60,100);
  assert.ok(rms(render())>0.0001,`Drive/WS ${shaper}/${position}`);
}
api.rustias_init();control(29,2);control(154,8);control(31,90);control(30,1);
for(const mode of [0,1,2]){control(29,mode);assert.equal(api.rustias_value(0,154),8);assert.equal(api.rustias_value(0,31),90);assert.equal(api.rustias_value(0,30),1);}
features.push("all filter routes, Filter2 + Comb, Drive and all 11 WS types, both positions");
api.rustias_init();api.rustias_note(0,69,100);const dry=render();
control(90,4);control(91,11);control(92,110);control(83,86);const modulated=render();
assert.ok(Math.abs(rms(dry)-rms(modulated))>0.0001,"LFO2 -> amplifier patch must change sound");
features.push("live LFO / modulation matrix");
api.rustias_init();control(62,0);api.rustias_note(0,60,100);api.rustias_note(0,64,100);assert.equal(api.rustias_voices(),1,"mono allocation");
api.rustias_init();control(67,1);control(68,4);control(69,40);control(70,100);api.rustias_note(0,60,100);assert.equal(api.rustias_voices(),4,"unison allocation");render();
features.push("mono and instrument unison allocation");
api.rustias_init();api.rustias_note(0,69,100);render();api.rustias_midi(0xb0,11,0);
assert.equal(api.rustias_value(0,150),0);assert.ok(rms(render().slice(3000))<0.00001,"Expression CC11 must silence output");
api.rustias_midi(0xb0,11,127);assert.ok(rms(render())>0.0001,"Expression restores gain");
api.rustias_midi(0xe0,0,96);assert.equal(api.rustias_value(0,137),4096);
api.rustias_midi(0xb0,1,93);assert.equal(api.rustias_value(0,138),93);
api.rustias_midi(0xb0,65,127);assert.equal(api.rustias_value(0,139),1);
control(151,0);control(153,1);control(149,0);api.rustias_midi(0xb0,11,0);assert.ok(rms(render())>0.0001,"mode 0 receive flag disables Expression");
control(149,1);assert.ok(rms(render().slice(3000))<0.00001,"mode 1 uses its independent Expression flag");
features.push("MIDI bend, wheel, Expression, portamento switch");
api.rustias_init();control(72,16);control(148,7);api.rustias_midi(0x97,69,100);assert.equal(api.rustias_voices(),1,"Global channel inheritance");
api.rustias_midi(0xb7,11,45);assert.equal(api.rustias_value(0,150),45);render();

api.rustias_init();control(140,1);api.rustias_drum_pad(0,100);assert.ok(rms(render())>0.0001);
const drumVoices=api.rustias_voices();control(142,1);assert.equal(api.rustias_voices(),drumVoices,"editing selection retains sounding drum voices");
control(0,4);control(11,72);control(147,2);control(146,40);control(143,80);assert.equal(api.rustias_voices(),drumVoices,"kit level preserves voices");
const program=save();control(140,0);assert.equal(load(program),1);assert.deepEqual(save(),program,"complete program round trip");
api.rustias_drum_pad(1,100);assert.ok(rms(render())>0.0001);api.rustias_drum_pad(1,0);
const invalid=structuredClone(program);invalid.drums[0][10]=99;assert.equal(load(invalid),0);assert.deepEqual(save(),program,"invalid import must leave program intact");
features.push("16 drum instruments, live kit controls, complete JSON round trip");

// The new PCM source must use the native controls and never double-trigger
// a synthetic drum. A looping sine provides deterministic audible evidence.
function upload(instrument,data,mode=0){const pointer=api.rustias_sample_buffer(instrument,data.length);assert.ok(pointer);new Float32Array(api.memory.buffer,pointer,data.length).set(data);assert.equal(api.rustias_sample_commit(instrument,data.length,mode),1);}
function pcmSetup(length=48000,mode=0){
  api.rustias_init();control(140,1);control(9,127);control(3,0);control(4,127);control(5,127);control(6,20);
  upload(0,Float32Array.from({length},(_,i)=>0.4*Math.sin(i*2*Math.PI*440/48000)),mode);
}
assert.equal(api.rustias_sample_buffer(16,100),0);assert.equal(api.rustias_sample_buffer(0,1),0);assert.equal(api.rustias_sample_buffer(0,1440001),0);
pcmSetup(4800);api.rustias_drum_pad(0,100);assert.equal(api.rustias_voices(),1,"PCM replaces the synthetic drum");assert.ok(rms(render(1500))>0.001);
api.rustias_drum_pad(0,0);assert.ok(rms(render(1500))>0.001,"One-shot survives key release");render(6000);assert.equal(api.rustias_voices(),0,"One-shot ends with its sample");
pcmSetup(48000,1);api.rustias_drum_pad(0,100);assert.ok(rms(render(2000))>0.001);api.rustias_drum_pad(0,0);render(6000);assert.equal(api.rustias_voices(),0,"Gate releases EG2");
pcmSetup(400,2);api.rustias_note(0,60,100);render(12000);assert.equal(api.rustias_voices(),1,"Loop survives multiple wraps");api.rustias_note(0,60,0);render(6000);assert.equal(api.rustias_voices(),0,"Loop releases with its key");
pcmSetup(48000,2);api.rustias_midi(0x90,60,100);assert.equal(api.rustias_voices(),1,"MIDI triggers PCM");const pcmDry=rms(render());assert.ok(pcmDry>0.001);api.rustias_midi(0xb0,11,0);assert.ok(rms(render().slice(3000))<0.00001,"PCM follows Expression");api.rustias_midi(0xb0,11,127);assert.ok(rms(render())>0.001);
control(29,1);control(31,100);assert.ok(Math.abs(rms(render())-pcmDry)>0.0001,"Native Drive processes PCM");
for(let type=0;type<11;type++)for(let position=0;position<2;position++){control(29,2);control(154,type);control(30,position);render(1000);}
control(29,0);control(9,0);control(1,15);assert.ok(rms(render(18000).slice(6000))<pcmDry*0.5,"Filter1 processes PCM");
api.rustias_midi(0xb0,120,0);assert.equal(api.rustias_voices(),0,"All Sound Off silences PCM");
pcmSetup(4800);upload(1,new Float32Array(4800).fill(0.2));api.rustias_drum_control(0,147,1);api.rustias_drum_control(1,147,1);api.rustias_drum_pad(0,100);assert.equal(api.rustias_voices(),1);api.rustias_drum_pad(1,100);assert.equal(api.rustias_voices(),1,"Exclusive groups choke PCM");
api.rustias_sample_clear(0);api.rustias_stop();api.rustias_drum_pad(0,100);assert.ok(rms(render())>0.0001,"Synth drum returns after clearing PCM");
pcmSetup(400,2);api.rustias_note(0,60,100);control(146,72);api.rustias_note(0,60,0);render(6000);assert.equal(api.rustias_voices(),0,"Reassigning a trigger must still release the original PCM note");
features.push("browser PCM: one-shot, gate, loop, MIDI, Expression, native filters, Drive / 11 WS types, choke and source replacement");

// Kit level is shared; ordinary synth controls address the selected drum.
pcmSetup(48000,2);upload(1,Float32Array.from({length:48000},(_,i)=>0.4*Math.sin(i*2*Math.PI*440/48000)),2);
for(const [id,value] of [[3,0],[4,127],[5,127],[6,20],[9,127]])assert.equal(api.rustias_drum_control(1,id,value),1);
api.rustias_drum_pad(1,100);const drum1Dry=rms(render());control(7,0);assert.ok(rms(render())>drum1Dry*.9,'Editing drum 1 must not mute sounding drum 2');
control(142,1);control(7,0);assert.ok(rms(render().slice(3000))<.00001,'Amp level controls the selected PCM drum live');
control(7,100);assert.ok(rms(render())>.001,'Amp level restores the PCM drum live');
control(9,0);control(1,15);assert.ok(rms(render().slice(3000))<drum1Dry*.5,'Cutoff changes the selected PCM drum live');
control(9,127);control(8,0);render();const pannedLeft=api.rustias_render(),leftBlock=new Float32Array(api.memory.buffer,pannedLeft,256);assert.ok(leftBlock.filter((_,i)=>i%2===1).every(x=>Math.abs(x)<.00001),'Instrument pan reaches PCM');
control(8,64);control(90,4);control(91,11);control(92,110);control(83,86);const patchLow=rms(render());const patchHigh=rms(render());assert.ok(Math.abs(patchLow-patchHigh)>.0001,'LFO2 -> amp modulation changes PCM');
features.push('selected PCM drum: independent live amp, cutoff, pan and LFO/virtual-patch processing');

pcmSetup(48000,2);control(152,0);control(7,64);
upload(1,Float32Array.from({length:48000},(_,i)=>0.4*Math.sin(i*2*Math.PI*440/48000)),2);
for(const [id,value] of [[3,0],[4,127],[5,127],[6,20],[9,127],[7,64]])assert.equal(api.rustias_drum_control(1,id,value),1);
api.rustias_drum_pad(0,100);const ampDry=rms(render().slice(3000));assert.ok(ampDry>.001);
control(114,-64);assert.ok(rms(render().slice(3000))<.00001,'PCM Level offset changes the selected amplifier live');control(114,0);
assert.equal(api.rustias_drum_control(1,114,-64),1);assert.ok(rms(render().slice(3000))>ampDry*.9,'A different instrument has its own Level offset');
control(115,0);assert.ok(rms(render().slice(3000))<.00001,'Manual Source gain silences the selected sample');control(115,32512);assert.ok(rms(render().slice(3000))>ampDry*.9);
control(116,1);control(117,0);assert.ok(rms(render().slice(3000))<.00001,'PCM MIDI volume acts when RX is enabled');
api.rustias_midi(0xb1,7,127);assert.equal(api.rustias_value(0,117),0,'CC7 on another channel does not change the kit');
api.rustias_midi(0xb0,7,127);assert.ok(rms(render().slice(3000))>ampDry*.9,'CC7 restores enabled PCM instruments');
assert.equal(api.rustias_drum_control(1,117,23),1);api.rustias_midi(0xb0,7,95);const ampProgram=save();assert.equal(ampProgram.drums[0][117],95);assert.equal(ampProgram.drums[1][117],23,'CC7 ignores instruments with MIDI volume RX off');
assert.equal(ampProgram.timbres[0][114],0);assert.equal(ampProgram.timbres[0][115],32512,'PCM edits do not change the common timbre amplifier');
assert.equal(api.rustias_drum_control(0,114,-65),0);assert.equal(api.rustias_drum_control(0,115,32768),0);assert.deepEqual(save(),ampProgram,'Invalid amplifier edits leave all instruments intact');
assert.equal(load(ampProgram),1);assert.deepEqual(save(),ampProgram,'Independent PCM amplifier settings survive JSON reload');
api.rustias_stop();api.rustias_drum_pad(1,100);assert.ok(rms(render().slice(3000))<.00001,'Reload preserves the second instrument offset');
control(142,1);control(114,0);assert.ok(rms(render().slice(3000))>ampDry*.9,'The second amplifier restores independently');
api.rustias_stop();control(142,0);control(116,0);control(52,127);api.rustias_drum_pad(0,100);const keyRoot=rms(render().slice(3000));control(53,76);const keyHigh=rms(render().slice(3000));assert.ok(keyHigh>keyRoot*1.5,'PCM key tracking follows live sample transpose');control(52,0);assert.ok(rms(render().slice(3000))<keyRoot*.3,'PCM Key track must survive modulation updates');
features.push('PCM amplifier: independent live Key track / Level offset / manual Source gain / MIDI volume, per-instrument CC7 receive and persistence');
const legacyPcm=emptySamples();legacyPcm.version=1;legacyPcm.slots[0]={source:'808:kick-01',mode:2};
const legacyGain=structuredClone(ampProgram);legacyGain.timbres[0][115]=8000;const upgradedSamples=validateSamples(legacyPcm),upgradedGain=migrateSampleAmplifiers(legacyGain,legacyPcm,upgradedSamples);
assert.equal(upgradedSamples.version,3);assert.equal(upgradedGain.drums[0][115],8000,'Older PCM patches keep their common manual gain');assert.equal(legacyGain.drums[0][115],32512,'Migration leaves the source patch intact');
upgradedGain.drums[0][115]=11000;assert.equal(migrateSampleAmplifiers(upgradedGain,upgradedSamples,upgradedSamples).drums[0][115],11000,'New patches preserve independent gain edits');

// Web-only voice masks include bit 127; native builds retain 24 voices.
assert.equal(api.rustias_voice_capacity(),128);
api.rustias_init();control(3,0);control(4,127);control(5,127);control(6,8);
for(let note=0;note<128;note++)api.rustias_note(0,note,100);
assert.equal(api.rustias_voices(),128,'128 independent native voices allocate');render(1000);
for(let note=64;note<128;note++)api.rustias_note(0,note,0);
render(14000);assert.equal(api.rustias_voices(),64,'Upper 64 voice-mask bits release without touching lower voices');
for(let note=0;note<64;note++)api.rustias_note(0,note,0);render(14000);assert.equal(api.rustias_voices(),0);
api.rustias_init();control(67,1);control(68,8);
for(let note=36;note<52;note++)api.rustias_note(0,note,100);
assert.equal(api.rustias_voices(),128,'Sixteen 8-voice Unison groups fill the expanded pool');
api.rustias_note(0,52,100);assert.equal(api.rustias_voices(),128,'Full-pool replacement never exceeds 128');render(1000);api.rustias_stop();
api.rustias_init();control(0,4);control(20,1);control(21,3);control(29,2);control(154,10);control(31,30);control(78,1);control(79,schema[79].max);
for(let note=0;note<128;note++)api.rustias_note(0,note,100);assert.equal(api.rustias_voices(),128,'Cost accounting allows 128 fully processed voices');render(3000);api.rustias_stop();
features.push('Web-only 128-voice native allocation, bit 127 release and 8-voice Unison; desktop capacity remains 24');

const libraryValues=sampleValues(schema.map(p=>p.default));libraryValues[9]=127;libraryValues[7]=64;libraryValues[6]=8;
function libraryUpload(asset,data){const pointer=api.rustias_library_sample_buffer(asset,data.length);assert.ok(pointer);new Float32Array(api.memory.buffer,pointer,data.length).set(data);assert.equal(api.rustias_library_sample_commit(asset,data.length),1);}
function libraryProfile(id,asset,timbre=0,mode=2,values=libraryValues){assert.equal(api.rustias_library_profile(id,asset,timbre,mode),1);values.forEach((v,p)=>assert.equal(api.rustias_library_control(id,p,v),1));api.rustias_library_sync();}
function libraryControl(id,parameter,value){assert.equal(api.rustias_library_control(id,parameter,value),1);api.rustias_library_sync();}
const sine=Float32Array.from({length:4800},(_,i)=>.15*Math.sin(i*2*Math.PI*440/48000));
api.rustias_init();assert.equal(api.rustias_library_sample_buffer(0,100),0);assert.equal(api.rustias_library_profile(100,9,0,0),0);
libraryUpload(99,sine);libraryProfile(321,99,2);libraryProfile(322,99,1);
assert.equal(api.rustias_library_note(2,321,100),1);assert.equal(api.rustias_voices(),1,'Any sample plays on another timbre with Drum mode off');const directDry=rms(render().slice(3000));assert.ok(directDry>.001);
libraryControl(322,7,0);assert.ok(rms(render().slice(3000))>directDry*.9,'Profiles on separate timbres edit independently');
libraryControl(321,7,0);assert.ok(rms(render().slice(3000))<.00001,'Direct sample amplifier edits are live');libraryControl(321,7,64);
libraryControl(321,9,0);libraryControl(321,1,10);assert.ok(rms(render().slice(3000))<directDry*.5,'Direct sample filters use their own native graph');libraryControl(321,9,127);
control(140,1);control(140,0);assert.equal(api.rustias_voices(),1,'Kit mode switches preserve independent sequence samples');
assert.equal(api.rustias_library_control(321,7,128),0);assert.equal(api.rustias_library_note(0,321,100),0,'Profile ownership is enforced');
libraryControl(321,116,1);api.rustias_midi(0xb2,7,0);assert.equal(api.rustias_library_value(321,117),0);assert.ok(rms(render().slice(3000))<.00001,'Direct samples receive CC7 on their own timbre');api.rustias_midi(0xb2,7,127);
for(let i=0;i<130;i++)api.rustias_library_note(2,321,100);assert.equal(api.rustias_voices(),128,'PCM allocation reaches 128');
api.rustias_note(0,60,100);assert.equal(api.rustias_voices(),128,'Native and PCM share one 128-voice limit');
api.rustias_library_note(2,321,100);assert.equal(api.rustias_voices(),128,'PCM can replace a native voice in a full pool');render(1000);api.rustias_stop();
for(let i=0;i<128;i++)api.rustias_note(0,i,100);assert.equal(api.rustias_voices(),128);api.rustias_library_note(2,321,100);assert.equal(api.rustias_voices(),128);api.rustias_stop();
api.rustias_library_note(2,321,100);api.rustias_midi(0xb0,120,0);assert.equal(api.rustias_voices(),1,'All Sound Off on the kit channel preserves samples on other timbres');api.rustias_library_note(2,321,0);render(10000);assert.equal(api.rustias_voices(),0,'Library Gate/Loop follows note off');
api.rustias_library_reset();assert.equal(api.rustias_library_note(2,321,100),0,'Switching patch discards previous sequence samples');
api.rustias_init();libraryUpload(99,sine);
for(let i=0;i<130;i++){const values=[...libraryValues];values[7]=i<2?64:0;libraryProfile(i+1,99,2,2,values);api.rustias_library_note(2,i+1,100);}
assert.equal(api.rustias_voices(),128);assert.ok(rms(render(6000))<.00001,'Same-frame overload steals the oldest samples, rather than repeatedly replacing the newest slot');
features.push('Arbitrary sequence samples beyond the 16 kit slots, independent timbre profiles, live DSP, MIDI, release and mixed 128-voice stealing');

for(const kind of ['pcm','native','library','timbre']){
  const levels=[];for(const db of [0,12]){
    if(kind==='pcm')pcmSetup(48000,2);else api.rustias_init();
    if(kind==='native'){control(140,1);control(3,0);control(4,127);control(5,127);control(7,30);}
    if(kind==='library'){libraryUpload(99,sine);libraryProfile(321,99,2);}
    assert.equal(api.rustias_drum_gain(db),1);
    if(kind==='pcm'||kind==='native')api.rustias_drum_pad(0,100);else if(kind==='library')api.rustias_library_note(2,321,100);else api.rustias_note(1,69,100);
    levels.push(rms(render().slice(3000)));
  }
  const ratio=levels[1]/levels[0];assert.ok(Math.abs(ratio-(kind==='timbre'?1:10**(12/20)))<.06,`${kind} kit gain ratio ${ratio}`);
}
assert.equal(api.rustias_drum_gain(25),0);assert.equal(api.rustias_drum_gain(-25),0);
const sampleState=emptySamples();sampleState.kitGain=19;sampleState.library.push({source:'909:bt7a0d7',timbre:2,mode:2,name:'909 Kick',values:libraryValues});
assert.deepEqual(validateSamples(sampleState,schema),sampleState,'Gain and per-source parameters round-trip in browser patches');
features.push('Shared -24…+24 dB Drum Kit Gain boosts native and PCM drums, leaves melodic synths unchanged and persists');

const manifest=JSON.parse(fs.readFileSync(new URL("../web/samples/manifest.json",import.meta.url)));
assert.equal(manifest.samples.length,224);assert.equal(manifest.banks.find(bank=>bank.id==='808').license,"CC0-1.0");const hashes=new Set();
for(const sample of manifest.samples){const data=fs.readFileSync(new URL(`../web/samples/${sample.file}`,import.meta.url));assert.equal(data.toString("ascii",0,4),"RIFF");assert.equal(data.toString("ascii",8,12),"WAVE");assert.ok(sample.duration>0);assert.equal(createHash("sha256").update(data).digest("hex"),sample.sha256);hashes.add(sample.sha256);}
const samples808=manifest.samples.filter(sample=>sample.id.startsWith('808:')),samples909=manifest.samples.filter(sample=>sample.id.startsWith('909:'));
assert.equal(samples808.length,64);assert.equal(new Set(samples808.map(sample=>sample.sha256)).size,64,'All 808 recordings remain distinct');assert.equal(samples909.length,160,'The original 909 set must stay complete');
for(const bank of manifest.banks){assert.equal(manifest.samples.filter(sample=>sample.id.startsWith(`${bank.id}:`)).length,bank.count);assert.ok(bank.sourceCommit);const license=fs.readFileSync(new URL(`../web/samples/${bank.licenseFile}`,import.meta.url));if(bank.licenseSha256)assert.equal(createHash('sha256').update(license).digest('hex'),bank.licenseSha256,'Original license text stays unmodified');}
features.push("64 CC0 808 WAVs plus the complete 160-file TR-909 set, source revisions, original licenses and verified checksums");

// Labels must describe the instruments that the native MIDI path triggers,
// rather than assuming the kit occupies C4 through D#5.
api.rustias_init();control(140,1);control(141,2);control(145,76);
assert.equal(api.rustias_drum_control(0,146,36),1);assert.equal(api.rustias_drum_control(1,146,36),1);
const drumProgram=save(),drumNames=['808 Kick 07','my snare.wav'];
let drumKit=drumSequenceKit(drumProgram.timbres[0],drumProgram.drums,drumNames);
assert.equal(drumKit.timbre,2);assert.equal(drumKit.instruments[0].note,48);
const drumNotes=[48,49];assert.deepEqual(sequenceLabels(drumNotes,drumKit),['808 Kick 07','my snare.wav','Unassigned 49']);assert.deepEqual(drumNotes,[48,49],"Changing labels must preserve stored MIDI notes");
api.rustias_note(2,48,100);assert.equal(api.rustias_voices(),2,"Both labelled drums sound at their shared transposed trigger");api.rustias_stop();
drumNames[1]='new sample.wav';drumKit=drumSequenceKit(drumProgram.timbres[0],drumProgram.drums,drumNames);assert.deepEqual(sequenceLabels([48],drumKit),['808 Kick 07','new sample.wav'],"Assignments refresh sample names");
drumProgram.drums[0][146]=127;drumKit=drumSequenceKit(drumProgram.timbres[0],drumProgram.drums,drumNames);assert.equal(drumKit.instruments[0].note,139);assert.deepEqual(sequenceLabels([127],drumKit),['Unassigned 127'],"Unreachable triggers must not clamp to another MIDI note");
drumProgram.timbres[0][140]=0;assert.equal(drumSequenceKit(drumProgram.timbres[0],drumProgram.drums,drumNames),null);assert.deepEqual(sequenceLabels([60,64],null),['C4','E4'],"Other timbres and Drum mode off retain pitch labels");
features.push("sequencer drum sample names follow native trigger/transpose, shared triggers, custom assignments and Drum mode");

const sequence=emptySequence();sequence.tracks.forEach((track,t)=>{track.length=t+1;track.steps[0]={notes:[60+t,64+t,67+t],velocity:90+t,gate:75};});
let time=0;const events=[];const clock=new SequenceClock((t,n,v)=>events.push({time,t,n,v}));clock.setConfig(sequence);clock.play();clock.beforeRender(128);time+=128;
assert.equal(events.length,12);assert.ok(events.every(e=>e.time===0&&e.v>0),"All four chords begin in the same native block");
while(time<4608){clock.beforeRender(128);time+=128;}clock.beforeRender(128);time+=128;
assert.equal(events.filter(e=>!e.v).length,12,"Gate releases every chord note");
while(time<6144){clock.beforeRender(128);time+=128;}assert.equal(events.filter(e=>e.v>0&&e.t===0).length,6,"Per-track length loops independently");assert.equal(events.filter(e=>e.v>0&&e.t===3).length,3);
clock.stop();assert.deepEqual(clock.status().positions,[-1,-1,-1,-1]);clock.play();clock.beforeRender();assert.ok(clock.status().positions.every(p=>p===0));clock.reset();clock.beforeRender();assert.ok(clock.status().positions.every(p=>p===0),"Reset starts all lanes together");
const muted=structuredClone(sequence);muted.tracks[0].enabled=false;const beforeMute=events.length;clock.setConfig(muted);assert.equal(events.slice(beforeMute).filter(e=>e.t===0&&!e.v).length,3,"Mute releases the track immediately");
const remaining=clock.nextSteps[0]-clock.frame;clock.setTempo(240);assert.ok(Math.abs(clock.nextSteps[0]-clock.frame-remaining/2)<0.001,"Tempo changes preserve fractional position");clock.stop();
const malformed=structuredClone(sequence);malformed.tracks[0].steps[0].notes=[128];assert.throws(()=>validateSequence(malformed));
const longSequence=emptySequence();longSequence.tracks[0].length=128;longSequence.tracks[0].steps[127].notes=[72,76];longSequence.tracks.slice(1).forEach(t=>t.enabled=false);const longEvents=[];const longClock=new SequenceClock((t,n,v)=>longEvents.push({t,n,v}));longClock.setConfig(longSequence);longClock.play();for(let i=0;i<6001;i++)longClock.beforeRender(128);assert.deepEqual(longEvents.filter(e=>e.v).map(e=>e.n),[72,76],"Step 128 is played before the track wraps");assert.equal(longClock.status().positions[0],0,"A 128-step lane wraps independently");const legacySequence=emptySequence();legacySequence.tracks.forEach(t=>t.steps=t.steps.slice(0,16));assert.equal(validateSequence(legacySequence).tracks[0].steps.length,128,"Existing 16-step patches migrate");
for(const resolution of RESOLUTIONS){assert.ok(stepFrames(120,resolution)>=3000&&stepFrames(120,resolution)<=96000);}
assert.equal(stepFrames(120,"1/16"),6000);assert.equal(stepFrames(120,"1/4"),24000);assert.equal(stepFrames(120,"1/3"),32000);assert.equal(stepFrames(120,"3/16"),18000);
const mixed=emptySequence();mixed.tracks.forEach((track,t)=>{track.length=1;track.resolution=["1/16","1/3","1/4","3/16"][t];track.steps[0].notes=[60+t];});let mixedTime=0;const mixedEvents=[];const mixedClock=new SequenceClock((t,n,v)=>{if(v)mixedEvents.push({t,time:mixedTime});});mixedClock.setConfig(mixed);mixedClock.play();while(mixedTime<=96000){mixedClock.beforeRender(128);mixedTime+=128;}
for(let t=0;t<4;t++){const onsets=mixedEvents.filter(e=>e.t===t).map(e=>e.time),period=stepFrames(120,mixed.tracks[t].resolution);assert.equal(onsets[0],0);onsets.forEach((time,i)=>assert.ok(time>=i*period&&time-i*period<128,"Each resolution retains a bounded native onset"));assert.equal(onsets.length,Math.floor(96000/period)+1);}
const resolutionAudition=[];const quarterAudition=new StepAudition((t,n,v)=>resolutionAudition.push(v));quarterAudition.play(0,{notes:[60],velocity:100,gate:100},120,"1/4");for(let i=0;i<100;i++)quarterAudition.beforeRender();assert.deepEqual(resolutionAudition,[100],"Audition uses the track's quarter-note duration");for(let i=0;i<100;i++)quarterAudition.beforeRender();assert.deepEqual(resolutionAudition,[100,0]);
const heard=[];const audition=new StepAudition((t,n,v)=>heard.push({t,n,v}));audition.play(2,{notes:[60,64,67],velocity:113,gate:80},120);assert.deepEqual(heard.map(e=>e.n),[60,64,67]);assert.ok(heard.every(e=>e.t===2&&e.v===113));for(let i=0;i<130;i++)audition.beforeRender();assert.equal(heard.filter(e=>!e.v).length,3,"Audition releases the whole chord");heard.length=0;audition.play(1,{notes:[62,65],velocity:87,gate:50},120);audition.play(1,{notes:[62,65,69],velocity:87,gate:50},120);assert.deepEqual(heard.filter(e=>e.v).map(e=>e.n),[62,65,62,65,69],"Every edit restarts all selected notes");audition.stop();
const sampleSequence=emptySequence();sampleSequence.tracks[0].steps[14]={notes:[60,64],samples:['808:kick-01','909:bt7a0d7'],velocity:87,gate:52};sampleSequence.tracks[0].steps[16]={notes:[],samples:['custom:qa-123'],velocity:113,gate:100};
const copied=copySteps(sampleSequence,0,17,14),pasted=pasteSteps(sampleSequence,copied,3,30);assert.equal(pasted.count,4);assert.deepEqual(pasted.sequence.tracks[3].steps.slice(30,34),copied.steps);assert.equal(pasted.sequence.tracks[3].length,34,'Paste expands the destination loop');
pasted.sequence.tracks[3].steps[30].samples.push('909:handclp1');assert.equal(sampleSequence.tracks[0].steps[14].samples.length,2,'Paste never aliases the original');assert.equal(copied.steps[0].samples.length,2,'Pasting leaves the reusable clipboard intact');
const kitClipboard=copySteps(sampleSequence,0,14,14,{instruments:[{note:60,source:'808:kick-01'},{note:64,source:'909:st7t7s7'}]});
const movedKit=pasteSteps(sampleSequence,kitClipboard,2,0);assert.deepEqual(movedKit.sequence.tracks[2].steps[0].notes,[]);assert.deepEqual(movedKit.sequence.tracks[2].steps[0].samples,['808:kick-01','909:bt7a0d7','909:st7t7s7'],'Drum triggers retain sample identity when copied to another timbre');
assert.deepEqual(pasteSteps(sampleSequence,kitClipboard,0,18).sequence.tracks[0].steps[18].notes,[60,64],'Copy within the kit preserves its live MIDI assignments');
const sameTrack=pasteSteps(sampleSequence,copied,0,18);assert.deepEqual(sameTrack.sequence.tracks[0].steps.slice(18,22),copied.steps);
const endPaste=pasteSteps(sampleSequence,copied,1,126);assert.equal(endPaste.count,2);assert.ok(endPaste.truncated);assert.equal(endPaste.sequence.tracks[1].length,128,'Paste at the end truncates without wrapping');
const sampleEvents=[];const sampleClock=new SequenceClock((t,n,v)=>sampleEvents.push({t,n,v}));sampleSequence.tracks[0].steps[0]=sampleSequence.tracks[0].steps[14];sampleSequence.tracks.slice(1).forEach(t=>t.enabled=false);sampleClock.setConfig(sampleSequence);sampleClock.play();sampleClock.beforeRender();assert.deepEqual(sampleEvents.map(e=>e.n),[60,64,'808:kick-01','909:bt7a0d7']);sampleClock.stop();assert.equal(sampleEvents.filter(e=>!e.v).length,4,'Clock releases pitches and samples together');
const directAudition=[];new StepAudition((t,n,v)=>directAudition.push(n)).play(0,sampleSequence.tracks[0].steps[0],120);assert.deepEqual(directAudition,[60,64,'808:kick-01','909:bt7a0d7']);
const fullChord=emptySequence();fullChord.tracks[0].steps[0].notes=Array.from({length:128},(_,i)=>i);assert.equal(validateSequence(fullChord).tracks[0].steps[0].notes.length,128);fullChord.tracks[0].steps[0].samples=['808:kick-01'];assert.throws(()=>validateSequence(fullChord),'A step cannot request over 128 events');
features.push('Range Copy/Paste across timbres and pattern banks preserves gaps, chords, samples, velocity and gate, without aliases or end-of-pattern wrapping');

const fakeStorage=new Map();fakeStorage.getItem=fakeStorage.get.bind(fakeStorage);fakeStorage.setItem=fakeStorage.set.bind(fakeStorage);const patches=new PatchStore(fakeStorage);const snapshot={version:2,engine:program,sequencer:sequence,samples:{version:1,slots:Array.from({length:16},()=>({source:"synth",mode:0}))}};
const patch=patches.save("Four timbres",snapshot);assert.deepEqual(new PatchStore(fakeStorage).list()[0].snapshot,snapshot);patches.save("Renamed",snapshot,patch.id);assert.equal(patches.list().length,1);patches.saveSession({snapshot,selected:3,volume:45});assert.deepEqual(patches.session().snapshot,snapshot);
features.push("four synchronized polyphonic sequencers up to 128 steps, complete chord audition, per-track straight/triplet/dotted resolution, gate, mute, length, reset, tempo and browser patch persistence");

const sequenceSource = fs.readFileSync(new URL("../web/sequence.js", import.meta.url),"utf8").replace(/^export /gm, "").replace(/^import .*;\n/gm,"");
const workletSource = fs.readFileSync(new URL("../web/worklet.js", import.meta.url), "utf8");

// Modular programs are genuinely evaluated per voice in the shared Rust kernel.
function setCircuit(c,t=0,accepted=1){const bytes=Buffer.from(JSON.stringify(audioCircuit(c)));new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),bytes.length).set(bytes);assert.equal(api.rustias_circuit(t,bytes.length),accepted);}
function graphNote(c,seconds=.3,note=69){api.rustias_init();control(3,0);control(4,127);control(5,127);control(6,8);setCircuit(c);api.rustias_note(0,note,100);return render(Math.round(seconds*48000)).slice(4000);}
function frequency(audio){let crossings=0;for(let i=1;i<audio.length;i++)if(audio[i-1]<=0&&audio[i]>0)crossings++;return crossings*48000/audio.length;}
let circuit=defaultCircuit();circuit.enabled=true;
const graphDry=rms(graphNote(circuit));assert.ok(graphDry>.001,'Connected native modules sound');
const disconnected=structuredClone(circuit);disconnected.wires=disconnected.wires.filter(w=>w.to!==15);assert.ok(rms(graphNote(disconnected))<.000001,'Disconnecting Output silences all voices');
circuit=connect(circuit,0,7,'in');const bypass=rms(graphNote(circuit));assert.ok(bypass>.001,'Rewiring OSC1 directly to Amp produces audio');
circuit.nodes.push({id:16,kind:'oscillator',x:24,y:1000,params:{wave:3,semitone:0,level:64}});circuit=connect(circuit,16,7,'in');
const extraSound=graphNote(circuit,.6),extraHz=frequency(extraSound);assert.ok(Math.abs(extraHz-440)<4,`New oscillator follows native pitch: ${extraHz}`);
circuit.nodes.find(n=>n.id===16).params.semitone=12;const octaveHz=frequency(graphNote(circuit,.6));assert.ok(Math.abs(octaveHz-880)<5,'Added oscillator has independent octave tuning');
circuit.nodes.find(n=>n.id===16).params.semitone=0;
circuit.nodes.push({id:17,kind:'filter',x:374,y:1000,params:{cutoff:127,resonance:0,morph:0}});circuit=connect(circuit,16,17,'in');circuit=connect(circuit,17,7,'in');
const bright=rms(graphNote(circuit));circuit.nodes.find(n=>n.id===17).params.cutoff=0;const dark=rms(graphNote(circuit));assert.ok(dark<bright*.2,'Additional filter uses native coefficients and its own cutoff');circuit.nodes.find(n=>n.id===17).params.cutoff=127;
circuit.nodes.push({id:18,kind:'vca',x:724,y:1000,params:{gain:0}});circuit=connect(circuit,7,18,'in');circuit=connect(circuit,18,15,'in');
const gainDry=rms(graphNote(circuit));circuit.nodes.find(n=>n.id===18).params.gain=-12;assert.ok(Math.abs(rms(graphNote(circuit))/gainDry-10**(-12/20))<.01,'New VCA changes gain in the audio graph');circuit.nodes.find(n=>n.id===18).params.gain=0;
circuit.nodes.push({id:19,kind:'lfo',x:1074,y:1000,params:{rate:4,shape:2,depth:100}});circuit=connect(circuit,19,18,'gain');assert.ok(rms(graphNote(circuit))>gainDry*.9,'CV cable runs the additional LFO through VCA gain');
const wrongType=structuredClone(circuit);wrongType.wires.push({from:0,to:19,port:'rate'});setCircuit(wrongType,0,0);
const feedback=structuredClone(circuit);feedback.wires=feedback.wires.filter(w=>w.to!==17||w.port!=='in');feedback.wires.push({from:18,to:17,port:'in'});setCircuit(feedback,0,0);assert.throws(()=>validateCircuits({version:1,tracks:[feedback,circuit,circuit,circuit]}));
circuit.nodes.push({id:20,kind:'envelope',x:1424,y:1000,params:{attack:0,decay:24,sustain:100,release:12}});circuit=connect(circuit,13,20,'gate');circuit=connect(circuit,20,18,'gain');
assert.ok(rms(graphNote(circuit))>0.0001,'Added ADSR has native envelope arithmetic');api.rustias_note(0,69,0);render(50000);assert.equal(api.rustias_voices(),0,'Custom envelopes and their voice states release completely');
// All additional WS modes use their own native state, including sub-oscillator gain slewing.
const addedDrive=defaultCircuit();addedDrive.enabled=true;addedDrive.nodes.push({id:16,kind:'shaper',x:24,y:1000,params:{mode:0,type:1,depth:90}});addedDrive.wires=[{from:0,to:16,port:'in'},{from:16,to:7,port:'in'},{from:7,to:15,port:'in'}];
const noShape=graphNote(addedDrive);for(let type=0;type<=10;type++){addedDrive.nodes.find(n=>n.id===16).params={mode:2,type,depth:90};const shaped=graphNote(addedDrive);const difference=rms(shaped.map((v,i)=>v-noShape[i]));assert.ok(difference>.00001,`Additional WS type ${type} affects audio`);}
// An EG2 → VCA cable retains the complete native release, without the fallback gate fade.
const egVca=defaultCircuit();egVca.enabled=true;egVca.nodes.push({id:16,kind:'vca',x:24,y:1000,params:{gain:0}});egVca.wires=[{from:0,to:16,port:'in'},{from:9,to:16,port:'gain'},{from:16,to:15,port:'in'}];
api.rustias_init();control(3,0);control(4,127);control(5,127);control(6,127);setCircuit(egVca);api.rustias_note(0,69,100);const egHeld=rms(render().slice(4000));api.rustias_note(0,69,0);assert.ok(rms(render(8000).slice(4000))>egHeld*.5,'EG2 preserves a long native release through an added VCA');api.rustias_stop();
// Each note receives independent oscillator/filter/envelope state, including high mask bits.
api.rustias_init();setCircuit(circuit);for(let n=0;n<128;n++)api.rustias_note(0,n,100);assert.equal(api.rustias_voices(),128);render(256);api.rustias_stop();
api.rustias_init();libraryUpload(99,sine);libraryProfile(321,99,2);let sampleCircuit=defaultCircuit();sampleCircuit.enabled=true;setCircuit(sampleCircuit,2);api.rustias_library_note(2,321,100);const sampleGraph=rms(render().slice(3000));assert.ok(sampleGraph>.001,'PCM voices pass through the same modular routing');sampleCircuit.wires=sampleCircuit.wires.filter(w=>w.to!==15);setCircuit(sampleCircuit,2);assert.ok(rms(render().slice(1000))<.000001,'PCM Output cable disconnect is live');api.rustias_stop();
const graphStore=new PatchStore(fakeStorage);const modularSnapshot={version:2,engine:program,sequencer:emptySequence(),samples:emptySamples(),circuits:{version:1,tracks:[circuit,defaultCircuit(),defaultCircuit(),defaultCircuit()]}};
const modularSaved=graphStore.save('Modular QA',modularSnapshot);assert.deepEqual(validateCircuits(graphStore.list().find(p=>p.id===modularSaved.id).snapshot.circuits),modularSnapshot.circuits,'Module positions, parameters and cables round-trip through patch storage');
features.push('Browser-only per-voice modular audio/CV routing: native modules, additional oscillators/filters/Drive/VCA/mixers/LFO/ADSR, 128 independent states, live PCM wiring, cycle/type validation and patch persistence');

const reports = [];
for (const sampleRate of [48000, 44100]) {
  let Processor;
  const messages = [];
  vm.runInNewContext(sequenceSource + "\n" + workletSource.replace(/^import .*;\n/gm,""), {
    sampleRate, WebAssembly, Float32Array, Uint8Array,validSampleSource,
    AudioWorkletProcessor: class { constructor() { this.port = { postMessage: message => messages.push(message) }; } },
    registerProcessor: (_name, processor) => { Processor = processor; },
  });
  const processor = new Processor({ processorOptions: { module, gain: 0.3 } });
  processor.port.onmessage({ data: { type: "note", timbre: 0, note: 69, velocity: 100 } });
  let frames = 0, crossings = 0, previous = 0, workletPeak = 0;
  for (let block = 0; frames < sampleRate; block++) {
    const size = [64, 128, 256][block % 3];
    const left = new Float32Array(size), right = new Float32Array(size);
    assert.equal(processor.process([], [[left, right]]), true);
    for (let i = 0; i < size; i++) {
      assert.ok(Number.isFinite(left[i]) && Number.isFinite(right[i]));
      workletPeak = Math.max(workletPeak, Math.abs(left[i]), Math.abs(right[i]));
      if (previous <= 0 && left[i] > 0) crossings++;
      previous = left[i];
    }
    frames += size;
  }
  assert.equal(processor.failed, false);
  assert.ok(workletPeak > 0.001);
  const hz = crossings / (frames / sampleRate);
  assert.ok(Math.abs(hz - 440) < 3, `A4 tuning at ${sampleRate} Hz: ${hz}`);
  const nativeFrames = processor.wasm.rustias_frames();
  assert.ok(Math.abs(nativeFrames - frames * 48000 / sampleRate) <= 128, "Resampling must retain the native clock");
  const workletCircuit=defaultCircuit();workletCircuit.enabled=true;workletCircuit.wires=workletCircuit.wires.filter(w=>w.to!==15);
  processor.port.onmessage({data:{type:'circuit',timbre:0,circuit:audioCircuit(workletCircuit)}});
  const mutedLeft=new Float32Array(512),mutedRight=new Float32Array(512);processor.process([],[[mutedLeft,mutedRight]]);
  assert.ok([...mutedLeft.slice(256),...mutedRight.slice(256)].every(v=>Math.abs(v)<.000001),'AudioWorklet applies live graph disconnection after its buffered samples');
  workletCircuit.enabled=false;processor.port.onmessage({data:{type:'circuit',timbre:0,circuit:audioCircuit(workletCircuit)}});
  assert.equal(processor.failed,false,'Modular messages remain healthy at both device rates');
  processor.process([],[[new Float32Array(64),new Float32Array(64)]]);
  const beforeGrowth=processor.wasm.memory.buffer;
  processor.port.onmessage({data:{type:'library-sample',asset:100,request:78,data:new Float32Array(1440000).fill(.01)}});
  assert.notEqual(processor.wasm.memory.buffer,beforeGrowth,'A large sample exercises actual Wasm memory growth during a partial audio block');
  const grownLeft=new Float32Array(128),grownRight=new Float32Array(128);processor.process([],[[grownLeft,grownRight]]);
  assert.ok([...grownLeft,...grownRight].every(Number.isFinite),'Sample uploads preserve valid audio across memory growth at both output rates');
  processor.port.onmessage({data:{type:"audition",timbre:1,step:{notes:[60,64,67],velocity:100,gate:75}}});assert.equal(processor.wasm.rustias_voices(),4,"Audition sounds the complete chord on its edited timbre");
  for(let block=0;block<100;block++)processor.process([],[[new Float32Array(128),new Float32Array(128)]]);assert.equal(processor.wasm.rustias_voices(),1,"Audition preserves the manually held note");
  processor.port.onmessage({data:{type:'library-sample',asset:99,request:77,data:sine.slice()}});
  processor.port.onmessage({data:{type:'library-profile',id:321,asset:99,timbre:3,source:'909:bt7a0d7',mode:2,values:libraryValues}});
  assert.ok(messages.some(m=>m.type==='sample-ready'&&m.request===77&&m.ok));
  processor.port.onmessage({data:{type:'audition',timbre:3,step:{notes:[72],samples:['909:bt7a0d7'],velocity:100,gate:75}}});assert.equal(processor.wasm.rustias_voices(),3,'Worklet audition combines melodic notes and arbitrary samples');
  for(let block=0;block<150;block++)processor.process([],[[new Float32Array(128),new Float32Array(128)]]);assert.equal(processor.wasm.rustias_voices(),1,'Worklet releases the direct sample and chord without dropping the manual note');
  const directLane=emptySequence();directLane.tracks.forEach((track,t)=>{track.enabled=t===3;});directLane.tracks[3].steps[0].samples=['909:bt7a0d7'];
  processor.port.onmessage({data:{type:'sequencer',config:directLane}});processor.port.onmessage({data:{type:'sequence-play'}});processor.process([],[[new Float32Array(128),new Float32Array(128)]]);assert.equal(processor.wasm.rustias_voices(),2,'Worklet clock triggers arbitrary samples');processor.port.onmessage({data:{type:'sequence-stop'}});
  for(let block=0;block<150;block++)processor.process([],[[new Float32Array(128),new Float32Array(128)]]);assert.equal(processor.wasm.rustias_voices(),1);
  const lane=emptySequence();lane.tracks[0].steps[0].notes=[69];lane.tracks.slice(1).forEach(t=>t.enabled=false);
  processor.port.onmessage({data:{type:"sequencer",config:lane}});processor.port.onmessage({data:{type:"sequence-play"}});processor.process([],[[new Float32Array(128),new Float32Array(128)]]);
  processor.port.onmessage({data:{type:"sequence-stop"}});assert.equal(processor.wasm.rustias_voices(),1,"Sequence Stop must preserve the manual note");
  processor.port.onmessage({ data: { type: "stop" } });
  for (let block = 0; block < 3; block++) processor.process([], [[new Float32Array(128), new Float32Array(128)]]);
  assert.equal(processor.wasm.rustias_voices(), 0);
  assert.ok(!messages.some(message => message.type === "error"));
  reports.push({ sampleRate, outputFrames: frames, nativeFrames, measuredA4Hz: Number(hz.toFixed(2)), peak: workletPeak });
}
features.push(verifyPrograms(schema,module));
console.log(JSON.stringify({ passed: true, wasmBytes: fs.statSync(path).size, runtimeImports: [], nativePeak: peak, parameters: schema.length, features, audioWorklet: reports }, null, 2));
