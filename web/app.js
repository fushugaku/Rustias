import {createDrumSamples} from "./samples.js";
import {createSequencer} from "./sequencer-ui.js";
import {emptySequence,validateSequence} from "./sequence.js";
import {PatchStore} from "./patches.js";
import {createPanel,makePicker,isChoosing} from "./panel.js";
const $ = selector => document.querySelector(selector);
function showError(error) { $("#error").textContent = error.message ?? String(error); $("#error").hidden = false; }
let parameters;
try {
  const response = await fetch(new URL("./parameters.json", import.meta.url));
  if (!response.ok) throw new Error(`Could not load the parameter definitions (${response.status}).`);
  parameters = await response.json();
} catch (error) { showError(error); throw error; }
const defaults = parameters.map(p => p.default);
const timbres = Array.from({length: 4}, (_, i) => ({values: defaults.map((v, id) => id === 72 ? i : v), preset: "init"}));
const drums = Array.from({length: 16}, (_, i) => defaults.map((v, id) => ({3: 0, 4: 32, 5: 0, 6: 20, 146: 60 + i})[id] ?? v));
const presets = {
  init: {}, pad: {0: 2, 1: 78, 2: 35, 3: 80, 4: 64, 5: 108, 6: 78, 7: 100, 67: 1, 68: 2, 69: 18, 70: 75},
  pulse: {0: 1, 1: 66, 2: 55, 3: 0, 4: 44, 5: 88, 6: 24, 7: 110, 11: 40, 62: 0},
  pluck: {0: 3, 1: 107, 2: 15, 3: 0, 4: 40, 5: 0, 6: 35, 7: 110},
};
const names = ["Saw", "Pulse", "Triangle", "Sine", "Noise", "Formant"];
const keyNames = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const computerKeys = ["a", "w", "s", "e", "d", "f", "t", "g", "y", "h", "u", "j", "k", "o", "l", "p"];
let selected = 0, octave = 4, context, node, module, midiAccess, audioStarting;
const held = new Map(), noteCounts = new Map();
let sampleUI;
let panel, programPicker, sequenceUI, activeSavedPatch=null, autosaveTimer, sequenceRequest=0, auditionRequest=0;
const patchStore=new PatchStore(window.localStorage);
const send = message => node?.port.postMessage(message);
const global = id => timbres[0].values[id];
const isDrum = () => global(140) !== 0 && global(141) === selected;
const instrumentParameter = id => parameters[id].scope !== "global" && !(id >= 59 && id <= 72 || id >= 114 && id <= 120 || id >= 137 && id <= 139 || id >= 150 && id <= 151 || id === 153);
const values = () => parameters.map(p => p.id === 118 ? (timbres[selected].values[67] ? timbres[selected].values[68] - 1 : 0) : isDrum() && instrumentParameter(p.id) ? drums[global(142)][p.id] : timbres[selected].values[p.id]);
function format(id, value) {
  const p = parameters[id];
  if (p.options) return p.options[(p.values ?? p.options.map((_, i) => p.min + i)).indexOf(value)];
  if ([1, 22].includes(id)) { const hz = 30 * 200 ** (value / 127); return hz >= 1000 ? `${(hz / 1000).toFixed(1)} kHz` : `${Math.round(hz)} Hz`; }
  if ([3, 4, 6, 32, 33, 35, 36, 37, 39, 59].includes(id)) {
    if (id === 59 && value === 0) return "Off";
    const ms = 3 * 2000 ** (value / 127); return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`;
  }
  if ([8, 144].includes(id)) return value === 64 ? "Center" : `${value < 64 ? "L" : "R"} ${Math.abs(value - 64)}`;
  if ([72, 148, 141, 142].includes(id)) return String(value + 1);
  if (id === 89) return `${(value / 10).toFixed(1)} BPM`;
  if (id === 146 || [119, 120].includes(id)) return noteLabel(value);
  if ([75, 83].includes(id)) return `${(0.02 * 1500 ** (value / 127)).toFixed(2)} Hz`;
  if (p.center !== undefined) return `${value - p.center > 0 ? "+" : ""}${value - p.center}`;
  return String(value);
}
function disabled(id, v) {
  if(isDrum()&&sampleUI?.assigned(global(142))&&[0,10,11,12,13,14,15,16,17,18,19].includes(id))return true;
  if (id === 10) return v[0] >= 4;
  if (id === 154) return v[29] !== 2;
  if ([21, 22, 23, 24, 27, 28].includes(id)) return v[20] === 0;
  if ([64, 63].includes(id)) return v[62] === 1;
  if ([68, 69, 70].includes(id)) return v[67] === 0;
  if (id === 115) return v[152] === 1;
  if ([114, 116, 117, 118].includes(id) && isDrum()) return true;
  if (id === 117) return v[116] === 0;
  if (id >= 125 && id <= 136) return v[122] !== 9;
  if ([75, 83].includes(id)) return v[id + 3] !== 0;
  if ([79, 87].includes(id)) return v[id - 1] === 0;
  if ([77, 85].includes(id)) return v[id - 1] === 0;
  if ([143, 144, 145].includes(id)) return v[140] === 0;
  if ([146, 147].includes(id)) return !isDrum();
  return false;
}
function setControl(id, value, fromUser = true) {
  const p = parameters[id]; value = Math.max(p.min, Math.min(p.max, Math.round(value)));
  const v = values(); if (disabled(id, v)) return;
  if (id === 119 && value > v[120] || id === 120 && value < v[119]) return;
  if (p.values && !p.values.includes(value)) return;
  if (p.scope === "global") for (const t of timbres) t.values[id] = value;
  else (isDrum() && instrumentParameter(id) ? drums[global(142)] : timbres[selected].values)[id] = value;
  if (id === 0 && value >= 4) (isDrum() ? drums[global(142)] : timbres[selected].values)[10] = 0;
  if ([66,137,138,139,150].includes(id)) {
    const channel = timbres[selected].values[72] === 16 ? global(148) : timbres[selected].values[72];
    for (const t of timbres) if ((t.values[72] === 16 ? global(148) : t.values[72]) === channel) t.values[id] = value;
  }
  if (fromUser) timbres[selected].preset = "custom";
  send({type: "control", timbre: selected, parameter: id, value}); updateControls(); updateKeys(); scheduleSession();
}
function updateControls() {
  const v = values();
  panel?.render(v); sampleUI?.render();
  $("#wave-display").setAttribute("aria-label", `${names[v[0]]} waveform`);
  const paths = ["M0 60 L60 12 L60 60 L120 12 L120 60 L180 12 L180 60 L240 12", "M0 60 V12 H30 V60 H60 V12 H90 V60 H120 V12 H150 V60 H180 V12 H210 V60 H240", "M0 36 L30 12 L60 36 L90 60 L120 36 L150 12 L180 36 L210 60 L240 36", `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 4 * Math.PI)}`).join(" L")}`, `M${Array.from({length: 61}, (_, i) => `${i * 4} ${12 + (i * 17 % 49)}`).join(" L")}`, `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 10 * Math.PI) * Math.sin(i / 120 * 2 * Math.PI)}`).join(" L")}`];
  $("#wave-path").setAttribute("d", paths[v[0]]);
  $("#edit-context").textContent = isDrum() ? `Drum ${String(global(142) + 1).padStart(2, "0")} · Timbre ${String(selected + 1).padStart(2, "0")}` : `Timbre ${String(selected + 1).padStart(2, "0")}`;
  programPicker?.render(activeSavedPatch?`saved:${activeSavedPatch}`:timbres[selected].preset);
}
function displayValue(p, value) { return p.id === 89 ? value / 10 : [72, 148, 141, 142].includes(p.id) ? value + 1 : value - (p.center ?? 0); }
function nativeValue(p, value) { return p.id === 89 ? value * 10 : [72, 148, 141, 142].includes(p.id) ? value - 1 : value + (p.center ?? 0); }
panel=createPanel({parameters,readValues:values,setControl,format,disabled,displayValue,nativeValue});
programPicker=makePicker({label:"Program",options:[{value:"init",label:"INIT"},{value:"pad",label:"Warm pad"},{value:"pulse",label:"Pulse bass"},{value:"pluck",label:"Soft pluck"},{value:"custom",label:"Custom"}],value:"init",onChange:applyPreset});
$("#preset").append(programPicker.button);
document.querySelectorAll("[data-timbre]").forEach(button => button.addEventListener("click", () => { selected = Number(button.dataset.timbre); document.querySelectorAll("[data-timbre]").forEach(tab => tab.setAttribute("aria-selected", tab === button)); updateControls(); updateKeys(); scheduleSession(); }));
function applyPreset(preset) {
  if(preset.startsWith("saved:")){const patch=patchStore.list().find(p=>p.id===preset.slice(6));if(patch)try{loadSnapshot(patch.snapshot);activeSavedPatch=patch.id;updateControls();scheduleSession();}catch(error){showError(error);}return;}
  if (!(preset in presets)) return;
  activeSavedPatch=null;stopSequence();
  releaseAll();
  for (const p of parameters) {
    if (p.scope === "global" || p.readonly || [71, 72, 119, 120, 137, 138, 139, 146, 147, 150, 151, 153].includes(p.id) || isDrum() && !instrumentParameter(p.id)) continue;
    const value = presets[preset][p.id] ?? p.default;
    (isDrum() ? drums[global(142)] : timbres[selected].values)[p.id] = value;
  }
  // Load the full program in one command so dependent controls reset together.
  timbres[selected].preset = preset; send({type: "load", program: state()}); updateControls(); updateKeys(); scheduleSession();
}
function state() { return {version: 1, timbres: timbres.map(t => [...t.values]), drums: drums.map(v => [...v])}; }
function acceptState(program) {
  for (let i = 0; i < 4; i++) timbres[i].values = [...program.timbres[i]];
  for (let i = 0; i < 16; i++) drums[i] = [...program.drums[i]];
  updateControls(); updateKeys();
}
function normalizeEngine(value){
  const program=structuredClone(value);
  if(program?.version!==1||program.timbres?.length!==4||program.drums?.length!==16)throw new Error("Choose a Rustias patch or program file.");
  for(const v of [...program.timbres,...program.drums]){
    if(Array.isArray(v)&&v.length===153)v.push(v[151]);
    if(Array.isArray(v)&&v.length===154){const legacy=v[29];v.push(legacy===3?0:legacy===2?1:legacy>=4?legacy-2:1);if(legacy>=2)v[29]=2;}
    if(!Array.isArray(v)||v.length!==parameters.length||parameters.some(p=>!Number.isInteger(v[p.id])||v[p.id]<p.min||v[p.id]>p.max||p.values&&!p.values.includes(v[p.id]))||v[0]>=4&&v[10]!==0||v[119]>v[120])throw new Error("The program contains invalid parameters.");
  }
  if(parameters.some(p=>p.scope==="global"&&program.timbres.some(v=>v[p.id]!==program.timbres[0][p.id])))throw new Error("Global settings must agree across timbres.");return program;
}
function snapshot(){return {version:2,engine:state(),sequencer:sequenceUI.getConfig(),samples:sampleUI?.getConfig?.()??null};}
function loadSnapshot(value){
  const program=normalizeEngine(value.version===2?value.engine:value),sequence=validateSequence(value.version===2?value.sequencer:emptySequence()),samples=sampleUI?.validateConfig(value.version===2?value.samples:null);
  stop();for(const t of timbres)t.preset="custom";acceptState(program);sequenceUI.setConfig(sequence);send({type:"load",program});send({type:"sequencer",config:sequence});
  if(sampleUI)sampleUI.setConfig(samples).then(updateControls).catch(showError);
}
function scheduleSession(){clearTimeout(autosaveTimer);autosaveTimer=setTimeout(saveSession,300);}
function saveSession(){try{patchStore.saveSession({snapshot:snapshot(),activeSavedPatch,selected,volume:Number($("#volume").value)});}catch(error){showError(new Error(`Could not save this patch in the browser: ${error.message}`));}}
function refreshLibrary(){programPicker.options=programPicker.options.filter(o=>!String(o.value).startsWith("saved:"));for(const patch of patchStore.list())programPicker.options.push({value:`saved:${patch.id}`,label:patch.name});updateControls();}
sequenceUI=createSequencer({onChange:config=>{send({type:"sequencer",config});scheduleSession();},onPlay:playSequence,onStop:stopSequence,onReset:()=>send({type:"sequence-reset"}),onSelectTimbre:t=>{$(`[data-timbre="${t}"]`).click();},onAudition:auditionStep});
sequenceUI.setReady(false);
async function playSequence(){const request=++sequenceRequest;try{await startAudio();if(request!==sequenceRequest)return;send({type:"sequencer",config:sequenceUI.getConfig()});send({type:"sequence-play"});sequenceUI.setStatus({running:true,positions:[0,0,0,0]});}catch(error){showError(error);}}
async function auditionStep(timbre,step,resolution){const request=++auditionRequest;try{await startAudio();if(request===auditionRequest)send({type:"audition",timbre,step,resolution});}catch(error){showError(error);}}
function stopSequence(){sequenceRequest++;send({type:"sequence-stop"});sequenceUI?.setStatus({running:false,positions:[-1,-1,-1,-1]});}
$("#save-patch").addEventListener("click",()=>{const existing=patchStore.list().find(p=>p.id===activeSavedPatch);if(existing){try{patchStore.save(existing.name,snapshot(),existing.id);refreshLibrary();saveSession();}catch(error){showError(error);}return;}$("#patch-name").value=`Patch ${String(patchStore.list().length+1).padStart(2,"0")}`;$("#patch-editor").showModal();$("#patch-name").select();});
$("#patch-cancel").addEventListener("click",()=>$("#patch-editor").close());$("#patch-form").addEventListener("submit",event=>{event.preventDefault();try{const saved=patchStore.save($("#patch-name").value,snapshot());activeSavedPatch=saved.id;refreshLibrary();saveSession();$("#patch-editor").close();}catch(error){showError(error);}});
window.addEventListener("pagehide",saveSession);

let programDownloadUrl;
$("#save-program").addEventListener("click", event => {
  if (programDownloadUrl) URL.revokeObjectURL(programDownloadUrl);
  programDownloadUrl = URL.createObjectURL(new Blob([JSON.stringify(snapshot(), null, 2)], {type: "application/json"}));
  event.currentTarget.href = programDownloadUrl;
});
$("#load-program").addEventListener("click", () => $("#program-file").click());
$("#program-file").addEventListener("change", async event => {
  try {
    const file = event.target.files[0]; if (!file) return;
    if (file.size > 1024*1024) throw new Error("This program file is too large.");
    const value = JSON.parse(await file.text());
    loadSnapshot(value);activeSavedPatch=null;await startAudio();send({type:"load",program:state()});send({type:"sequencer",config:sequenceUI.getConfig()});$("#error").hidden=true;scheduleSession();
  } catch (error) { showError(error); } finally { event.target.value = ""; }
});
function noteLabel(note) { return `${keyNames[note % 12]}${Math.floor(note / 12) - 1}`; }
function updateKeys() {
  const drum = isDrum(); $("#keybed-title").textContent = drum ? "DRUM PADS" : "KEYBOARD";
  document.querySelectorAll(".key").forEach((key, index) => {
    const note = (octave + 1) * 12 + index; key.dataset.note = note;
    const label = drum ? `Drum ${String(index + 1).padStart(2, "0")}` : noteLabel(note);
    key.querySelector("span").textContent = drum ? String(index + 1).padStart(2, "0") : label; key.setAttribute("aria-label", label);
    key.setAttribute("aria-pressed", (noteCounts.get(drum ? `drum:${index}` : `${selected}:${note}`) ?? 0) > 0);
    key.classList.toggle("black", !drum && [1, 3, 6, 8, 10].includes(index % 12));
  });
  $("#octave-label").textContent = drum ? "01–16" : `${noteLabel((octave + 1) * 12)}–${noteLabel((octave + 1) * 12 + 15)}`;
  $("#octave-down").disabled = drum || octave === 0; $("#octave-up").disabled = drum || octave === 7;
}
function startAudio() { if (audioStarting) return audioStarting; audioStarting = setupAudio().finally(() => { audioStarting = null; }); return audioStarting; }
async function setupAudio() {
  if (node && context?.state === "running") {await sampleUI.waitReady();return;}
  $("#error").hidden = true;
  if (context) { await context.resume(); updatePower(); return; }
  const AudioContextClass = window.AudioContext ?? window.webkitAudioContext;
  if (!AudioContextClass || !window.AudioWorkletNode) throw new Error("This browser does not support AudioWorklet. Try a current browser over HTTPS.");
  context = new AudioContextClass({sampleRate: 48000, latencyHint: "interactive"}); const pendingContext = context;
  try {
    await context.audioWorklet.addModule(new URL("./worklet.js", import.meta.url));
    const pendingNode = new AudioWorkletNode(context, "rustias", {numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2], processorOptions: {module, gain: Number($("#volume").value) / 100}});
    const ready = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("The Rust audio engine did not start.")), 10000);
      pendingNode.onprocessorerror = () => { clearTimeout(timeout); reject(new Error("The audio engine stopped unexpectedly.")); showError(new Error("The audio engine stopped unexpectedly.")); };
      pendingNode.port.onmessage = ({data}) => {
        if(sampleUI?.handleMessage(data))return;
        if (data.type === "ready") { clearTimeout(timeout); resolve(); }
        else if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); showError(new Error(data.message)); }
        else if (data.type === "state") {acceptState(data.program);scheduleSession();}
        else if (data.type === "warning") showError(new Error(data.message));
        else if (data.type === "stats") {
          if(data.sequence)sequenceUI.setStatus(data.sequence);
          $("#voices").textContent = data.voices; $("#meter").value = data.peak;
          document.body.dataset.frames = data.frames; document.body.dataset.audibleFrames = data.audibleFrames; document.body.dataset.peak = data.peak; document.body.dataset.callbacks = data.callbacks;
        }
      };
    });
    pendingNode.connect(context.destination); node = pendingNode; send({type: "load", program: state()}); send({type:"sequencer",config:sequenceUI.getConfig()});
    await context.resume(); await ready; await sampleUI.attach(pendingNode,context); context.onstatechange = updatePower; updatePower();
  } catch (error) { sampleUI?.detach(); node?.disconnect(); node = null; context = null; await pendingContext.close(); throw error; }
}
function updatePower() {
  const playing = context?.state === "running";
  $("#power").textContent = playing ? "Pause audio" : "Start audio"; $("#power").setAttribute("aria-pressed", !!playing);
  $("#status").textContent = playing ? `Native engine · ${context.sampleRate / 1000} kHz` : "Ready · no firmware required"; $("#led").classList.toggle("live", !!playing);
}
async function down(source, note, instrument) {
  if (held.has(source) || !module) return;
  const entry = {timbre: selected, note, instrument: isDrum() ? instrument : undefined, pending: true, released: false}; held.set(source, entry);
  try { await startAudio(); } catch (error) { held.delete(source); showError(error); return; }
  if (held.get(source) !== entry) return;
  entry.pending = false; entry.startedAt = performance.now();
  const id = entry.instrument === undefined ? `${entry.timbre}:${entry.note}` : `drum:${entry.instrument}`, count = noteCounts.get(id) ?? 0;
  noteCounts.set(id, count + 1);
  if (!count) send(entry.instrument === undefined ? {type: "note", timbre: entry.timbre, note, velocity: 100} : {type: "drum", instrument: entry.instrument, velocity: 100});
  updateKeys(); if (entry.released) up(source);
}
function up(source, force = false) {
  const entry = held.get(source); if (!entry) return;
  if (entry.pending) { if (force) held.delete(source); else entry.released = true; return; }
  const remaining = 32 - (performance.now() - entry.startedAt);
  if (!force && remaining > 0) { entry.releaseTimer ??= setTimeout(() => up(source, true), remaining); return; }
  clearTimeout(entry.releaseTimer); held.delete(source);
  const id = entry.instrument === undefined ? `${entry.timbre}:${entry.note}` : `drum:${entry.instrument}`, count = Math.max(0, (noteCounts.get(id) ?? 0) - 1);
  if (!count) { noteCounts.delete(id); send(entry.instrument === undefined ? {type: "note", timbre: entry.timbre, note: entry.note, velocity: 0} : {type: "drum", instrument: entry.instrument, velocity: 0}); } else noteCounts.set(id, count);
  updateKeys();
}
function releaseAll() { for (const source of [...held.keys()]) up(source, true); }
function stop() { auditionRequest++; stopSequence(); releaseAll(); send({type: "stop"}); $("#voices").textContent = "0"; $("#meter").value = 0; }
for (let index = 0; index < 16; index++) {
  const key = document.createElement("button"); key.type = "button"; key.className = "key"; key.innerHTML = `<span></span><small>${computerKeys[index].toUpperCase()}</small>`; $("#keys").append(key);
  key.addEventListener("pointerdown", event => { if (event.button !== 0 || !module) return; event.preventDefault(); key.setPointerCapture(event.pointerId); down(`pointer-${event.pointerId}`, Number(key.dataset.note), index); });
  const release = event => up(`pointer-${event.pointerId}`, event.type === "pointercancel"); for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) key.addEventListener(event, release);
  key.addEventListener("keydown", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); if (!event.repeat) down(`pad-${index}`, Number(key.dataset.note), index); } });
  key.addEventListener("focusout", () => up(`pad-${index}`)); key.addEventListener("keyup", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); up(`pad-${index}`); } });
}
document.addEventListener("keydown", event => {
  if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || !module || (isChoosing() || event.target.closest("input,select,dialog,[role=slider],[role=combobox],[role=option],.key"))) return;
  const key = event.key.toLowerCase(), index = computerKeys.indexOf(key); if (index >= 0) { event.preventDefault(); down(`key-${key}`, (octave + 1) * 12 + index, index); } if (event.key === "Escape") stop();
});
document.addEventListener("keyup", event => up(`key-${event.key.toLowerCase()}`)); window.addEventListener("blur", releaseAll);
document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
$("#octave-down").addEventListener("click", () => { releaseAll(); octave = Math.max(0, octave - 1); updateKeys(); });
$("#octave-up").addEventListener("click", () => { releaseAll(); octave = Math.min(7, octave + 1); updateKeys(); });
$("#volume").addEventListener("input", event => {send({type: "gain", value: Number(event.target.value) / 100});scheduleSession();});
$("#panic").addEventListener("click", stop);
$("#power").addEventListener("click", async () => { try { if (context?.state === "running") { stop(); await context.suspend(); updatePower(); } else await startAudio(); } catch (error) { showError(error); } });
$("#midi").addEventListener("click", async () => {
  try {
    if (!navigator.requestMIDIAccess) throw new Error("Web MIDI is unavailable in this browser. The keyboard and pads still work.");
    midiAccess ??= await navigator.requestMIDIAccess({sysex: false}); await startAudio();
    const connect = () => { for (const input of midiAccess.inputs.values()) input.onmidimessage = event => { if (event.data.length === 3) send({type: "midi", bytes: [...event.data]}); }; $("#midi").textContent = midiAccess.inputs.size ? `MIDI · ${midiAccess.inputs.size}` : "MIDI · no device"; }; connect(); midiAccess.onstatechange = connect;
  } catch (error) { showError(error); }
});
const fullscreenButton=$("#fullscreen");
function updateFullscreen(){const active=!!(document.fullscreenElement??document.webkitFullscreenElement);fullscreenButton.setAttribute("aria-pressed",active);fullscreenButton.setAttribute("aria-label",active?"Exit full screen":"Full screen");fullscreenButton.title=active?"Exit full screen":"Full screen";document.body.dataset.fullscreen=active?"on":"off";}
fullscreenButton.addEventListener("click",async()=>{
  try {
    if(document.fullscreenElement)await document.exitFullscreen();
    else if(document.webkitFullscreenElement)document.webkitExitFullscreen();
    else if(document.documentElement.requestFullscreen)await document.documentElement.requestFullscreen();
    else if(document.documentElement.webkitRequestFullscreen)document.documentElement.webkitRequestFullscreen();
    else throw new Error("Open this instrument in a browser that supports full screen.");
    updateFullscreen();
  }catch(error){showError(error);}
});
document.addEventListener("fullscreenchange",updateFullscreen);document.addEventListener("webkitfullscreenchange",updateFullscreen);updateFullscreen();
function prepareSampleInstrument(instrument){
  for(const [parameter,value] of [[3,0],[4,127],[5,127],[6,32],[9,127],[1,127],[2,0]]){drums[instrument][parameter]=value;send({type:"drum-control",instrument,parameter,value});}
}
function enableDrumKit(){if(!global(140))setControl(140,1);if(!isDrum())$(`[data-timbre="${global(141)}"]`).click();}
sampleUI=createDrumSamples({getInstrument:()=>global(142),isDrum,ensureAudio:startAudio,onError:showError,onChange:()=>{updateControls();scheduleSession();},onAssign:(instrument,fresh)=>{enableDrumKit();if(fresh)prepareSampleInstrument(instrument);},onKit:()=>{
  enableDrumKit();for(let i=0;i<16;i++){prepareSampleInstrument(i);for(const [parameter,value] of [[146,60+i],[147,[3,4].includes(i)?1:0]]){drums[i][parameter]=value;send({type:"drum-control",instrument:i,parameter,value});}}
  setControl(142,0);
}});
refreshLibrary();
try{const session=patchStore.session();if(session?.snapshot){loadSnapshot(session.snapshot);activeSavedPatch=session.activeSavedPatch??null;if(Number.isInteger(session.selected)&&session.selected>=0&&session.selected<4)$(`[data-timbre="${session.selected}"]`).click();if(Number.isFinite(session.volume))$("#volume").value=session.volume;}}catch(error){showError(new Error(`Could not restore the saved session: ${error.message}`));}
updateControls(); updateKeys(); document.body.dataset.parameterCount = parameters.length;
try {
  const response = await fetch(new URL("./rustias.wasm", import.meta.url)); if (!response.ok) throw new Error(`Could not load the Rust engine (${response.status}).`);
  module = await WebAssembly.compile(await response.arrayBuffer()); sequenceUI.setReady(true); $("#power").disabled = false; updatePower(); document.body.dataset.engine = "ready";
} catch (error) { $("#status").textContent = "Engine unavailable"; showError(error); }
