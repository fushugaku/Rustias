// Capture the actual device-rate stereo output without interrupting rendering.
export class RecordingTap {
  constructor(post, rate, chunkFrames = 16384) {
    this.post = post; this.rate = rate; this.chunkFrames = chunkFrames; this.id = null;
  }
  start(id) {
    if (this.id || typeof id !== 'string') return;
    this.id = id; this.frames = 0; this.index = 0; this.used = 0;
    this.buffer = new Int16Array(this.chunkFrames * 2);
    this.post({type: 'record-started', id, sampleRate: this.rate});
  }
  capture(left, right) {
    if (!this.id) return;
    for (let i = 0; i < left.length; i++) {
      const l = Number.isFinite(left[i]) ? Math.max(-1, Math.min(1, left[i])) : 0;
      const r = Number.isFinite(right[i]) ? Math.max(-1, Math.min(1, right[i])) : 0;
      this.buffer[this.used++] = Math.round(l * (l < 0 ? 32768 : 32767));
      this.buffer[this.used++] = Math.round(r * (r < 0 ? 32768 : 32767));
      this.frames++;
      if (this.used === this.buffer.length) this.flush();
    }
  }
  flush() {
    if (!this.used) return;
    const pcm = this.used === this.buffer.length ? this.buffer : this.buffer.slice(0, this.used);
    this.post({type: 'record-chunk', id: this.id, index: this.index++, frames: this.frames, pcm}, [pcm.buffer]);
    this.buffer = new Int16Array(this.chunkFrames * 2); this.used = 0;
  }
  stop(id = this.id) {
    if (!this.id || id !== this.id) return;
    this.flush();
    this.post({type: 'record-stopped', id: this.id, frames: this.frames});
    this.id = null; this.buffer = null;
  }
}
