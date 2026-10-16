import assert from 'node:assert/strict';
import {writeWasmJson} from '../web/wasm-json.js';
import {defaultEffect} from '../web/effects.js';
import {MASTER_EFFECT_SLOT,effectUsesMaster} from '../web/limits.js';

const delayKinds=new Set([11,12,13,14,15,16,17,18,19,20,21,22,26,27,28,29]);
const ringBytes=131072*2*4;
function call(api,name,args,value){assert.equal(api[name](...args,writeWasmJson(api,value)),1,name);}
function effect(api,slot,kind,enabled=kind!==0){
  const program={...defaultEffect(kind,effectUsesMaster(slot)),enabled};
  call(api,'rustias_effect',[slot],program);return program;
}
function render(api,blocks=1){
  const result=[];
  for(let i=0;i<blocks;i++){
    const pointer=api.rustias_render();result.push(Buffer.from(new Uint8Array(api.memory.buffer,pointer,256*4)));
  }
  return Buffer.concat(result);
}
export function verifyRuntimeCaches({module}){
  const create=()=>{const api=new WebAssembly.Instance(module,{}).exports;api.rustias_init();return api;};
  const api=create();
  assert.equal(typeof api.rustias_effect_delay_bytes,'function');
  assert.equal(api.rustias_effect_delay_bytes(),0,'An empty browser program owns no FX rings');
  for(const slot of [0,1,MASTER_EFFECT_SLOT,17,18,32])for(let kind=1;kind<=30;kind++){
    effect(api,slot,kind);
    assert.equal(api.rustias_effect_delay_bytes(),delayKinds.has(kind)?ringBytes:0,`scratch ${slot}/${kind}`);
    render(api);
    effect(api,slot,kind,false);
    assert.equal(api.rustias_effect_delay_bytes(),delayKinds.has(kind)?ringBytes:0,'Bypass keeps owned history');
    effect(api,slot,0);
    assert.equal(api.rustias_effect_delay_bytes(),0,'Removing a delay type releases its ring');
  }
  const chain=[6,14,11,1,9,23,26,29,30].map((kind,i)=>({...defaultEffect(kind,true),id:'cached-'+i}));
  call(api,'rustias_effect_chain',[0],chain);
  assert.equal(api.rustias_effect_delay_bytes(),4*ringBytes,'Only delay algorithms in added chains own rings');
  const n=api.rustias_save(),saved=JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),n)));
  call(api,'rustias_load',[],saved);
  assert.equal(api.rustias_effect_delay_bytes(),4*ringBytes,'Complete program restore uses compact scratch');
  call(api,'rustias_effect_chain',[0],[]);
  assert.equal(api.rustias_effect_delay_bytes(),0);

  // The first insert's kind controls pairing even when that insert is bypassed.
  const a=create(),b=create();
  for(const engine of [a,b]){
    effect(engine,0,29,false);
    const master=effect(engine,MASTER_EFFECT_SLOT,24);master.parameters[0]=0;
    call(engine,'rustias_effect',[MASTER_EFFECT_SLOT],master);
    engine.rustias_note(0,69,100);
  }
  effect(a,1,14);
  assert.deepEqual(render(a,32),render(b,32),'A paired odd insert remains skipped');
  effect(a,0,0);effect(b,0,0);effect(b,1,14);
  assert.deepEqual(render(a,96),render(b,96),'Changing the first kind immediately rebuilds odd-insert routing');

  // A bypassed delay freezes both its history and its cursor. A short one-shot
  // produces exact silence at the input before comparing with a paused clock.
  const left=create(),right=create();
  for(const engine of [left,right]){
    engine.rustias_control(0,140,1);
    const pointer=engine.rustias_sample_buffer(0,256);
    new Float32Array(engine.memory.buffer,pointer,256).fill(.2);
    assert.equal(engine.rustias_sample_commit(0,256,0),1);
    const fx=defaultEffect(14,true);fx.parameters[0]=100;fx.parameters[2]=0;fx.parameters[4]=10;fx.parameters[5]=10;
    call(engine,'rustias_effect_chain',[0],[{...fx,id:'tail'}]);
    engine.rustias_drum_pad(0,100);
  }
  assert.deepEqual(render(left,80),render(right,80));
  assert.equal(left.rustias_voices(),0);
  const fx=defaultEffect(14,true);fx.parameters[0]=100;fx.parameters[2]=0;fx.parameters[4]=10;fx.parameters[5]=10;
  call(left,'rustias_chain_effect',[0],{...fx,id:'tail',enabled:false});render(left,20);
  assert.equal(left.rustias_effect_delay_bytes(),ringBytes);
  call(left,'rustias_chain_effect',[0],{...fx,id:'tail'});
  const resumed=render(left,40),paused=render(right,40);
  assert.deepEqual(resumed,paused,'Re-enabling a skipped added delay resumes its exact tail');
  assert.ok(new Float32Array(resumed.buffer,resumed.byteOffset,resumed.length/4).some(value=>value!==0),'Tail comparison must contain audio');
  return 'Runtime caches: compact FX scratch (0 bytes for empty/EQ/dynamics; full rings for delay algorithms), program restore, all 30 types, paired/bypass routing and exact retained tails';
}
