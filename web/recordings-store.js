// Audio never goes into localStorage: metadata and ordered PCM chunks are atomic.
export class RecordingStore {
  async open() {
    if (!this.database) this.database = new Promise((resolve, reject) => {
      const request = indexedDB.open('rustias-recordings', 1);
      request.onupgradeneeded = () => {
        const db = request.result;
        db.createObjectStore('folders', {keyPath: 'id'}).createIndex('program', 'programKey', {unique: true});
        db.createObjectStore('recordings', {keyPath: 'id'}).createIndex('folder', 'folderId');
        db.createObjectStore('chunks', {keyPath: ['recordingId', 'index']});
      };
      request.onerror = () => { this.database = null; reject(request.error); };
      request.onsuccess = () => {
        request.result.onversionchange = () => { request.result.close(); this.database = null; };
        resolve(request.result);
      };
    });
    return this.database;
  }
  async transaction(names, mode, work) {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(names, mode), stores = Object.fromEntries(names.map(n => [n, tx.objectStore(n)]));
      let result, failure;
      tx.oncomplete = () => resolve(result);
      tx.onabort = tx.onerror = () => reject(failure ?? tx.error ?? new Error('Could not store the recording.'));
      const abort = error => { failure = error; tx.abort(); };
      try { work(stores, value => { result = value; }, abort); } catch (error) { abort(error); }
    });
  }
  async list() {
    return this.transaction(['folders', 'recordings'], 'readonly', (s, done) => {
      const result = {folders: [], recordings: []};
      s.folders.getAll().onsuccess = e => { result.folders = e.target.result; };
      s.recordings.getAll().onsuccess = e => { result.recordings = e.target.result; };
      done(result);
    });
  }
  async folder(program) {
    return this.transaction(['folders'], 'readwrite', (s, done) => {
      s.folders.index('program').get(program.key).onsuccess = e => {
        const folder = e.target.result ?? {id: crypto.randomUUID(), programKey: program.key, name: program.name, createdAt: Date.now(), nextTake: 1};
        if (!e.target.result) s.folders.add(folder);
        done(folder);
      };
    });
  }
  async createFolder(name) {
    const folder = {id: crypto.randomUUID(), name: name.trim().slice(0, 64) || 'Untitled', createdAt: Date.now(), nextTake: 1};
    return this.transaction(['folders'], 'readwrite', (s, done) => { s.folders.add(folder); done(folder); });
  }
  async begin(folderId, sampleRate) {
    return this.transaction(['folders', 'recordings'], 'readwrite', (s, done, abort) => {
      s.folders.get(folderId).onsuccess = e => {
        const folder = e.target.result;
        if (!folder) { abort(new Error('Choose a recording folder.')); return; }
        const recording = {id: crypto.randomUUID(), folderId, name: `Recording ${folder.nextTake++}`, createdAt: Date.now(), sampleRate, frames: 0, chunks: 0, status: 'recording'};
        s.folders.put(folder); s.recordings.add(recording); done(recording);
      };
    });
  }
  async append(id, index, pcm) {
    return this.transaction(['recordings', 'chunks'], 'readwrite', (s, done, abort) => {
      s.recordings.get(id).onsuccess = e => {
        const recording = e.target.result;
        if (!recording || recording.status !== 'recording' || index !== recording.chunks || !(pcm instanceof Int16Array) || pcm.length % 2) { abort(new Error('Recording chunks arrived out of order.')); return; }
        s.chunks.add({recordingId: id, index, pcm}); recording.chunks++; recording.frames += pcm.length / 2;
        s.recordings.put(recording); done(recording);
      };
    });
  }
  async finish(id) {
    return this.transaction(['recordings'], 'readwrite', (s, done) => {
      s.recordings.get(id).onsuccess = e => {
        const recording = e.target.result;
        if (recording) { recording.status = 'ready'; s.recordings.put(recording); }
        done(recording);
      };
    });
  }
  async update(kind, id, changes) {
    return this.transaction([kind], 'readwrite', (s, done, abort) => {
      s[kind].get(id).onsuccess = e => {
        if (!e.target.result) { abort(new Error('The recording no longer exists.')); return; }
        const value = {...e.target.result, ...changes}; s[kind].put(value); done(value);
      };
    });
  }
  async remove(id) {
    return this.transaction(['recordings', 'chunks'], 'readwrite', s => {
      s.recordings.delete(id); s.chunks.delete(IDBKeyRange.bound([id, 0], [id, Number.MAX_SAFE_INTEGER]));
    });
  }
  async read(id, onChunk) {
    return this.transaction(['recordings', 'chunks'], 'readonly', (s, done, abort) => {
      s.recordings.get(id).onsuccess = e => {
        const recording = e.target.result;
        if (!recording?.frames) { abort(new Error('This recording has no audio yet.')); return; }
        let index = 0, frames = 0;
        s.chunks.openCursor(IDBKeyRange.bound([id, 0], [id, Number.MAX_SAFE_INTEGER])).onsuccess = event => {
          const cursor = event.target.result;
          if (!cursor) {
            if (frames !== recording.frames || index !== recording.chunks) { abort(new Error('This recording is incomplete.')); return; }
            done(recording); return;
          }
          try {
            const chunk = cursor.value;
            if (chunk.index !== index++) throw new Error('A recording chunk is missing.');
            frames += chunk.pcm.length / 2; onChunk(chunk.pcm, recording, frames);
            cursor.continue();
          } catch (error) { abort(error); }
        };
      };
    });
  }
}
