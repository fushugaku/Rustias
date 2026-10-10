import {makePicker} from './panel.js';
import {RecordingStore} from './recordings-store.js';
import {recordingTime, recordingFilename} from './recording-format.js';

const $ = selector => document.querySelector(selector);
function workerClient(onProgress = () => {}) {
  const worker = new Worker(new URL('./recordings-worker.js', import.meta.url));
  const pending = new Map(); let next = 0, failure;
  worker.onmessage = ({data}) => {
    if (data.progress != null) { onProgress(data.progress); return; }
    const request = pending.get(data.request); if (!request) return;
    pending.delete(data.request);
    if (data.error) request.reject(new Error(data.error)); else request.resolve(data.result);
  };
  worker.onerror = () => {
    failure = new Error('The recording worker could not run.');
    for (const request of pending.values()) request.reject(failure);
    pending.clear();
  };
  return {
    request(data, transfer = []) {
      return new Promise((resolve, reject) => {
        if (failure) { reject(failure); return; }
        const request = ++next; pending.set(request, {resolve, reject});
        try { worker.postMessage({...data, request}, transfer); } catch (error) { pending.delete(request); reject(error); }
      });
    },
    close() { worker.terminate(); }
  };
}

export function createRecorder({ensureAudio, getAudio, getProgram, send, onError}) {
  const store = new RecordingStore(), writer = workerClient();
  const library = $('#recordings-library'), recordButton = $('#record-toggle'), timer = $('#record-time');
  let ready = false, phase = 'idle', active = null, starting, stopping, failed = false;
  let folders = [], recordings = [], folderId, programKey, programTask, programRequest = 0, playerUrl, player, previewRequest = 0;
  const downloadUrls = new Set();
  const movers = [];
  const replies = new Map(), closed = new Set();
  const folderPicker = makePicker({label: 'Recording folder', options: [], value: '', searchable: true, searchLabel: 'Search folders', onChange: id => { folderId = id; updateFolder(); }});
  $('#record-folder').append(folderPicker.button);

  function controls() {
    recordButton.disabled = !ready || phase === 'starting' || phase === 'saving';
    recordButton.textContent = phase === 'starting' ? 'Starting…' : phase === 'saving' ? 'Saving…' : phase === 'recording' ? 'Stop recording' : 'Record';
    recordButton.setAttribute('aria-pressed', phase === 'recording');
    folderPicker.button.disabled = phase !== 'idle';
    $('#recorder').classList.toggle('is-recording', phase === 'recording');
    document.body.dataset.recording = phase;
  }
  function updateFolder() {
    folderPicker.options = folders.map(f => ({value: f.id, label: f.name}));
    const target = active?.folderId ?? folderId;
    folderPicker.render(target ?? '', folders.find(f => f.id === target)?.name ?? getProgram().name);
  }
  async function refresh(render = true) {
    ({folders, recordings} = await store.list());
    folders.sort((a, b) => a.createdAt - b.createdAt); recordings.sort((a, b) => a.createdAt - b.createdAt);
    updateFolder();
    for (const {picker, recording} of movers) { picker.options = folders.map(f => ({value:f.id,label:f.name})); picker.render(recording.folderId, 'Move'); }
    $('#recordings-toggle').textContent = `Recordings${recordings.length ? ` · ${recordings.length}` : ''}`;
    if (render && !library.hidden) renderLibrary();
  }
  function programChanged() {
    const program = getProgram(); if (program.key === programKey) return programTask;
    programKey = program.key; const request = ++programRequest;
    programTask = (async () => {
      try {
        const folder = await store.folder(program);
        if (request !== programRequest) return;
        folderId = folder.id; await refresh();
      } catch (error) { if (request === programRequest) programKey = undefined; onError(error); }
    })();
    return programTask;
  }
  function audioReply(type, id, timeout) {
    return new Promise((resolve, reject) => {
      const key = `${type}:${id}`, clock = setTimeout(() => { replies.delete(key); reject(new Error('The audio engine did not respond to the recorder.')); }, timeout);
      replies.set(key, () => { clearTimeout(clock); replies.delete(key); resolve(); });
    });
  }
  function start() {
    if (starting || phase !== 'idle') return starting;
    starting = (async () => {
      phase = 'starting'; failed = false; controls();
      try {
        await ensureAudio();
        await programChanged();
        // Program selection and folder creation can still be settling at startup.
        if (!folderId) { const folder = await store.folder(getProgram()); folderId = folder.id; }
        active = await writer.request({type: 'begin', folderId, sampleRate: getAudio().sampleRate});
        timer.textContent = '0:00';
        const ack = audioReply('record-started', active.id, 5000);
        send({type: 'record-start', id: active.id}); await ack;
        phase = 'recording'; controls(); $('#record-message').textContent = `Recording to ${folders.find(f => f.id === active.folderId)?.name ?? getProgram().name}`;
        await refresh();
      } catch (error) {
        if (active) {
          send({type: 'record-stop', id: active.id});
          await writer.request({type: 'finish', id: active.id}).catch(() => {});
          if (!active.frames) await store.remove(active.id).catch(() => {});
        }
        active = null; phase = 'idle'; controls(); onError(error);
      } finally { starting = null; }
    })();
    return starting;
  }
  function stop() {
    if (starting) return starting.then(stop);
    if (stopping) return stopping;
    if (!active) return Promise.resolve();
    stopping = (async () => {
      phase = 'saving'; controls(); const id = active.id;
      try {
        const ack = audioReply('record-stopped', id, 1500);
        send({type: 'record-stop', id});
        // Already committed chunks remain usable if audio was interrupted.
        await ack.catch(() => {});
        const saved = await writer.request({type: 'finish', id});
        if (saved) timer.textContent = recordingTime(saved.frames / saved.sampleRate);
        $('#record-message').textContent = saved?.frames ? `${saved.name} saved` : 'Empty recording saved';
      } catch (error) { onError(error); }
      finally { active = null; phase = 'idle'; stopping = null; controls(); await refresh().catch(onError); }
    })();
    return stopping;
  }
  function handleMessage(data) {
    if (!data.type?.startsWith('record-')) return false;
    if (data.id !== active?.id) return true;
    if (data.type === 'record-chunk' && !failed) {
      active.frames = data.frames; timer.textContent = recordingTime(data.frames / active.sampleRate);
      const metadata = library.querySelector(`[data-recording-id="${data.id}"] .recording-meta`);
      if (metadata) metadata.textContent = takeDetails(active, true);
      writer.request({type: 'append', id: data.id, index: data.index, pcm: data.pcm}, [data.pcm.buffer]).catch(error => {
        if (failed || active?.id !== data.id) return;
        failed = true; onError(new Error(`Recording stopped: ${error.message}. Previously saved audio is kept.`)); stop();
      });
    } else replies.get(`${data.type}:${data.id}`)?.();
    return true;
  }
  async function audioFile(recording, format, onProgress) {
    const exporter = workerClient(onProgress);
    try { return await exporter.request({type: 'export', id: recording.id, format}); }
    finally { exporter.close(); }
  }
  function stopPlayer() {
    previewRequest++;
    if (player) { player.pause(); player.removeAttribute('src'); player.load(); player.hidden = true; player = null; }
    if (playerUrl) URL.revokeObjectURL(playerUrl); playerUrl = null;
  }
  function clearDownloads() { for (const url of downloadUrls) URL.revokeObjectURL(url); downloadUrls.clear(); }
  function report(promise) { promise.catch(onError); }
  function takeDetails(recording, live) {
    return `${recordingTime(recording.frames / recording.sampleRate)} · ${recording.sampleRate / 1000} kHz · ${new Date(recording.createdAt).toLocaleString()}${live ? ' · Recording' : recording.status === 'recording' ? ' · Recovered' : ''}`;
  }
  function editName(input, kind, value) {
    let last = value.name, clock;
    const save = () => {
      clearTimeout(clock); const name = input.value.trim().slice(0, 64);
      if (!name || name === last) return;
      last = name;
      report(store.update(kind, value.id, {name}).then(saved => { Object.assign(value, saved); return refresh(false); }).catch(error => { last = value.name; throw error; }));
    };
    input.addEventListener('input', () => { clearTimeout(clock); clock = setTimeout(save, 250); });
    input.addEventListener('change', save); input.addEventListener('blur', save);
    input.addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); save(); input.blur(); } });
  }
  function renderLibrary() {
    stopPlayer(); clearDownloads(); movers.length = 0; const list = $('#recording-folders'); list.replaceChildren();
    $('#recordings-empty').hidden = recordings.length > 0;
    for (const folder of folders) {
      const takes = recordings.filter(r => r.folderId === folder.id);
      if (!takes.length && folder.programKey && folder.id !== folderId) continue;
      const details = document.createElement('details');
      details.className = 'recording-folder'; details.dataset.folderId = folder.id; details.open = !closed.has(folder.id);
      details.addEventListener('toggle', () => { if (details.open) closed.delete(folder.id); else closed.add(folder.id); });
      const summary = document.createElement('summary'), name = document.createElement('input'), count = document.createElement('span');
      name.type = 'text'; name.value = folder.name; name.maxLength = 64; name.setAttribute('aria-label', 'Folder name');
      name.addEventListener('click', e => e.stopPropagation());
      editName(name, 'folders', folder);
      count.textContent = String(takes.length); summary.append(name, count); details.append(summary);
      for (const recording of takes) {
        const row = document.createElement('div'); row.className = 'recording-row'; row.dataset.recordingId = recording.id;
        row.innerHTML = '<div class="recording-info"><input type="text" maxlength="64" aria-label="Recording name"><span class="recording-meta"></span></div><div class="recording-actions"><button class="record-listen">Listen</button><a class="record-wav" href="#" role="button">WAV</a><a class="record-mp3" href="#" role="button">MP3</a><div class="record-move"></div><button class="record-delete" aria-label="Delete recording">×</button></div><audio controls preload="none" hidden></audio>';
        const input = row.querySelector('input'), metadata = row.querySelector('.recording-meta'); input.value = recording.name;
        editName(input, 'recordings', recording);
        const live = recording.id === active?.id;
        metadata.textContent = takeDetails(recording, live);
        const buttons = [...row.querySelectorAll('button')]; for (const button of buttons) button.disabled = live || !recording.frames;
        row.querySelector('.record-delete').disabled = live;
        row.querySelector('.record-delete').addEventListener('click', () => report(store.remove(recording.id).then(() => refresh())));
        const mover = makePicker({label: 'Move recording to folder', options: folders.map(f => ({value: f.id, label: f.name})), value: folder.id, onChange: id => report(store.update('recordings', recording.id, {folderId: id}).then(() => refresh()))});
        mover.button.title = 'Move to folder'; mover.render(folder.id, 'Move'); mover.button.disabled = live; row.querySelector('.record-move').append(mover.button);
        movers.push({picker:mover,recording});
        for (const format of ['wav', 'mp3']) {
          const button = row.querySelector(`.record-${format}`); let fileUrl;
          button.setAttribute('aria-disabled', live || !recording.frames);
          if (live || !recording.frames) button.tabIndex = -1;
          button.addEventListener('click', async event => {
            if (fileUrl) { button.download = recordingFilename(folder.name, input.value, format); return; }
            event.preventDefault(); if (button.getAttribute('aria-disabled') === 'true') return;
            button.setAttribute('aria-disabled', 'true');
            try {
              const blob = await audioFile(recording, format, progress => { button.textContent = `${progress}%`; });
              if (!row.isConnected || library.hidden) return;
              fileUrl = URL.createObjectURL(blob); downloadUrls.add(fileUrl); button.href = fileUrl;
              button.download = recordingFilename(folder.name, input.value, format);
              button.setAttribute('aria-disabled', 'false'); button.click();
            } catch (error) { onError(error); }
            finally { button.setAttribute('aria-disabled', 'false'); button.textContent = format.toUpperCase() + (fileUrl ? ' ↓' : ''); }
          });
        }
        const listen = row.querySelector('.record-listen'), audio = row.querySelector('audio');
        listen.addEventListener('click', async () => {
          if (player === audio) { stopPlayer(); listen.textContent = 'Listen'; return; }
          listen.disabled = true;
          try {
            stopPlayer(); library.querySelectorAll('.record-listen').forEach(b => { b.textContent = 'Listen'; });
            const request = previewRequest, blob = await audioFile(recording, 'wav');
            if (request !== previewRequest || !row.isConnected || library.hidden) return;
            playerUrl = URL.createObjectURL(blob);
            player = audio; audio.src = playerUrl; audio.hidden = false; listen.textContent = 'Close player';
            await audio.play().catch(() => {});
          } catch (error) { onError(error); }
          finally { listen.disabled = false; }
        });
        details.append(row);
      }
      list.append(details);
    }
  }
  recordButton.addEventListener('click', () => { if (phase === 'recording') stop(); else start(); });
  $('#recordings-toggle').addEventListener('click', async () => {
    library.hidden = !library.hidden; $('#recordings-toggle').setAttribute('aria-expanded', !library.hidden);
    if (library.hidden) { stopPlayer(); clearDownloads(); } else { await refresh().catch(onError); library.scrollIntoView({block:'start'}); }
  });
  $('#recordings-close').addEventListener('click', () => { library.hidden = true; $('#recordings-toggle').setAttribute('aria-expanded', 'false'); stopPlayer(); clearDownloads(); });
  $('#record-folder-add').addEventListener('click', () => { $('#record-folder-form').hidden = false; $('#record-folder-name').focus(); });
  $('#record-folder-cancel').addEventListener('click', () => { $('#record-folder-form').hidden = true; });
  $('#record-folder-form').addEventListener('submit', event => {
    event.preventDefault(); report(store.createFolder($('#record-folder-name').value).then(async folder => { folderId = folder.id; $('#record-folder-form').hidden = true; $('#record-folder-name').value = ''; await refresh(); }));
  });
  window.addEventListener('pagehide', () => { stop(); stopPlayer(); clearDownloads(); });
  controls(); programChanged();
  return {setReady(value) { ready = value; controls(); }, handleMessage, stop, programChanged};
}
