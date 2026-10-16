// Offline before/after WASM qualification. Never opens an audio device.
// node scripts/compare-render-performance.mjs baseline.wasm products.wasm final.wasm
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {writeWasmJson} from '../web/wasm-json.js';
import {readEffectCatalog} from './verify-effects.mjs';
import {defaultEffect} from '../web/effects.js';
import {webParameters} from '../web/parameters.js';

const args=process.argv.slice(2),paths=args.filter(arg=>!arg.startsWith('--'));
const verifyOnly=args.includes('--verify-only');
const rounds=Number(args.find(arg=>arg.startsWith('--rounds='))?.split('=')[1]??7);
assert.ok(Number.isInteger(rounds)&&rounds>=1&&rounds<=21,'Rounds must be 1…21.');
const selected=args.find(arg=>arg.startsWith('--bench='))?.slice('--bench='.length).split(',');
assert.ok(paths.length>=2,'Provide a baseline and one or more candidate WASM files.');
const builds=await Promise.all(paths.map(async path=>{
  const bytes=fs.readFileSync(path),module=await WebAssembly.compile(bytes);
  return {path,sha256:createHash('sha256').update(bytes).digest('hex'),bytes:bytes.length,api:new WebAssembly.Instance(module,{}).exports};
}));
readEffectCatalog(builds[0].api);
function control(api,id,value,t=0){
  assert.equal(id>=171?api.rustias_osc3_control(t,id-171,value):api.rustias_control(t,id,value),1,`control ${t}/${id}=${value}`);
}
function jsonCall(api,name,args,value){assert.equal(api[name](...args,writeWasmJson(api,value)),1,name);}
function fixture(api,scene){
  api.rustias_init();
  for(let t=0;t<8;t++){
    control(api,71,1,t);for(const [id,value]of [[3,0],[4,127],[5,127],[6,15]])control(api,id,value,t);
    for(const [id,value]of scene.controls??[])control(api,id,value,t);
  }
  if(scene.effect){const [slot,kind]=scene.effect;jsonCall(api,'rustias_effect',[slot],defaultEffect(kind,slot===16));}
  if(scene.pairedIgnored){
    jsonCall(api,'rustias_effect',[0],{...defaultEffect(29,false),enabled:false});
    jsonCall(api,'rustias_effect',[1],defaultEffect(14,false));
    api.rustias_drum_gain(24);
  }
  if(scene.chain)jsonCall(api,'rustias_effect_chain',[0],scene.chain.map((kind,i)=>({...defaultEffect(kind,true),id:'chain-'+i})));
  if(scene.pcm!=null){
    control(api,140,1);const length=4800,pointer=api.rustias_sample_buffer(0,length);
    const samples=new Float32Array(api.memory.buffer,pointer,length);
    for(let i=0;i<length;i++)samples[i]=.25*Math.sin(i*2*Math.PI*220/48000);
    assert.equal(api.rustias_sample_commit(0,length,scene.pcm),1);
    assert.equal(api.rustias_drum_pad(0,100),1);
    if(scene.pairedIgnored)for(let t=1;t<8;t++)api.rustias_note(t,40+t,127);
  }else for(let i=0;i<scene.voices;i++)assert.equal(api.rustias_note(i%8,36+Math.floor(i/8),100),1);
}
const scenes=[{name:'idle',voices:0}];
function add(name,controls=[],extra={}){scenes.push({name,controls,voices:3,...extra});}
for(let mode=0;mode<4;mode++)for(let wave=0;wave<(mode===0?6:4);wave++){
  add(`osc1-${wave}-${mode}`,[[0,wave],[10,mode],[11,81],[12,55]]);
  add(`osc3-${wave}-${mode}`,[[17,0],[19,127],[171,wave],[172,mode],[173,81],[174,55],[175,71],[176,41]]);
}
for(let mode=0;mode<4;mode++)add('osc2-'+mode,[[18,127],[14,mode],[15,71],[16,41]]);
for(let route=0;route<4;route++)for(let kind=0;kind<4;kind++)add(`filters-${route}-${kind}`,[[20,route],[21,kind],[22,75],[23,43]]);
for(let ws=0;ws<12;ws++)for(let position=0;position<2;position++)add(`ws-${ws}-${position}`,[[29,ws===0?1:2],[154,Math.max(0,ws-1)],[30,position],[31,87]]);
for(let kind=1;kind<=30;kind++)for(const slot of [0,16])add(`fx-${slot}-${kind}`,[],{effect:[slot,kind]});
const parameters=webParameters(JSON.parse(fs.readFileSync(new URL('../crates/radias-synth-infrastructure/src/parameters.json',import.meta.url))));
for(const destination of parameters.find(p=>p.id===91).values)add('patch-'+destination,[[90,3],[91,destination],[92,104],[73,2],[74,91]]);
for(const mode of [0,1,2])add('pcm-'+mode,[[20,1],[21,3],[22,76],[23,38]],{voices:0,pcm:mode,chain:[6,14]});
add('extra-fx',[],{chain:[6,14,7,11]});
add('eight-timbres-128',[],{voices:128});
add('mono-retrigger',[[62,0]],{voices:16});
add('unison',[[67,1],[68,4],[69,43],[70,99]],{voices:16});
add('paired-insert-clipped-pcm',[[7,127]],{voices:0,pcm:0,pairedIgnored:true});

let comparedFrames=0;
const audioHash=createHash('sha256');
for(const scene of scenes){
  for(const build of builds)fixture(build.api,scene);
  for(let block=0;block<96;block++){
    for(const {api}of builds){
      if(block===8)for(let t=0;t<8;t++)control(api,1,35,t);
      if(block===16)for(let t=0;t<8;t++)control(api,11,96,t);
      if(block===20)for(let t=0;t<8;t++)control(api,66,1,t);
      if(block===24){
        if(scene.pcm!=null)api.rustias_drum_pad(0,0);
        else for(let i=0;i<scene.voices;i++)api.rustias_note(i%8,36+Math.floor(i/8),0);
      }
      if(block===32)for(let t=0;t<8;t++)control(api,66,0,t);
      if(block===48&&scene.voices)api.rustias_note(0,36,101);
      if(block===64)api.rustias_note(0,36,0);
    }
    const buffers=builds.map(({api})=>{const pointer=api.rustias_render();return Buffer.from(new Uint8Array(api.memory.buffer,pointer,256*4));});
    for(let i=1;i<buffers.length;i++)assert.ok(buffers[0].equals(buffers[i]),`${scene.name} block ${block}: ${builds[i].path}`);
    for(const value of new Float32Array(buffers[0].buffer,buffers[0].byteOffset,256))assert.ok(Number.isFinite(value));
    audioHash.update(buffers[0]);comparedFrames+=128;
  }
}
process.stderr.write(`Exact PCM: ${scenes.length} scenes, ${comparedFrames} stereo frames. ${verifyOnly?'Verification only.':'Timing separately.'}\n`);
const benchmark=[
  {name:'idle',voices:0},{name:'saw1',voices:1},{name:'saw8',voices:8},
  {name:'saw48',voices:48},{name:'saw128',voices:128},
  {name:'comb24',voices:24,controls:[[20,1],[21,3],[22,75],[23,40]]},
  {name:'shaper24',voices:24,controls:[[29,2],[154,8],[31,87],[30,1]]},
  {name:'fx-chain',voices:8,chain:[6,14,7,11]},
  {name:'osc3-sine128',voices:128,controls:[[17,0],[19,127],[171,3]]},
  {name:'osc3-unison48',voices:48,controls:[[17,0],[19,127],[171,0],[172,2],[173,70],[174,40]]},
  {name:'reverb',voices:8,chain:[11]},
  {name:'delay',voices:8,chain:[14]},
  {name:'tube',voices:8,chain:[9]},
  {name:'early',voices:8,chain:[12]},
];
const timings=[];
if(selected)for(const name of selected)assert.ok(benchmark.some(scene=>scene.name===name),`Unknown benchmark: ${name}`);
for(const scene of verifyOnly?[]:benchmark.filter(scene=>!selected||selected.includes(scene.name))){
  const samples=builds.map(()=>[]);
  for(let round=0;round<rounds;round++){
    // Alternate order to avoid giving one build a systematic thermal advantage.
    const order=builds.map((_,i)=>i);if(round%2)order.reverse();
    for(const i of order){
      const api=builds[i].api;fixture(api,scene);
      for(let block=0;block<128;block++)api.rustias_render();
      const start=performance.now();
      for(let block=0;block<512;block++)api.rustias_render();
      samples[i].push(performance.now()-start);
    }
  }
  const medians=samples.map(values=>[...values].sort((a,b)=>a-b)[Math.floor(rounds/2)]);
  timings.push({scene:scene.name,voices:scene.voices,frames:65536,median_ms:medians,saved_percent:medians.map(value=>(1-value/medians[0])*100),samples_ms:samples});
  process.stderr.write(`${scene.name}: ${medians.map(x=>x.toFixed(2)).join(' / ')} ms\n`);
}
console.log(JSON.stringify({passed:true,platform:process.platform,arch:process.arch,node:process.version,scenes:scenes.length,comparedFrames,audio_sha256:audioHash.digest('hex'),builds:builds.map(({api,...build})=>build),timings},null,2));
