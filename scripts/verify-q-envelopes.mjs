// Offline verification of the real WASM. Never creates an audio device.
import assert from 'node:assert/strict';
import {writeWasmJson} from '../web/wasm-json.js';
import {extendValues} from '../web/parameters.js';
import {ENVELOPES,qEnvelopeSeconds} from '../web/envelopes.js';
import {emptyModulation,newModulationLane,validateModulation} from '../web/modulation.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';

export function verifyQEnvelopes({module,parameters}){
  const api=new WebAssembly.Instance(module,{}).exports;
  const c=(id,value,t=0)=>assert.equal(api.rustias_control(t,id,value),1,`Q parameter ${t}/${id}`);
  const json=(name,value,...args)=>api[name](...args,writeWasmJson(api,value));
  function render(n=24000){const out=[];for(let i=0;i<n;i+=128){const p=api.rustias_render(),a=new Float32Array(api.memory.buffer,p,256);for(let j=0;j<256;j+=2){assert.ok(Number.isFinite(a[j]));out.push(a[j]);}}return out;}
  const rms=a=>Math.sqrt(a.reduce((sum,x)=>sum+x*x,0)/a.length);
  const difference=(a,b)=>rms(a.map((x,i)=>x-b[i]));
  function save(){const n=api.rustias_save();return JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),n)));}
  function setup(mode,extra=[]){api.rustias_init();for(const [id,value]of [[0,3],[9,127],[3,0],[4,45],[5,90],[6,24],[199,mode],[200,100],[201,30],[202,24],...extra])c(id,value);}
  function sound(mode,extra=[],frames=12000){setup(mode,extra);api.rustias_note(0,69,100);return render(frames);}

  api.rustias_init();assert.equal(api.rustias_parameter_count(),parameters.length);
  for(const p of ENVELOPES)for(let t=0;t<8;t++)assert.equal(api.rustias_value(t,p.base),0,'Existing/new patches default to RADIAS');
  const original=save();
  for(let t=0;t<8;t++)for(const [i,p]of ENVELOPES.entries())c(p.base,1+(t+i)%5,t);
  const saved=save();assert.equal(json('rustias_load',saved),1);assert.deepEqual(save(),saved);
  const program=normalizeProgram(saved,parameters),copy=applyTimbre(program,1,captureTimbre(program,0,parameters),parameters);
  for(const p of ENVELOPES)assert.equal(copy.engine.timbres[1][p.base],saved.timbres[0][p.base],'Timbre copies retain every envelope type');
  const legacy=structuredClone(original);for(const v of [...legacy.timbres,...legacy.drums])v.length=195;
  assert.equal(json('rustias_load',legacy),1);
  for(const v of [...save().timbres,...save().drums])assert.deepEqual(v.slice(195),[0,127,64,64,0,127,64,64,0,127,64,64]);
  assert.deepEqual(extendValues(legacy.timbres[0],parameters),original.timbres[0]);
  const invalid=save();invalid.timbres[0][199]=6;assert.equal(json('rustias_load',invalid),0);

  const radias=sound(0,[],128),q=sound(1,[],128);
  assert.ok(rms(q)>rms(radias)*1.2,'Q zero Attack reaches the amplifier before the RADIAS minimum Attack');
  assert.equal(qEnvelopeSeconds(0),0);assert.ok(qEnvelopeSeconds(12)<.003);
  const full=sound(2,[[200,127]],256),low=sound(2,[[200,0]],256);
  assert.ok(rms(low)<rms(full)*.5,'Attack Level changes the actual Q peak');
  const sustainHigh=sound(2,[[202,100]]),sustainLow=sound(2,[[202,10]]);
  assert.ok(rms(sustainHigh.slice(6000))>rms(sustainLow.slice(6000))*2,'Sustain 2 is the held level');
  for(const mode of [1,2,3,4,5]){
    const audio=sound(mode);assert.ok(rms(audio)>.00001,`Q mode ${mode} produces audio`);
    if(mode===3)assert.equal(api.rustias_voices(),0,'One Shot completes while the key is still held');
    else {assert.equal(api.rustias_voices(),1);api.rustias_note(0,69,0);render(12000);assert.equal(api.rustias_voices(),0,`Q mode ${mode} retires after note off`);}
  }
  function oneShot(release){setup(3,[[3,40]]);api.rustias_note(0,69,100);const a=render(128);if(release)api.rustias_note(0,69,0);return [...a,...render(12000)];}
  assert.deepEqual(oneShot(false),oneShot(true),'A short gate cannot truncate Q One Shot Attack/Decay/Release');
  for(const mode of [4,5]){
    const audio=sound(mode,[],48000);assert.equal(api.rustias_voices(),1,'Loops continue while held');
    assert.ok(difference(audio.slice(0,12000),audio.slice(12000,24000))>.00001,'Loop modulation keeps moving');
    api.rustias_note(0,69,0);render(12000);assert.equal(api.rustias_voices(),0,'Loop release ends the voice');
  }
  setup(1);api.rustias_note(0,69,100);render(1024);
  for(const mode of [2,3,4,5,0,1]){c(199,mode);render(128);assert.equal(api.rustias_voices(),1,'Live mode changes preserve the voice');}
  api.rustias_note(0,69,0);render(24000);assert.equal(api.rustias_voices(),0);

  function aux(index,mode){
    const p=ENVELOPES[index];setup(1,[[9,0],[1,20],[5,127],[p.base,mode],[p.adsr[0],0],[p.adsr[1],30],[p.adsr[2],100],[p.adsr[3],30]]);
    if(index===0)c(25,127);
    else {const source=parameters[90].options.findIndex(s=>/EG\s*3/i.test(s));assert.ok(source>=0);c(90,parameters[90].values?.[source]??source);c(91,7);c(92,127);}
    api.rustias_note(0,69,100);return render();
  }
  for(const index of [0,2])assert.ok(difference(aux(index,1),aux(index,3))>.00001,`Q EG${index+1} reaches filter/modulation, independently of EG2`);

  setup(1);c(140,1);
  for(const [id,value]of [[3,0],[4,30],[5,90],[6,24],[199,3]])assert.equal(api.rustias_drum_control(0,id,value),1);
  api.rustias_drum_pad(0,100);render();assert.equal(api.rustias_voices(),0,'Synthetic drum instruments also use Q One Shot');

  const graph=defaultCircuit();graph.enabled=true;
  setup(3);assert.equal(json('rustias_circuit',audioCircuit(graph),0),1);api.rustias_note(0,69,100);render();assert.equal(api.rustias_voices(),0,'Build uses the selected amplifier envelope too');

  function sample(mode,poly=false){
    setup(1);c(140,1);
    for(const [id,value]of [[3,0],[4,45],[5,90],[6,24],[199,mode],[200,100],[201,30],[202,24]])assert.equal(api.rustias_drum_control(0,id,value),1);
    const pcm=Float32Array.from({length:2400},(_,i)=>.4*Math.sin(i*2*Math.PI*440/48000));
    const p=api.rustias_sample_buffer(0,pcm.length);new Float32Array(api.memory.buffer,p,pcm.length).set(pcm);assert.equal(api.rustias_sample_commit(0,pcm.length,2),1);
    if(poly){const config=emptyModulation(),lane=newModulationLane('q-decay');Object.assign(lane,{playback:'Poly',target:{kind:'synth',timbre:0,parameter:201},length:1,values:Array(128).fill(55)});config.tracks[0]=[lane];assert.equal(json('rustias_poly_motion',validateModulation(config)),1);}
    api.rustias_drum_pad(0,100);return render();
  }
  const sustain=sample(1),shot=sample(3);assert.ok(rms(sustain.slice(6000))>.00001);assert.ok(rms(shot.slice(6000))<.000001,'Q One Shot gates an uploaded looping PCM source');
  assert.ok(difference(sample(2),sample(2,true))>.00001,'Poly motion reaches Q Decay 2 on each PCM voice');
  return ['all five Q envelope modes per EG, immediate Attack, extended breakpoints, loops/short gates, live edits, legacy/program/timbre migration, Build and PCM/Poly modulation'];
}
