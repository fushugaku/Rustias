import fs from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";

const path = process.argv[2] ?? "dist/rustias.wasm";
const module = await WebAssembly.compile(fs.readFileSync(path));
assert.deepEqual(WebAssembly.Module.imports(module), [], "Standalone Wasm must have no runtime imports");
const api = new WebAssembly.Instance(module, {}).exports;
api.rustias_init();
assert.equal(api.rustias_note(4, 69, 100), 0);
assert.equal(api.rustias_control(0, 0, 4), 0);
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

const workletSource = fs.readFileSync(new URL("../web/worklet.js", import.meta.url), "utf8");
const reports = [];
for (const sampleRate of [48000, 44100]) {
  let Processor;
  const messages = [];
  vm.runInNewContext(workletSource, {
    sampleRate, WebAssembly, Float32Array,
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
console.log(JSON.stringify({ passed: true, wasmBytes: fs.statSync(path).size, runtimeImports: [], nativePeak: peak, audioWorklet: reports }, null, 2));
