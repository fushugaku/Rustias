// Capture reference PCM from the actual browser build for the native DSP check.
// No AudioContext, audio device, playback or user data is involved.
import fs from 'node:fs/promises';
import {setEffectCatalog,defaultEffect} from '../web/effects.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';
const root=new URL('../',import.meta.url);
setEffectCatalog(JSON.parse(await fs.readFile(new URL('dist/effects.json',root),'utf8')));
const module=await WebAssembly.compile(await fs.readFile(new URL('dist/rustias.wasm',root)));
const cases=[];
let api,operations;
function call(name,...args){const value=api[name](...args);operations.push({call:name,args,...(typeof value==='number'?{expected:value}:{})});return value;}
function pointer(name,...args){const value=api[name](...args);operations.push({call:name,args,buffer:true});return value;}
function control(id,value,t=0){if(call('rustias_control',t,id,value)!==1)throw new Error(`Invalid fixture control ${id}/${value}`);}
function json(name,args,value){const bytes=new TextEncoder().encode(JSON.stringify(value));call('rustias_preset_reserve',bytes.length);const p=pointer('rustias_preset_buffer');new Uint8Array(api.memory.buffer,p,bytes.length).set(bytes);operations.push({write:[...bytes]});if(call(name,...args,bytes.length)!==1)throw new Error('Invalid fixture JSON');}
function pcm(name,args,values){const p=pointer(name,...args,values.length);new Float32Array(api.memory.buffer,p,values.length).set(values);operations.push({floats:[...values]});}
function render(blocks=20){const expected=[];for(let b=0;b<blocks;b++){const p=api.rustias_render();expected.push(...new Float32Array(api.memory.buffer,p,256));}operations.push({render:blocks,expected});}
function snapshot(){const size=api.rustias_save(),p=api.rustias_preset_buffer();operations.push({snapshot:JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer,p,size)))});}
function scenario(name,setup){api=new WebAssembly.Instance(module,{}).exports;operations=[];call('rustias_init');call('rustias_voice_capacity');call('rustias_timbre_capacity');call('rustias_parameter_count');setup();render();snapshot();cases.push({name,operations});}
function source({third=false,wave=0,mode=0,semi=64,fine=64,interaction=0,reference=0,t=0}={}){
  control(71,1,t);for(const [id,value]of [[17,third?0:127],[18,0],[19,third?127:0],[9,127],[3,0],[4,127],[5,127]])control(id,value,t);
  if(third)for(const [id,value]of [[171,wave],[172,mode],[173,60],[174,60],[175,semi],[176,fine],[177,interaction],[178,reference]])control(id,value,t);
  else for(const [id,value]of [[0,wave],[10,mode],[11,60],[12,60]])control(id,value,t);
}
for(const third of [false,true])for(let mode=0;mode<4;mode++)for(let wave=0;wave<(mode===0?6:4);wave++)scenario(`OSC ${third?3:1} wave ${wave} mode ${mode}`,()=>{source({third,wave,mode});call('rustias_note',0,69,100);render();if(third){control(15,76);control(16,127);control(13,2);render();control(175,76);control(176,127);render();}});
for(let interaction=1;interaction<4;interaction++)for(let reference=0;reference<3;reference++)scenario(`OSC 3 Ring/Sync ${interaction}/${reference}`,()=>{source({third:true,wave:3,semi:59,interaction,reference});control(0,3);control(13,1);control(14,3);control(15,71);call('rustias_note',0,69,100);});
scenario('Eight timbres and 128-voice allocation',()=>{for(let t=0;t<8;t++){source({t,third:t%2===1,wave:3});control(7,20,t);for(let n=40;n<56;n++)call('rustias_note',t,n,100);}call('rustias_voices');render(3);call('rustias_midi',0xb7,123,0);render(3);call('rustias_stop');});
scenario('Modular OSC 3 with live control and filters',()=>{source({third:true,wave:0});const circuit=defaultCircuit();circuit.enabled=true;json('rustias_circuit',[0],audioCircuit(circuit));call('rustias_note',0,69,100);render();control(173,100);control(175,71);render();circuit.enabled=false;json('rustias_circuit',[0],audioCircuit(circuit));});
const sample=Float32Array.from({length:4800},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));
scenario('Kit PCM with amplifier, filter and drum gain',()=>{control(140,1);pcm('rustias_sample_buffer',[0],sample);call('rustias_sample_commit',0,sample.length,2);call('rustias_drum_control',0,9,100);call('rustias_drum_control',0,114,32);call('rustias_drum_gain',12);call('rustias_drum_pad',0,100);render();call('rustias_drum_control',0,9,0);render();call('rustias_drum_pad',0,0);});
scenario('Sample library in timbre eight',()=>{control(71,1,7);control(141,7);control(140,1);pcm('rustias_library_sample_buffer',[77],sample);call('rustias_library_sample_commit',77,sample.length);call('rustias_library_profile',77,77,7,2);call('rustias_library_control',77,9,110);call('rustias_library_sync');call('rustias_library_note',7,77,100);render();call('rustias_library_control',77,9,0);call('rustias_library_sync');});
for(let kind=1;kind<=30;kind++)scenario(`Native FX ${kind}`,()=>{source({wave:3});json('rustias_effect',[0],defaultEffect(kind));json('rustias_effect',[17],defaultEffect(kind,true));json('rustias_effect_chain',[0],[{...defaultEffect(kind,true),id:'native-chain'}]);call('rustias_note',0,69,100);render();control(89,1350);render();call('rustias_note',0,69,0);});
await fs.mkdir(new URL('output/plugin/',root),{recursive:true});
await fs.writeFile(new URL('output/plugin/native-parity.json',root),JSON.stringify({version:1,cases}));
console.log(`Captured ${cases.length} browser PCM/state scenarios for native parity.`);
