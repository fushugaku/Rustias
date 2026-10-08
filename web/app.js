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
const held = new Map(), noteCounts = new Map(), knobs = new Map(), fields = new Map();
const send = message => node?.port.postMessage(message);
const global = id => timbres[0].values[id];
const isDrum = () => global(140) !== 0 && global(141) === selected;
const instrumentParameter = id => parameters[id].scope !== "global" && !(id >= 59 && id <= 72 || id >= 114 && id <= 120 || id >= 137 && id <= 139 || id >= 150 && id <= 151);
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
  if (id === 10) return v[0] >= 4;
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
  send({type: "control", timbre: selected, parameter: id, value}); updateControls(); updateKeys();
}
function updateControls() {
  const v = values();
  for (const [id, knob] of knobs) {
    const p = parameters[id], value = v[id];
    knob.button.style.setProperty("--angle", `${-135 + (value - p.min) / (p.max - p.min) * 270}deg`);
    knob.button.setAttribute("aria-valuenow", value); knob.button.setAttribute("aria-valuetext", format(id, value));
    knob.output.value = format(id, value); knob.button.disabled = disabled(id, v);
  }
  for (const [id, field] of fields) {
    const p = parameters[id], value = v[id];
    if (p.readonly) { field.output.value = format(id, value); continue; }
    if (field.select) field.select.value = value;
    else {
      field.range.value = value;
      if (document.activeElement !== field.number) field.number.value = displayValue(p, value);
      field.output.value = format(id, value);
    }
    for (const input of field.inputs) input.disabled = disabled(id, v);
    field.wrap.classList.toggle("inactive", disabled(id, v));
  }
  document.querySelectorAll("[data-wave]").forEach(button => button.setAttribute("aria-pressed", Number(button.dataset.wave) === v[0]));
  document.querySelectorAll("[data-filter]").forEach(button => button.setAttribute("aria-pressed", Number(button.dataset.filter) === v[9]));
  $("#wave-name").textContent = names[v[0]].toUpperCase(); $("#wave-display").setAttribute("aria-label", `${names[v[0]]} waveform`);
  const paths = ["M0 60 L60 12 L60 60 L120 12 L120 60 L180 12 L180 60 L240 12", "M0 60 V12 H30 V60 H60 V12 H90 V60 H120 V12 H150 V60 H180 V12 H210 V60 H240", "M0 36 L30 12 L60 36 L90 60 L120 36 L150 12 L180 36 L210 60 L240 36", `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 4 * Math.PI)}`).join(" L")}`, `M${Array.from({length: 61}, (_, i) => `${i * 4} ${12 + (i * 17 % 49)}`).join(" L")}`, `M${Array.from({length: 121}, (_, i) => `${i * 2} ${36 - 24 * Math.sin(i / 120 * 10 * Math.PI) * Math.sin(i / 120 * 2 * Math.PI)}`).join(" L")}`];
  $("#wave-path").setAttribute("d", paths[v[0]]);
  $("#edit-context").textContent = isDrum() ? `Drum ${String(global(142) + 1).padStart(2, "0")} · Timbre ${String(selected + 1).padStart(2, "0")}` : `Timbre ${String(selected + 1).padStart(2, "0")}`;
  if (timbres[selected].preset === "custom" && !$("#preset option[value=custom]")) $("#preset").add(new Option("Custom", "custom"));
  $("#preset").value = timbres[selected].preset;
}
const mainControls = [[1, "Cutoff", "#filter-knobs"], [2, "Resonance", "#filter-knobs"], [3, "Attack", "#envelope-knobs"], [4, "Decay", "#envelope-knobs"], [5, "Sustain", "#envelope-knobs"], [6, "Release", "#envelope-knobs"], [7, "Level", "#output-knobs"], [8, "Pan", "#output-knobs"]];
for (const [id, label, target] of mainControls) {
  const p = parameters[id], wrap = document.createElement("div"), button = document.createElement("button");
  wrap.className = "knob-control"; button.className = "knob"; button.type = "button"; button.id = `control-${id}`;
  button.setAttribute("role", "slider"); button.setAttribute("aria-label", label); button.setAttribute("aria-valuemin", p.min); button.setAttribute("aria-valuemax", p.max); button.innerHTML = '<span class="knob-face"></span>';
  const title = document.createElement("label"), output = document.createElement("output"); title.htmlFor = button.id; title.textContent = label.toUpperCase(); output.htmlFor = button.id;
  wrap.append(title, button, output); $(target).append(wrap); knobs.set(id, {button, output});
  let drag;
  button.addEventListener("pointerdown", event => { if (event.button !== 0) return; button.setPointerCapture(event.pointerId); drag = {y: event.clientY, value: values()[id]}; event.preventDefault(); });
  button.addEventListener("pointermove", event => { if (drag) setControl(id, drag.value + (drag.y - event.clientY) * (event.shiftKey ? 0.12 : 0.7)); });
  const end = () => { drag = null; }; for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) button.addEventListener(event, end);
  button.addEventListener("wheel", event => { event.preventDefault(); setControl(id, values()[id] - Math.sign(event.deltaY)); }, {passive: false});
  button.addEventListener("keydown", event => {
    const steps = {ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10};
    if (event.key in steps) { event.preventDefault(); setControl(id, values()[id] + steps[event.key]); }
    else if (["Home", "End"].includes(event.key)) { event.preventDefault(); setControl(id, event.key === "Home" ? p.min : p.max); }
  });
  button.addEventListener("dblclick", () => setControl(id, p.default));
}
function displayValue(p, value) { return p.id === 89 ? value / 10 : [72, 148, 141, 142].includes(p.id) ? value + 1 : value - (p.center ?? 0); }
function nativeValue(p, value) { return p.id === 89 ? value * 10 : [72, 148, 141, 142].includes(p.id) ? value - 1 : value + (p.center ?? 0); }
const categories = [
  ["Oscillators", ["OSC 1", "OSC 2", "Mixer"]],
  ["Filters", ["Filters", "Filter 1", "Filter 2", "Drive / Waveshaper"]],
  ["Envelopes", ["EG 1", "EG 2", "EG 3", "Amplifier"]],
  ["Modulation", ["LFO 1", "LFO 2", "Tempo", ...Array.from({length: 6}, (_, i) => `Patch ${i + 1}`)]],
  ["Voice & pitch", ["Voice", "Pitch", "Portamento", "Scale", "Custom scale"]],
  ["MIDI & drums", ["Timbre", "Performance", "Global MIDI", "Drum Kit", "Drum instrument"]],
];
for (const [index, [name, groups]] of categories.entries()) {
  const tab = document.createElement("button"), panel = document.createElement("div");
  tab.type = "button"; tab.id = `editor-tab-${index}`; tab.textContent = name; tab.setAttribute("role", "tab"); tab.setAttribute("aria-selected", index === 0); tab.setAttribute("aria-controls", `editor-panel-${index}`);
  panel.id = `editor-panel-${index}`; panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", tab.id); panel.className = "parameter-groups"; panel.hidden = index !== 0;
  tab.addEventListener("click", () => { for (const other of $("#editor-tabs").children) other.setAttribute("aria-selected", other === tab); for (const other of $("#editor-panels").children) other.hidden = other !== panel; });
  $("#editor-tabs").append(tab); $("#editor-panels").append(panel);
  for (const group of groups) {
    const section = document.createElement("section"), heading = document.createElement("h3"), grid = document.createElement("div");
    heading.textContent = group; grid.className = "parameter-grid"; section.append(heading, grid); panel.append(section);
    for (const p of parameters.filter(p => p.group === group)) {
      const wrap = document.createElement("div"), label = document.createElement("label"); wrap.className = "parameter"; wrap.dataset.parameter = p.id;
      label.textContent = p.id === 89 ? "Tempo" : p.label; label.htmlFor = `parameter-${p.id}`; wrap.append(label); grid.append(wrap);
      const aria = `${p.group} ${p.id === 89 ? "BPM" : p.label}`;
      if (p.readonly) {
        const output = document.createElement("output"); output.id = label.htmlFor; output.className = "parameter-readout"; output.setAttribute("aria-label", aria);
        wrap.append(output); fields.set(p.id, {wrap, output, inputs: []});
      } else if (p.options) {
        const select = document.createElement("select"); select.id = label.htmlFor; select.setAttribute("aria-label", aria);
        for (const [i, text] of p.options.entries()) select.add(new Option(text, p.values?.[i] ?? p.min + i));
        select.addEventListener("change", () => setControl(p.id, Number(select.value))); wrap.append(select); fields.set(p.id, {wrap, select, inputs: [select]});
      } else {
        const range = document.createElement("input"), number = document.createElement("input"), output = document.createElement("output"), row = document.createElement("div");
        range.type = "range"; range.min = p.min; range.max = p.max; range.step = 1; range.setAttribute("aria-label", `${aria} slider`);
        number.id = label.htmlFor; number.type = "number"; number.min = displayValue(p, p.min); number.max = displayValue(p, p.max); number.step = p.id === 89 ? 0.1 : 1; number.setAttribute("aria-label", aria);
        range.addEventListener("input", () => setControl(p.id, Number(range.value)));
        number.addEventListener("input", () => { if (number.value !== "" && number.validity.valid) setControl(p.id, nativeValue(p, Number(number.value))); });
        number.addEventListener("change", () => { if (number.value !== "" && number.validity.valid) setControl(p.id, nativeValue(p, Number(number.value))); number.value = displayValue(p, values()[p.id]); });
        number.addEventListener("blur", () => { number.value = displayValue(p, values()[p.id]); });
        row.className = "parameter-value"; row.append(range, number); wrap.append(row, output); fields.set(p.id, {wrap, range, number, output, inputs: [range, number]});
      }
    }
  }
}
$("#editor-tabs").addEventListener("keydown", event => {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault(); const tabs = [...$("#editor-tabs").children], i = tabs.indexOf(document.activeElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (i + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length; tabs[next].click(); tabs[next].focus();
});
document.querySelectorAll("[data-wave]").forEach(button => button.addEventListener("click", () => setControl(0, Number(button.dataset.wave))));
document.querySelectorAll("[data-filter]").forEach(button => button.addEventListener("click", () => setControl(9, Number(button.dataset.filter))));
document.querySelectorAll("[data-timbre]").forEach(button => button.addEventListener("click", () => { selected = Number(button.dataset.timbre); document.querySelectorAll("[data-timbre]").forEach(tab => tab.setAttribute("aria-selected", tab === button)); updateControls(); updateKeys(); }));
$("#preset").addEventListener("change", event => {
  const preset = event.target.value; if (!(preset in presets)) return;
  releaseAll();
  for (const p of parameters) {
    if (p.scope === "global" || p.readonly || [71, 72, 119, 120, 137, 138, 139, 146, 147, 150, 151].includes(p.id) || isDrum() && !instrumentParameter(p.id)) continue;
    const value = presets[preset][p.id] ?? p.default;
    (isDrum() ? drums[global(142)] : timbres[selected].values)[p.id] = value;
  }
  // Load the full program in one command so dependent controls reset together.
  timbres[selected].preset = preset; send({type: "load", program: state()}); updateControls(); updateKeys();
});
function state() { return {version: 1, timbres: timbres.map(t => [...t.values]), drums: drums.map(v => [...v])}; }
function acceptState(program) {
  for (let i = 0; i < 4; i++) timbres[i].values = [...program.timbres[i]];
  for (let i = 0; i < 16; i++) drums[i] = [...program.drums[i]];
  updateControls(); updateKeys();
}
let programDownloadUrl;
$("#save-program").addEventListener("click", event => {
  if (programDownloadUrl) URL.revokeObjectURL(programDownloadUrl);
  programDownloadUrl = URL.createObjectURL(new Blob([JSON.stringify(state(), null, 2)], {type: "application/json"}));
  event.currentTarget.href = programDownloadUrl;
});
$("#load-program").addEventListener("click", () => $("#program-file").click());
$("#program-file").addEventListener("change", async event => {
  try {
    const file = event.target.files[0]; if (!file) return;
    if (file.size > 65536) throw new Error("This program file is too large.");
    const program = JSON.parse(await file.text());
    if (program.version !== 1 || program.timbres?.length !== 4 || program.drums?.length !== 16) throw new Error("Choose a Rustias program JSON file.");
    for (const v of [...program.timbres, ...program.drums]) {
      if (v.length !== parameters.length || parameters.some(p => !Number.isInteger(v[p.id]) || v[p.id] < p.min || v[p.id] > p.max || p.values && !p.values.includes(v[p.id])) || v[0] >= 4 && v[10] !== 0 || v[119] > v[120]) throw new Error("The program contains invalid parameters.");
    }
    if (parameters.some(p => p.scope === "global" && program.timbres.some(v => v[p.id] !== program.timbres[0][p.id]))) throw new Error("Global settings must agree across timbres.");
    releaseAll(); for (const t of timbres) t.preset = "custom"; acceptState(program);
    await startAudio(); send({type: "load", program}); $("#error").hidden = true;
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
  if (node && context?.state === "running") return;
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
        if (data.type === "ready") { clearTimeout(timeout); resolve(); }
        else if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); showError(new Error(data.message)); }
        else if (data.type === "state") acceptState(data.program);
        else if (data.type === "warning") showError(new Error(data.message));
        else if (data.type === "stats") {
          $("#voices").textContent = data.voices; $("#meter").value = data.peak;
          document.body.dataset.frames = data.frames; document.body.dataset.audibleFrames = data.audibleFrames; document.body.dataset.peak = data.peak; document.body.dataset.callbacks = data.callbacks;
        }
      };
    });
    pendingNode.connect(context.destination); node = pendingNode; send({type: "load", program: state()});
    await context.resume(); await ready; context.onstatechange = updatePower; updatePower();
  } catch (error) { node?.disconnect(); node = null; context = null; await pendingContext.close(); throw error; }
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
function stop() { releaseAll(); send({type: "stop"}); $("#voices").textContent = "0"; $("#meter").value = 0; }
for (let index = 0; index < 16; index++) {
  const key = document.createElement("button"); key.type = "button"; key.className = "key"; key.innerHTML = `<span></span><small>${computerKeys[index].toUpperCase()}</small>`; $("#keys").append(key);
  key.addEventListener("pointerdown", event => { if (event.button !== 0 || !module) return; event.preventDefault(); key.setPointerCapture(event.pointerId); down(`pointer-${event.pointerId}`, Number(key.dataset.note), index); });
  const release = event => up(`pointer-${event.pointerId}`, event.type === "pointercancel"); for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) key.addEventListener(event, release);
  key.addEventListener("keydown", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); if (!event.repeat) down(`pad-${index}`, Number(key.dataset.note), index); } });
  key.addEventListener("focusout", () => up(`pad-${index}`)); key.addEventListener("keyup", event => { if ([" ", "Enter"].includes(event.key)) { event.preventDefault(); up(`pad-${index}`); } });
}
document.addEventListener("keydown", event => {
  if (event.repeat || event.metaKey || event.ctrlKey || event.altKey || !module || event.target.closest("input,select,[role=slider],.key")) return;
  const key = event.key.toLowerCase(), index = computerKeys.indexOf(key); if (index >= 0) { event.preventDefault(); down(`key-${key}`, (octave + 1) * 12 + index, index); } if (event.key === "Escape") stop();
});
document.addEventListener("keyup", event => up(`key-${event.key.toLowerCase()}`)); window.addEventListener("blur", releaseAll);
document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
$("#octave-down").addEventListener("click", () => { releaseAll(); octave = Math.max(0, octave - 1); updateKeys(); });
$("#octave-up").addEventListener("click", () => { releaseAll(); octave = Math.min(7, octave + 1); updateKeys(); });
$("#volume").addEventListener("input", event => send({type: "gain", value: Number(event.target.value) / 100}));
$("#panic").addEventListener("click", stop);
$("#power").addEventListener("click", async () => { try { if (context?.state === "running") { stop(); await context.suspend(); updatePower(); } else await startAudio(); } catch (error) { showError(error); } });
$("#midi").addEventListener("click", async () => {
  try {
    if (!navigator.requestMIDIAccess) throw new Error("Web MIDI is unavailable in this browser. The keyboard and pads still work.");
    midiAccess ??= await navigator.requestMIDIAccess({sysex: false}); await startAudio();
    const connect = () => { for (const input of midiAccess.inputs.values()) input.onmidimessage = event => { if (event.data.length === 3) send({type: "midi", bytes: [...event.data]}); }; $("#midi").textContent = midiAccess.inputs.size ? `MIDI · ${midiAccess.inputs.size}` : "MIDI · no device"; }; connect(); midiAccess.onstatechange = connect;
  } catch (error) { showError(error); }
});
updateControls(); updateKeys(); document.body.dataset.parameterCount = parameters.length;
try {
  const response = await fetch(new URL("./rustias.wasm", import.meta.url)); if (!response.ok) throw new Error(`Could not load the Rust engine (${response.status}).`);
  module = await WebAssembly.compile(await response.arrayBuffer()); $("#power").disabled = false; updatePower(); document.body.dataset.engine = "ready";
} catch (error) { $("#status").textContent = "Engine unavailable"; showError(error); }
