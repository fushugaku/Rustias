// A dedicated writer keeps disk I/O off the instrument UI. Exports use another
// instance of this worker, so MP3 encoding cannot stall an ongoing recording.
const modules = Promise.all([import('./recordings-store.js'), import('./recording-format.js')]);
const storage = modules.then(([{RecordingStore}]) => new RecordingStore());
let queue = Promise.resolve();
onmessage = ({data}) => {
  queue = queue.then(async () => {
    try {
      const [, {wavHeader}] = await modules;
      const store = await storage; let result;
      if (data.type === 'begin') result = await store.begin(data.folderId, data.sampleRate);
      else if (data.type === 'append') result = await store.append(data.id, data.index, data.pcm);
      else if (data.type === 'finish') result = await store.finish(data.id);
      else if (data.type === 'export') {
        const parts = []; let encoder, previous = 0;
        if (data.format === 'mp3') importScripts('./vendor/lamejs/lame.all.js');
        const recording = await store.read(data.id, (pcm, info, frames) => {
          if (data.format === 'wav') parts.push(pcm);
          else {
            encoder ??= new lamejs.Mp3Encoder(2, info.sampleRate, 320);
            const left = new Int16Array(1152), right = new Int16Array(1152);
            for (let offset = 0; offset < pcm.length / 2; offset += 1152) {
              const length = Math.min(1152, pcm.length / 2 - offset);
              for (let i = 0; i < length; i++) { left[i] = pcm[(offset + i) * 2]; right[i] = pcm[(offset + i) * 2 + 1]; }
              const bytes = encoder.encodeBuffer(left.subarray(0, length), right.subarray(0, length));
              if (bytes.length) parts.push(new Int8Array(bytes));
            }
          }
          const progress = Math.floor(frames / info.frames * 100);
          if (progress >= previous + 5) { previous = progress; postMessage({request: data.request, progress}); }
        });
        if (encoder) parts.push(new Int8Array(encoder.flush()));
        else parts.unshift(wavHeader(recording.frames, recording.sampleRate));
        result = new Blob(parts, {type: encoder ? 'audio/mpeg' : 'audio/wav'});
      } else throw new Error('Unknown recording operation.');
      postMessage({request: data.request, result});
    } catch (error) { postMessage({request: data.request, error: error.message}); }
  });
};
