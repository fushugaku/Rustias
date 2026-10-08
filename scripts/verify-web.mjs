import fs from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";

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
for(let shaper=1;shaper<13;shaper++){
  api.rustias_init();control(29,shaper);control(31,70);api.rustias_note(0,60,100);
  assert.ok(rms(render())>0.0001,`Waveshaper ${shaper}`);
}
features.push("all filter routes, Filter2 + Comb, 12 waveshaper types");
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

const workletSource = fs.readFileSync(new URL("../web/worklet.js", import.meta.url), "utf8");
const reports = [];
for (const sampleRate of [48000, 44100]) {
  let Processor;
  const messages = [];
  vm.runInNewContext(workletSource, {
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
  processor.port.onmessage({ data: { type: "stop" } });
  for (let block = 0; block < 3; block++) processor.process([], [[new Float32Array(128), new Float32Array(128)]]);
  assert.equal(processor.wasm.rustias_voices(), 0);
  assert.ok(!messages.some(message => message.type === "error"));
  reports.push({ sampleRate, outputFrames: frames, nativeFrames, measuredA4Hz: Number(hz.toFixed(2)), peak: workletPeak });
}
console.log(JSON.stringify({ passed: true, wasmBytes: fs.statSync(path).size, runtimeImports: [], nativePeak: peak, parameters: schema.length, features, audioWorklet: reports }, null, 2));
