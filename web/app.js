const $ = (selector) => document.querySelector(selector);
const presets = {
  init: [0, 100, 20, 12, 48, 100, 48, 100, 64, 0],
  pad: [2, 78, 35, 80, 64, 108, 78, 100, 64, 0],
  pulse: [1, 66, 55, 0, 44, 88, 24, 110, 64, 0],
  pluck: [3, 107, 15, 0, 40, 0, 35, 110, 64, 0],
};
const timbres = Array.from({ length: 4 }, () => ({ values: [...presets.init], preset: "init" }));
const names = ["Saw", "Pulse", "Triangle", "Sine"];
const keyNames = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const computerKeys = ["a", "w", "s", "e", "d", "f", "t", "g", "y", "h", "u", "j", "k", "o", "l", "p"];
const controls = [
  [1, "Cutoff", "#filter-knobs"], [2, "Resonance", "#filter-knobs"],
  [3, "Attack", "#envelope-knobs"], [4, "Decay", "#envelope-knobs"], [5, "Sustain", "#envelope-knobs"], [6, "Release", "#envelope-knobs"],
  [7, "Level", "#output-knobs"], [8, "Pan", "#output-knobs"],
];
let selected = 0, octave = 4, context, node, module, midiAccess, audioStarting;
const held = new Map();
const noteCounts = new Map();
const knobs = new Map();
const send = (message) => node?.port.postMessage(message);

function showError(error) {
  $("#error").textContent = error.message ?? String(error);
  $("#error").hidden = false;
}
function format(parameter, value) {
  if (parameter === 1) { const hz = 30 * 200 ** (value / 127); return hz >= 1000 ? `${(hz / 1000).toFixed(1)} kHz` : `${Math.round(hz)} Hz`; }
  if ([3, 4, 6].includes(parameter)) { const ms = 3 * 2000 ** (value / 127); return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`; }
  if (parameter === 8) return value === 64 ? "Center" : `${value < 64 ? "L" : "R"} ${Math.round(Math.abs(value - 64) / 64 * 100)}`;
  return `${Math.round(value / 127 * 100)}%`;
}
function setControl(parameter, value, fromUser = true) {
  value = Math.max(0, Math.min(127, Math.round(value)));
  timbres[selected].values[parameter] = value;
  if (fromUser) timbres[selected].preset = "custom";
  send({ type: "control", timbre: selected, parameter, value });
  updateControls();
}
function updateControls() {
  const values = timbres[selected].values;
  for (const [parameter, knob] of knobs) {
    const value = values[parameter];
    knob.button.style.setProperty("--angle", `${-135 + value / 127 * 270}deg`);
    knob.button.setAttribute("aria-valuenow", value);
    knob.button.setAttribute("aria-valuetext", format(parameter, value));
    knob.output.value = format(parameter, value);
  }
  document.querySelectorAll("[data-wave]").forEach(button => button.setAttribute("aria-pressed", Number(button.dataset.wave) === values[0]));
  document.querySelectorAll("[data-filter]").forEach(button => button.setAttribute("aria-pressed", Number(button.dataset.filter) === values[9]));
  $("#wave-name").textContent = names[values[0]].toUpperCase();
  $("#wave-display").setAttribute("aria-label", `${names[values[0]]} waveform`);
  const paths = ["M0 60 L60 12 L60 60 L120 12 L120 60 L180 12 L180 60 L240 12", "M0 60 V12 H30 V60 H60 V12 H90 V60 H120 V12 H150 V60 H180 V12 H210 V60 H240", "M0 36 L30 12 L60 36 L90 60 L120 36 L150 12 L180 36 L210 60 L240 36"];
  if (values[0] === 3) paths[3] = `M${Array.from({ length: 121 }, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 4 * Math.PI)}`).join(" L")}`;
  $("#wave-path").setAttribute("d", paths[values[0]]);
  if (timbres[selected].preset === "custom" && !$("#preset option[value=custom]")) $("#preset").add(new Option("Custom", "custom"));
  $("#preset").value = timbres[selected].preset;
}
for (const [parameter, label, target] of controls) {
  const wrap = document.createElement("div"); wrap.className = "knob-control";
  const button = document.createElement("button"); button.className = "knob"; button.type = "button";
  button.id = `control-${parameter}`; button.setAttribute("role", "slider"); button.setAttribute("aria-label", label);
  button.setAttribute("aria-valuemin", "0"); button.setAttribute("aria-valuemax", "127");
  button.innerHTML = '<span class="knob-face"></span>';
  const title = document.createElement("label"); title.htmlFor = button.id; title.textContent = label.toUpperCase();
  const output = document.createElement("output"); output.htmlFor = button.id;
  wrap.append(title, button, output); $(target).append(wrap); knobs.set(parameter, { button, output });
  let drag;
  button.addEventListener("pointerdown", event => { if (event.button !== 0) return; button.setPointerCapture(event.pointerId); drag = { y: event.clientY, value: timbres[selected].values[parameter] }; event.preventDefault(); });
  button.addEventListener("pointermove", event => { if (!drag) return; setControl(parameter, drag.value + (drag.y - event.clientY) * (event.shiftKey ? 0.12 : 0.7)); });
  const end = () => { drag = null; }; button.addEventListener("pointerup", end); button.addEventListener("pointercancel", end); button.addEventListener("lostpointercapture", end);
  button.addEventListener("wheel", event => { event.preventDefault(); setControl(parameter, timbres[selected].values[parameter] - Math.sign(event.deltaY)); }, { passive: false });
  button.addEventListener("keydown", event => {
    const steps = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 };
    if (event.key in steps) { event.preventDefault(); setControl(parameter, timbres[selected].values[parameter] + steps[event.key]); }
    else if (event.key === "Home" || event.key === "End") { event.preventDefault(); setControl(parameter, event.key === "Home" ? 0 : 127); }
  });
  button.addEventListener("dblclick", () => setControl(parameter, presets.init[parameter]));
}
document.querySelectorAll("[data-wave]").forEach(button => button.addEventListener("click", () => setControl(0, Number(button.dataset.wave))));
document.querySelectorAll("[data-filter]").forEach(button => button.addEventListener("click", () => setControl(9, Number(button.dataset.filter))));
document.querySelectorAll("[data-timbre]").forEach(button => button.addEventListener("click", () => {
  selected = Number(button.dataset.timbre);
  document.querySelectorAll("[data-timbre]").forEach(tab => tab.setAttribute("aria-selected", tab === button)); updateControls(); updateKeys();
}));
$("#preset").addEventListener("change", event => {
  const preset = event.target.value; if (!(preset in presets)) return;
  timbres[selected] = { values: [...presets[preset]], preset };
  for (const [parameter, value] of timbres[selected].values.entries()) send({ type: "control", timbre: selected, parameter, value });
  updateControls();
});
function noteLabel(note) { return `${keyNames[note % 12]}${Math.floor(note / 12) - 1}`; }
function updateKeys() {
  document.querySelectorAll(".key").forEach((key, index) => {
    const note = (octave + 1) * 12 + index; key.dataset.note = note;
    key.querySelector("span").textContent = noteLabel(note); key.setAttribute("aria-label", noteLabel(note));
    key.setAttribute("aria-pressed", (noteCounts.get(`${selected}:${note}`) ?? 0) > 0);
  });
  $("#octave-label").textContent = `${noteLabel((octave + 1) * 12)}–${noteLabel((octave + 1) * 12 + 15)}`;
  $("#octave-down").disabled = octave === 0; $("#octave-up").disabled = octave === 7;
}
function startAudio() {
  if (audioStarting) return audioStarting;
  audioStarting = setupAudio().finally(() => { audioStarting = null; });
  return audioStarting;
}
async function setupAudio() {
  if (node && context?.state === "running") return;
  $("#error").hidden = true;
  if (context) { await context.resume(); updatePower(); return; }
  const AudioContextClass = window.AudioContext ?? window.webkitAudioContext;
  if (!AudioContextClass || !window.AudioWorkletNode) throw new Error("This browser does not support AudioWorklet. Try a current browser over HTTPS.");
  context = new AudioContextClass({ sampleRate: 48000, latencyHint: "interactive" });
  const pendingContext = context;
  try {
    await context.audioWorklet.addModule(new URL("./worklet.js", import.meta.url));
    const pendingNode = new AudioWorkletNode(context, "rustias", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2], processorOptions: { module, gain: Number($("#volume").value) / 100 } });
    const ready = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("The Rust audio engine did not start.")), 10000);
      pendingNode.onprocessorerror = () => { clearTimeout(timeout); reject(new Error("The audio engine stopped unexpectedly.")); showError(new Error("The audio engine stopped unexpectedly.")); };
      pendingNode.port.onmessage = ({ data }) => {
        if (data.type === "ready") { clearTimeout(timeout); resolve(); }
        else if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); showError(new Error(data.message)); }
        else if (data.type === "stats") {
          $("#voices").textContent = data.voices; $("#meter").value = data.peak;
          document.body.dataset.frames = data.frames; document.body.dataset.audibleFrames = data.audibleFrames; document.body.dataset.peak = data.peak; document.body.dataset.callbacks = data.callbacks;
        }
      };
    });
    pendingNode.connect(context.destination); node = pendingNode;
    for (let timbre = 0; timbre < 4; timbre++) for (const [parameter, value] of timbres[timbre].values.entries()) send({ type: "control", timbre, parameter, value });
    await context.resume(); await ready;
    context.onstatechange = updatePower; updatePower();
  } catch (error) { node?.disconnect(); node = null; context = null; await pendingContext.close(); throw error; }
}
function updatePower() {
  const playing = context?.state === "running";
  $("#power").textContent = playing ? "Pause audio" : "Start audio"; $("#power").setAttribute("aria-pressed", !!playing);
  $("#status").textContent = playing ? `Native engine · ${context.sampleRate / 1000} kHz` : "Ready · no firmware required";
  $("#led").classList.toggle("live", !!playing);
}
async function down(source, note) {
  if (held.has(source)) return;
  const entry = { timbre: selected, note, pending: true, released: false }; held.set(source, entry);
  try { await startAudio(); } catch (error) { held.delete(source); showError(error); return; }
  if (held.get(source) !== entry) return;
  entry.pending = false; entry.startedAt = performance.now();
  const id = `${entry.timbre}:${entry.note}`; const count = noteCounts.get(id) ?? 0;
  noteCounts.set(id, count + 1); if (!count) send({ type: "note", timbre: entry.timbre, note, velocity: 100 }); updateKeys();
  if (entry.released) up(source);
}
function up(source, force = false) {
  const entry = held.get(source); if (!entry) return;
  if (entry.pending) {
    if (force) held.delete(source); else entry.released = true;
    return;
  }
  const remaining = 32 - (performance.now() - entry.startedAt);
  if (!force && remaining > 0) {
    entry.releaseTimer ??= setTimeout(() => up(source, true), remaining);
    return;
  }
  clearTimeout(entry.releaseTimer); held.delete(source);
  const id = `${entry.timbre}:${entry.note}`; const count = Math.max(0, (noteCounts.get(id) ?? 0) - 1);
  if (!count) { noteCounts.delete(id); send({ type: "note", timbre: entry.timbre, note: entry.note, velocity: 0 }); } else noteCounts.set(id, count);
  updateKeys();
}
function releaseAll() { for (const source of [...held.keys()]) up(source, true); }
function stop() { releaseAll(); send({ type: "stop" }); $("#voices").textContent = "0"; $("#meter").value = 0; }
for (let index = 0; index < 16; index++) {
  const key = document.createElement("button"); key.type = "button"; key.className = `key${[1, 3, 6, 8, 10].includes(index % 12) ? " black" : ""}`;
  key.innerHTML = `<span></span><small>${computerKeys[index].toUpperCase()}</small>`; $("#keys").append(key);
  key.addEventListener("pointerdown", event => { if (event.button !== 0 || !module) return; event.preventDefault(); key.setPointerCapture(event.pointerId); down(`pointer-${event.pointerId}`, Number(key.dataset.note)); });
  const release = event => up(`pointer-${event.pointerId}`, event.type === "pointercancel"); key.addEventListener("pointerup", release); key.addEventListener("pointercancel", release); key.addEventListener("lostpointercapture", release);
  key.addEventListener("keydown", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); if (!event.repeat) down(`pad-${index}`, Number(key.dataset.note)); } });
  key.addEventListener("focusout", () => up(`pad-${index}`));
  key.addEventListener("keyup", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); up(`pad-${index}`); } });
}
document.addEventListener("keydown", event => {
  if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || !module || event.target.closest("input,select,[role=slider]")) return;
  const key = event.key.toLowerCase(), index = computerKeys.indexOf(key);
  if (index >= 0) { event.preventDefault(); down(`key-${key}`, (octave + 1) * 12 + index); }
  if (event.key === "Escape") stop();
});
document.addEventListener("keyup", event => up(`key-${event.key.toLowerCase()}`));
window.addEventListener("blur", releaseAll);
document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
$("#octave-down").addEventListener("click", () => { releaseAll(); octave = Math.max(0, octave - 1); updateKeys(); });
$("#octave-up").addEventListener("click", () => { releaseAll(); octave = Math.min(7, octave + 1); updateKeys(); });
$("#volume").addEventListener("input", event => send({ type: "gain", value: Number(event.target.value) / 100 }));
$("#panic").addEventListener("click", stop);
$("#power").addEventListener("click", async () => {
  try { if (context?.state === "running") { stop(); await context.suspend(); updatePower(); } else await startAudio(); } catch (error) { showError(error); }
});
$("#midi").addEventListener("click", async () => {
  try {
    if (!navigator.requestMIDIAccess) throw new Error("Web MIDI is unavailable in this browser. The keyboard and pads still work.");
    midiAccess ??= await navigator.requestMIDIAccess({ sysex: false });
    await startAudio();
    const connect = () => {
      for (const input of midiAccess.inputs.values()) input.onmidimessage = event => { if (event.data.length === 3) send({ type: "midi", bytes: [...event.data] }); };
      $("#midi").textContent = midiAccess.inputs.size ? `MIDI · ${midiAccess.inputs.size}` : "MIDI · no device";
    }; connect(); midiAccess.onstatechange = connect;
  } catch (error) { showError(error); }
});
updateControls(); updateKeys();
try {
  const response = await fetch(new URL("./rustias.wasm", import.meta.url));
  if (!response.ok) throw new Error(`Could not load the Rust engine (${response.status}).`);
  module = await WebAssembly.compile(await response.arrayBuffer());
  $("#power").disabled = false; updatePower(); document.body.dataset.engine = "ready";
} catch (error) { $("#status").textContent = "Engine unavailable"; showError(error); }
