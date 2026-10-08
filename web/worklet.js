class RustiasProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.wasm = new WebAssembly.Instance(options.processorOptions.module, {}).exports;
    this.wasm.rustias_init();
    this.gain = options.processorOptions.gain ?? 0.3;
    this.nativeIndex = 128;
    this.phase = 0;
    this.currentLeft = this.currentRight = this.nextLeft = this.nextRight = 0;
    this.callbacks = 0;
    this.audibleFrames = 0;
    this.peak = 0;
    this.failed = false;
    this.port.onmessage = ({ data }) => {
      try {
        if (data.type === "note") this.wasm.rustias_note(data.timbre, data.note, data.velocity);
        else if (data.type === "control") this.wasm.rustias_control(data.timbre, data.parameter, data.value);
        else if (data.type === "midi") this.wasm.rustias_midi(...data.bytes);
        else if (data.type === "stop") this.wasm.rustias_stop();
        else if (data.type === "gain") this.gain = Math.max(0, Math.min(1, data.value));
      } catch (error) { this.fail(error); }
    };
    this.port.postMessage({ type: "ready", sampleRate });
  }
  fail(error) {
    this.failed = true;
    this.port.postMessage({ type: "error", message: error.message ?? String(error) });
  }
  nativeSample() {
    if (this.nativeIndex === 128) {
      const pointer = this.wasm.rustias_render();
      if (this.pointer !== pointer || this.block?.buffer !== this.wasm.memory.buffer) {
        this.pointer = pointer; this.block = new Float32Array(this.wasm.memory.buffer, pointer, 256);
      }
      this.nativeIndex = 0;
    }
    const index = this.nativeIndex++ * 2;
    this.nativeLeft = this.block[index]; this.nativeRight = this.block[index + 1];
  }
  process(_inputs, outputs) {
    const [left, right] = outputs[0];
    if (this.failed || !left || !right) return true;
    try {
      for (let i = 0; i < left.length; i++) {
        if (sampleRate === 48000) {
          this.nativeSample();
          left[i] = this.nativeLeft * this.gain;
          right[i] = this.nativeRight * this.gain;
        } else {
          const fraction = this.phase / sampleRate;
          left[i] = (this.currentLeft + (this.nextLeft - this.currentLeft) * fraction) * this.gain;
          right[i] = (this.currentRight + (this.nextRight - this.currentRight) * fraction) * this.gain;
          this.phase += 48000;
          while (this.phase >= sampleRate) {
            this.phase -= sampleRate;
            this.currentLeft = this.nextLeft; this.currentRight = this.nextRight;
            this.nativeSample();
            this.nextLeft = this.nativeLeft; this.nextRight = this.nativeRight;
          }
        }
      }
      for (let i = 0; i < left.length; i++) {
        this.peak = Math.max(this.peak, Math.abs(left[i]), Math.abs(right[i]));
        if (left[i] !== 0 || right[i] !== 0) this.audibleFrames++;
      }
      if (++this.callbacks % 20 === 0) {
        this.port.postMessage({ type: "stats", voices: this.wasm.rustias_voices(), frames: this.wasm.rustias_frames(), audibleFrames: this.audibleFrames, peak: this.peak, callbacks: this.callbacks });
        this.peak = 0;
      }
    } catch (error) { this.fail(error); }
    return true;
  }
}
registerProcessor("rustias", RustiasProcessor);
