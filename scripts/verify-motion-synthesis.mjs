// Real WASM only, rendered into memory. This script never opens an audio device.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {webParameters} from '../web/parameters.js';
import {emptyModulation,newModulationLane,validateModulation} from '../web/modulation.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
const module=await WebAssembly.compile(fs.readFileSync(process.argv[2]??'dist/rustias.wasm'));
const api=new WebAssembly.Instance(module,{}).exports;
const parameters=webParameters(JSON.parse(fs.readFileSync('crates/radias-synth-infrastructure/src/parameters.json')));
const c=(id,v,t=0)=>assert.equal(api.rustias_control(t,id,v),1,`parameter ${id}=${v}`);
const rms=a=>Math.sqrt(a.reduce((s,x)=>s+x*x,0)/a.length);
const diff=(a,b)=>rms(a.map((v,i)=>v-b[i]));
function render(n=24000){const out=[];for(let i=0;i<n;i+=128){const p=api.rustias_render(),a=new Float32Array(api.memory.buffer,p,256);for(let j=0;j<256;j+=2){assert.ok(Number.isFinite(a[j])&&Math.abs(a[j])<=1);out.push(a[j]);}}return out;}
function jsonCall(method,value,...args){const bytes=Buffer.from(JSON.stringify(value));assert.ok(bytes.length<api.rustias_preset_capacity());new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),bytes.length).set(bytes);return api[method](...args,bytes.length);}
function save(){const n=api.rustias_save();return JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),n)));}
function setup(options={}){api.rustias_init();for(const [id,value]of [[17,0],[18,0],[19,0],[187,127],[179,3],[180,0],[181,0],[182,64],[9,127],[3,0],[4,127],[5,127],...Object.entries(options).map(([id,v])=>[+id,v])])c(id,value);}
function sound(options={}){setup(options);api.rustias_note(0,69,100);return render();}
function frequency(a){let count=0;for(let i=4001;i<a.length;i++)if(a[i]>=0&&a[i-1]<0)count++;return count*48000/(a.length-4000);}

assert.equal(api.rustias_parameter_count(),207);
assert.ok(Math.abs(frequency(sound())-440)<5,'OSC4 is independently tuned to A4');
assert.ok(Math.abs(frequency(sound({183:76}))-880)<5,'OSC4 semitone');
assert.ok(Math.abs(frequency(sound({184:127}))-466.16)<5,'OSC4 fine');
const clean=sound();for(const patch of [{15:76},{16:127},{13:2},{175:76},{176:127}])assert.ok(diff(clean,sound(patch))<1e-8,'OSC2/3 tuning and wave never retune normal OSC4');
assert.ok(diff(clean,sound({179:0}))>.001,'OSC4 waveform selects its own generator');
for(const mode of [0,1,2,3,4])assert.ok(rms(sound({180:mode,181:75}))>.001,`OSC4 mode ${mode} renders`);
const folded=sound({180:4,181:20});assert.ok(diff(folded,sound({180:4,181:100}))>.001,'Fold changes harmonics');
assert.ok(diff(folded,sound({180:4,181:20,182:100}))>.001,'Bias changes folding');
const shapes=[];for(let shape=0;shape<5;shape++){const a=sound({180:4,181:80,188:shape});assert.ok(rms(a)>.001);assert.ok(Math.abs(a.slice(4000).reduce((s,x)=>s+x,0)/(a.length-4000))<.05,'Wavefolder rejects DC');shapes.push(a);}
for(let i=1;i<shapes.length;i++)assert.ok(diff(shapes[0],shapes[i])>.0001,'Every shaper family changes audio');
for(const interaction of [1,2,3])for(let reference=0;reference<4;reference++)assert.ok(diff(clean,sound({183:59,185:interaction,186:reference}))>.001,`OSC4 Ring/Sync ${interaction}/${reference}`);
assert.ok(diff(sound({183:59,185:1,186:1,13:1,14:1}),sound({183:59,185:1,186:2,13:1,14:1}))>.001,'OSC4 distinguishes OSC2 before Ring from its processed output');
setup();api.rustias_note(0,69,100);render(6000);c(183,76);assert.equal(api.rustias_voices(),1);assert.ok(Math.abs(frequency(render())-880)<5,'Held OSC4 responds without retriggering');

const graphs=defaultCircuit();graphs.enabled=true;
setup();assert.equal(jsonCall('rustias_circuit',audioCircuit(graphs),0),1);api.rustias_note(0,69,100);assert.ok(Math.abs(frequency(render())-440)<5,'OSC4 is wired into the default Build mixer');
const dry=sound({187:0,17:80});assert.ok(diff(dry,sound({187:0,17:80,180:4,181:120}))<1e-8,'Muted OSC4 preserves existing DSP');
for(let model=1;model<=17;model++){
  const first=sound({187:0,17:100,0:0,9:0,1:65,2:30,189:model});assert.ok(rms(first)>.00001,`Filter1 model ${model}`);
  const second=sound({187:0,17:100,0:0,20:1,22:65,23:30,190:model});assert.ok(rms(second)>.00001,`Filter2 model ${model}`);
  assert.ok(diff(first,sound({187:0,17:100,0:0,9:0,1:100,2:30,189:model}))>.0001,`Model ${model} cutoff`);
}
assert.ok(diff(sound({189:1,1:65,191:0}),sound({189:1,1:65,191:100}))>.001,'Filter Drive');
for(const model of [5,6])assert.ok(diff(sound({189:model,1:70,193:0}),sound({189:model,1:70,193:127}))>.001,'SEM/Multi Filter morph');
assert.ok(diff(sound({20:1,21:3,190:1,22:30}),sound({20:1,21:3,190:1,22:100}))>.001,'A new Filter2 model keeps Cutoff working when the saved RADIAS type is Comb');
for(let model=1;model<=17;model++)for(const cutoff of [0,64,127])for(const resonance of [0,127]){setup({189:model,1:cutoff,2:resonance,191:127});api.rustias_note(0,90,100);render(2048);}
for(const destination of [52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68]){
  setup({180:4,181:45,187:70,189:6,190:6,20:1,1:75,22:85});c(90,5);c(91,destination);c(92,100);api.rustias_note(0,69,100);assert.ok(rms(render(6000))>.000001,`Expanded virtual patch ${destination} is valid and finite`);
}

// A second note must get a fresh cursor without resetting the first note.
setup({187:0,17:60});
const config=emptyModulation(),lane=newModulationLane('poly-pitch');Object.assign(lane,{playback:'Poly',target:{kind:'synth',timbre:0,parameter:53},length:4,resolution:'1/16',values:Array.from({length:128},(_,i)=>i%4*3)});config.tracks[0]=[lane];
assert.equal(jsonCall('rustias_poly_motion',validateModulation(config)),1);
api.rustias_note(0,60,100);render(12288);assert.equal(api.rustias_poly_motion_position(0,0),2);
api.rustias_note(0,67,100);render(128);assert.equal(api.rustias_poly_motion_position(0,0),2,'New note leaves the first voice at its own step');
api.rustias_note(0,60,0);render(128);assert.equal(api.rustias_poly_motion_position(0,0),0,'The second voice started at step one');
assert.equal(api.rustias_value(0,53),64,'Polyphonic motion never overwrites patch values');
setup({187:0,17:60,3:0});api.rustias_note(0,60,100);const fastAttack=render(2048);
setup({187:0,17:60,3:0});const attackConfig=emptyModulation(),attackLane=newModulationLane('poly-attack');Object.assign(attackLane,{playback:'Poly',target:{kind:'synth',timbre:0,parameter:3},length:1,values:Array(128).fill(60)});attackConfig.tracks[0]=[attackLane];assert.equal(jsonCall('rustias_poly_motion',attackConfig),1);api.rustias_note(0,60,100);const slowAttack=render(2048);assert.ok(rms(slowAttack.slice(0,512))<rms(fastAttack.slice(0,512))*.25,'The first Poly step reaches EG2 Attack before the note starts');
for(const direction of ['Forward','Reverse','Alt1','Alt2','Random']){
  setup({187:0,17:50});const oneShot=emptyModulation(),line=newModulationLane('one-shot');Object.assign(line,{playback:'Poly',target:{kind:'synth',timbre:0,parameter:53},length:3,resolution:'1/48',direction,runMode:'OneShot',values:Array.from({length:128},(_,i)=>i%3*4)});oneShot.tracks[0]=[line];assert.equal(jsonCall('rustias_poly_motion',oneShot),1);api.rustias_note(0,60,100);render(24000);const final=api.rustias_poly_motion_position(0,0);if(direction!=='Random')assert.equal(final,direction==='Forward'?2:0,'Poly OneShot stops at its actual endpoint '+direction);render(12000);assert.equal(api.rustias_poly_motion_position(0,0),final,'Poly OneShot holds '+direction);
}
setup({187:0,17:60});assert.equal(jsonCall('rustias_poly_motion',config),1);
lane.loopStart=1;lane.loopEnd=2;lane.durations=Array(128).fill(1);lane.durations[1]=2;lane.probabilities=Array(128).fill(100);lane.curves=Array(128).fill('Linear');
api.rustias_stop();assert.equal(jsonCall('rustias_poly_motion',validateModulation(config)),1);api.rustias_note(0,60,100);render(6000);assert.equal(api.rustias_poly_motion_position(0,0),1,'Loop starts on its selected first step and respects duration');render(8000);assert.equal(api.rustias_poly_motion_position(0,0),2);
const before=save(),program=normalizeProgram(before,parameters);program.modulation=validateModulation(config);program.engine.timbres[0][187]=93;program.engine.timbres[0][189]=6;
const moved=applyTimbre(program,1,captureTimbre(program,0,parameters),parameters);assert.equal(moved.engine.timbres[1][187],93);assert.equal(moved.engine.timbres[1][189],6);assert.equal(moved.modulation.tracks[1][0].playback,'Poly');assert.equal(moved.modulation.tracks[1][0].target.timbre,1);assert.deepEqual(moved.modulation.tracks[1][0].durations,lane.durations);
const old=structuredClone(before);for(const values of [...old.timbres,...old.drums])values.length=179;assert.equal(jsonCall('rustias_load',old),1);const upgraded=save();assert.deepEqual(upgraded.timbres[0].slice(0,179),old.timbres[0]);assert.deepEqual(upgraded.timbres[0].slice(179),parameters.slice(179).map(p=>p.default));
assert.equal(upgraded.timbres[0][187],0);assert.equal(upgraded.timbres[0][189],0);assert.equal(upgraded.timbres[0][190],0);

// Native drum sources and PCM profiles retain their own model/OSC4 settings.
function drum(semi,osc=4){api.rustias_init();c(140,1);for(const [id,v]of [[17,osc===1?127:0],[0,3],[11,0],[12,0],[18,0],[19,0],[187,osc===4?127:0],[179,3],[180,0],[181,0],[182,0],[183,semi],[3,0],[4,127],[5,127],[9,127]])assert.equal(api.rustias_drum_control(0,id,v),1);api.rustias_drum_pad(0,100);return render();}
const drumRoot=frequency(drum(64)),drumOctave=frequency(drum(76));const nativeDrumRoot=frequency(drum(64,1));assert.ok(Math.abs(drumRoot-nativeDrumRoot)<5,'Drum OSC4 follows its native instrument root');assert.ok(Math.abs(drumOctave-drumRoot*2)<6,'Drum OSC4 keeps independent tuning');
function sample(model,cutoff,poly=false){api.rustias_init();c(140,1);const pcm=Float32Array.from({length:12000},(_,i)=>.25*Math.sin(i*2*Math.PI*440/48000)+.2*Math.sin(i*2*Math.PI*4000/48000));const p=api.rustias_sample_buffer(0,pcm.length);new Float32Array(api.memory.buffer,p,pcm.length).set(pcm);assert.equal(api.rustias_sample_commit(0,pcm.length,2),1);for(const [id,v]of [[189,model],[1,cutoff],[3,0],[4,127],[5,127],[9,0]])assert.equal(api.rustias_drum_control(0,id,v),1);if(poly){const config=emptyModulation(),lane=newModulationLane('sample-motion');Object.assign(lane,{playback:'Poly',target:{kind:'synth',timbre:0,parameter:1},length:2,resolution:'1/48',values:Array.from({length:128},(_,i)=>i%2?40:-40)});config.tracks[0]=[lane];assert.equal(jsonCall('rustias_poly_motion',config),1);}api.rustias_drum_pad(0,100);return render();}
assert.ok(diff(sample(1,30),sample(1,100))>.001,'New models process uploaded PCM');assert.ok(diff(sample(1,65),sample(1,65,true))>.001,'PCM voices execute their own polyphonic motion');
console.log('OSC4, 5 wavefolders, independent tuning, Ring/Sync, Build, 17 filter models in both slots, modulation, per-voice motion and program migration: passed.');
