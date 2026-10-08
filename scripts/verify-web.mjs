import fs from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {SequenceClock,StepAudition,emptySequence,validateSequence,RESOLUTIONS,stepFrames} from "../web/sequence.js";
import {PatchStore} from "../web/patches.js";

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

const manifest=JSON.parse(fs.readFileSync(new URL("../web/samples/manifest.json",import.meta.url)));
assert.equal(manifest.samples.length,64);assert.equal(manifest.license,"CC0-1.0");const hashes=new Set();
for(const sample of manifest.samples){const data=fs.readFileSync(new URL(`../web/samples/${sample.file}`,import.meta.url));assert.equal(data.toString("ascii",0,4),"RIFF");assert.equal(data.toString("ascii",8,12),"WAVE");assert.ok(sample.duration>0);assert.equal(createHash("sha256").update(data).digest("hex"),sample.sha256);hashes.add(sample.sha256);}
assert.equal(hashes.size,64,"Bundled recordings must be distinct");
features.push("64 distinct CC0 WAVs with source revision, license and verified checksums");

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
const heard=[];const audition=new StepAudition((t,n,v)=>heard.push({t,n,v}));audition.play(2,{notes:[60,64,67],velocity:113,gate:80},120);assert.deepEqual(heard.map(e=>e.n),[60,64,67]);assert.ok(heard.every(e=>e.t===2&&e.v===113));for(let i=0;i<50;i++)audition.beforeRender();assert.equal(heard.filter(e=>!e.v).length,3,"Audition releases the whole chord");heard.length=0;audition.play(1,{notes:[62,65],velocity:87,gate:50},120);audition.play(1,{notes:[62,65,69],velocity:87,gate:50},120);assert.deepEqual(heard.filter(e=>e.v).map(e=>e.n),[62,65,62,65,69],"Every edit restarts all selected notes");audition.stop();
const fakeStorage=new Map();fakeStorage.getItem=fakeStorage.get.bind(fakeStorage);fakeStorage.setItem=fakeStorage.set.bind(fakeStorage);const patches=new PatchStore(fakeStorage);const snapshot={version:2,engine:program,sequencer:sequence,samples:{version:1,slots:Array.from({length:16},()=>({source:"synth",mode:0}))}};
const patch=patches.save("Four timbres",snapshot);assert.deepEqual(new PatchStore(fakeStorage).list()[0].snapshot,snapshot);patches.save("Renamed",snapshot,patch.id);assert.equal(patches.list().length,1);patches.saveSession({snapshot,selected:3,volume:45});assert.deepEqual(patches.session().snapshot,snapshot);
features.push("four synchronized polyphonic sequencers up to 128 steps, complete chord audition, per-track straight/triplet/dotted resolution, gate, mute, length, reset, tempo and browser patch persistence");

const sequenceSource = fs.readFileSync(new URL("../web/sequence.js", import.meta.url),"utf8").replace(/^export /gm, "");
const workletSource = fs.readFileSync(new URL("../web/worklet.js", import.meta.url), "utf8");
const reports = [];
for (const sampleRate of [48000, 44100]) {
  let Processor;
  const messages = [];
  vm.runInNewContext(sequenceSource + "\n" + workletSource.replace(/^import .*;\n/gm,""), {
    sampleRate, WebAssembly, Float32Array, Uint8Array,
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
  processor.port.onmessage({data:{type:"audition",timbre:1,step:{notes:[60,64,67],velocity:100,gate:75}}});assert.equal(processor.wasm.rustias_voices(),4,"Audition sounds the complete chord on its edited timbre");
  for(let block=0;block<100;block++)processor.process([],[[new Float32Array(128),new Float32Array(128)]]);assert.equal(processor.wasm.rustias_voices(),1,"Audition preserves the manually held note");
  const lane=emptySequence();lane.tracks[0].steps[0].notes=[69];lane.tracks.slice(1).forEach(t=>t.enabled=false);
  processor.port.onmessage({data:{type:"sequencer",config:lane}});processor.port.onmessage({data:{type:"sequence-play"}});processor.process([],[[new Float32Array(128),new Float32Array(128)]]);
  processor.port.onmessage({data:{type:"sequence-stop"}});assert.equal(processor.wasm.rustias_voices(),1,"Sequence Stop must preserve the manual note");
  processor.port.onmessage({ data: { type: "stop" } });
  for (let block = 0; block < 3; block++) processor.process([], [[new Float32Array(128), new Float32Array(128)]]);
  assert.equal(processor.wasm.rustias_voices(), 0);
  assert.ok(!messages.some(message => message.type === "error"));
  reports.push({ sampleRate, outputFrames: frames, nativeFrames, measuredA4Hz: Number(hz.toFixed(2)), peak: workletPeak });
}
console.log(JSON.stringify({ passed: true, wasmBytes: fs.statSync(path).size, runtimeImports: [], nativePeak: peak, parameters: schema.length, features, audioWorklet: reports }, null, 2));
