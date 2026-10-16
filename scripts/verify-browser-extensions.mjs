import {engineWrite,engineBytes,engineFloats} from '../web/engine-io.js';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {SequenceClock,emptySequence} from '../web/sequence.js';
import {ModulationAutomation,emptyModulation,newModulationLane,modulationKey} from '../web/modulation.js';
import {ModulationHost} from '../web/modulation-host.js';
import {emptyMacros,MacroBank} from '../web/macros.js';
import {emptyEffects,defaultEffect,validateEffects} from '../web/effects.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';
import {RecordingTap} from '../web/recording-tap.js';
import {MAX_TIMBRES,INITIAL_TIMBRES,OSC3_BASE,PARAMETER_COUNT,timbreArray} from '../web/limits.js';
import {validSampleSource} from '../web/sample-state.js';
import {writeWasmJson} from '../web/wasm-json.js';

export function verifyBrowserExtensions({module,parameters,workletSource,sequenceSource}){
  const target={kind:'sequence-gate',timbre:0},events=[],clock=new SequenceClock((t,n,v)=>events.push({t,n,v,frame:clock.frame}));
  const sequence=emptySequence();sequence.tracks.forEach(t=>t.enabled=false);sequence.tracks[0].enabled=true;sequence.tracks[0].steps[0]={notes:[60,64,67],samples:['custom:gate-test'],gate:75,velocity:100};
  const saved=structuredClone(sequence);clock.setConfig(sequence);clock.setGateOffset(0,-50);clock.play();for(let i=0;i<16;i++)clock.beforeRender();
  assert.equal(events.filter(e=>e.v>0).length,4);assert.deepEqual(events.filter(e=>e.v===0).map(e=>e.frame),[1536,1536,1536,1536],'Gate offset releases every chord note and gated sample together');
  assert.deepEqual(clock.config,saved,'Gate modulation never edits stored step values');
  clock.stop();events.length=0;clock.frame=0;clock.setGateOffset(0,0);clock.play();for(let i=0;i<8;i++)clock.beforeRender();clock.setGateOffset(0,25);assert.equal(clock.releases[0].frame,6000,'Gate can lengthen a sounding current step');for(let i=0;i<8;i++)clock.beforeRender();clock.setGateOffset(0,-60);clock.beforeRender();assert.equal(events.find(e=>e.v===0).frame,2048,'Shortening a sounding step past its elapsed time releases it at this block');
  clock.stop();clock.frame=0;events.length=0;clock.setGateOffset(0,-100);clock.play();clock.beforeRender();assert.equal(events.length,0,'Effective zero Gate mutes an ordinary step');
  const host=new ModulationHost({wasm:{},sequencer:clock}),automation=new ModulationAutomation(changes=>host.write(changes));const config=emptyModulation(),lane=newModulationLane('gate');lane.target=target;lane.values.fill(-50);config.tracks[0]=[lane];
  automation.setState({config,macros:emptyMacros(),targets:[{target,min:-100,max:100,base:0,available:true}]});automation.clock.play();automation.beforeRender();assert.equal(clock.gateMod[0],-50);assert.equal(clock.gateMod[1],0);automation.stop();assert.equal(clock.gateMod[0],0,'Stopping modulation restores the sequencer Gate base');

  const makeApi=()=>{const api=new WebAssembly.Instance(module,{}).exports;api.rustias_init();return api;};
  const read=api=>{const n=api.rustias_save();assert.ok(n>0);return JSON.parse(Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_preset_buffer(),n)));};
  const send=(api,method,t,value,accepted=1)=>assert.equal(api[method](t,writeWasmJson(api,value)),accepted,method);
  const control=(api,p,v,t=0)=>assert.equal(api.rustias_control(t,p,v),1);
  const render=(api,n=8192)=>{const values=[];for(let i=0;i<n;i+=128){const pointer=api.rustias_render();const pcm=new Float32Array(api.memory.buffer,pointer,256);for(let j=0;j<256;j+=2){assert.ok(Number.isFinite(pcm[j]));values.push(pcm[j]);}}return values;};
  const rms=a=>Math.sqrt(a.reduce((s,x)=>s+x*x,0)/a.length);
  function tone(api,osc=1,wave=0,mode=0,graph=false){for(const [p,v]of [[17,osc===1?127:0],[18,0],[19,osc===3?127:0],[0,wave],[10,mode],[171,wave],[172,mode],[9,127],[3,0],[4,127],[5,127],[6,0]])control(api,p,v);if(graph){const c=audioCircuit(defaultCircuit());c.enabled=true;send(api,'rustias_circuit',0,c);}api.rustias_note(0,69,100);}
  const api=makeApi(),gain=[];
  for(const graph of [false,true])for(const mode of [0,2,3])for(let wave=0;wave<4;wave++){
    api.rustias_init();tone(api,1,wave,mode,graph);const first=rms(render(api,24000).slice(4000));api.rustias_init();tone(api,3,wave,mode,graph);const third=rms(render(api,24000).slice(4000));
    assert.ok(Math.abs(third/first-1)<.04,`OSC 3 uses OSC 1 gain for wave ${wave}, mode ${mode}, routing ${graph}: ${third/first}`);if(!graph&&mode===0)gain.push(Number((20*Math.log10(third/first)).toFixed(3)));
  }
  api.rustias_init();const large=Array.from({length:600},(_,i)=>({...defaultEffect(0,true),id:'extra-'+i}));
  send(api,'rustias_effect_chain',7,large);assert.ok(api.rustias_preset_capacity()>65536,'No fixed effect-count cap is imposed by the former 64 KiB JSON buffer');const complete=read(api);assert.equal(complete.effects.chains[7].length,600);assert.ok(api.rustias_load(writeWasmJson(api,complete)));assert.deepEqual(read(api),complete);
  const bad=structuredClone(large);bad[1].id=bad[0].id;send(api,'rustias_effect_chain',7,bad,0);assert.deepEqual(read(api).effects.chains[7],large,'Malformed chains leave the live rack intact');
  api.rustias_init();const chain=Array.from({length:30},(_,i)=>({...defaultEffect(i+1,true),enabled:false,id:'kind-'+(i+1)}));send(api,'rustias_effect_chain',0,chain);assert.equal(read(api).effects.chains[0].length,30,'Every Master-capable algorithm can be instantiated on a timbre');
  const eq={...defaultEffect(6,true),id:'eq'},dist={...defaultEffect(7,true),id:'dist'};eq.parameters[0]=100;eq.parameters[6]=88;dist.parameters[0]=100;dist.parameters[1]=100;
  const sound=entries=>{api.rustias_init();send(api,'rustias_effect_chain',0,entries);tone(api);return render(api,12000).slice(4000);};
  const forward=sound([eq,dist]),reverse=sound([dist,eq]);assert.ok(rms(forward.map((v,i)=>v-reverse[i]))>.00001,'Reordering effects changes the real serial audio path');
  api.rustias_init();send(api,'rustias_effect_chain',0,[eq]);control(api,9,127,1);api.rustias_note(1,69,100);const other=render(api);api.rustias_init();control(api,9,127,1);api.rustias_note(1,69,100);assert.deepEqual(render(api),other,'Added timbre FX do not affect another timbre');
  const delay={...defaultEffect(14,true),id:'delay'},bypass={...defaultEffect(0,true),id:'bypass'};
  const a=makeApi(),b=makeApi();for(const instance of [a,b]){send(instance,'rustias_effect_chain',0,[delay,bypass]);tone(instance);render(instance,16000);instance.rustias_note(0,69,0);}
  send(a,'rustias_effect_chain',0,[bypass,delay]);assert.deepEqual(render(a,8192),render(b,8192),'Stable effect IDs retain delay history and oscillator phase on reorder');
  const macroTarget={kind:'chain-effect',timbre:0,id:'delay',effectKind:14,parameter:0};let wet=delay.parameters[0];const bank=new MacroBank({resolve:()=>({min:0,max:100,read:()=>wet,label:'Delay wet',available:true}),write:changes=>{for(const change of changes){wet=change.value;send(a,'rustias_chain_effect',0,{...delay,parameters:delay.parameters.map((v,i)=>i===0?wet:v)});}}});bank.assign(0,macroTarget);bank.setValue(0,50);assert.equal(read(a).effects.chains[0].find(f=>f.id==='delay').parameters[0],80,'A Macro controls an extra effect by stable identity after reorder');bank.remove(0,macroTarget);assert.equal(read(a).effects.chains[0].find(f=>f.id==='delay').parameters[0],30);
  const program=normalizeProgram(read(a),parameters);program.modulation.tracks[0]=[{...newModulationLane('fx-lane'),target:{kind:'chain-effect',timbre:0,id:'delay',effectKind:14,parameter:0}}, {...newModulationLane('gate-lane'),target}];const moved=applyTimbre(program,7,captureTimbre(program,0,parameters),parameters);
  assert.deepEqual(moved.engine.effects.chains[7],program.engine.effects.chains[0]);assert.ok(moved.modulation.tracks[7].every(l=>l.target.timbre===7),'Timbre saves move Gate lanes and extra FX lanes to their new owner');assert.deepEqual(validateEffects(complete.effects),complete.effects);

  for(const sampleRate of [48000,44100]){
    let Processor;const messages=[];vm.runInNewContext(sequenceSource+'\n'+workletSource.replace(/^import .*;\n/gm,''),{sampleRate,WebAssembly,Float32Array,Uint8Array,validSampleSource,RecordingTap,MAX_TIMBRES,INITIAL_TIMBRES,OSC3_BASE,PARAMETER_COUNT,timbreArray,ModulationAutomation,ModulationHost,writeWasmJson,engineWrite,engineBytes,engineFloats,AudioWorkletProcessor:class{constructor(){this.port={postMessage:data=>messages.push(data)};}},registerProcessor:(_name,p)=>Processor=p});
    const p=new Processor({processorOptions:{module,gain:1}}),wasm=p.wasm;
    for(const [parameter,value]of [[3,0],[4,127],[5,127],[6,0],[9,127]])p.port.onmessage({data:{type:'control',timbre:0,parameter,value}});
    const seq=emptySequence();seq.tracks.forEach(track=>track.enabled=false);seq.tracks[0].enabled=true;seq.tracks[0].steps[0].notes=[69];const mod=emptyModulation(),gate={...newModulationLane('gate-clock'),target,resolution:'1/8'};gate.values.fill(-50);mod.tracks[0]=[gate];
    const state={config:mod,macros:emptyMacros(),targets:[{target,min:-100,max:100,base:0,available:true}],engine:read(wasm),effects:emptyEffects(),circuits:Array.from({length:MAX_TIMBRES},()=>audioCircuit(defaultCircuit()))};
    const msg=data=>{p.port.onmessage({data});assert.equal(p.failed,false);},block=()=>{const l=new Float32Array(128),r=new Float32Array(128);p.process([],[[l,r]]);assert.equal(p.failed,false);};
    msg({type:'sequencer',config:seq});msg({type:'modulation',state});msg({type:'sequence-play'});while(p.sequencer.frame<1024)block();assert.equal(p.sequencer.releases[0].frame,1500);while(p.sequencer.frame<2304)block();assert.equal(wasm.rustias_voices(),0,'Real AudioWorklet Gate note-offs release native voices at both device rates');assert.equal(seq.tracks[0].steps[0].gate,75);msg({type:'sequence-stop'});assert.equal(p.sequencer.gateMod[0],0);
    state.effects.chains[0]=[delay];const fxTarget={kind:'chain-effect',timbre:0,id:'delay',effectKind:14,parameter:0};const fxLane={...newModulationLane('extra-fx'),target:fxTarget};fxLane.values.fill(30);state.config=emptyModulation();state.config.tracks[0]=[fxLane];state.targets=[{target:fxTarget,min:0,max:100,base:delay.parameters[0],available:true}];msg({type:'effect-chain',timbre:0,chain:[delay]});msg({type:'modulation',state});msg({type:'sequence-play'});block();assert.equal(read(wasm).effects.chains[0][0].parameters[0],delay.parameters[0]+30);msg({type:'sequence-stop'});assert.equal(read(wasm).effects.chains[0][0].parameters[0],delay.parameters[0]);
    assert.ok(!messages.some(m=>m.type==='error'||m.type==='warning'));
  }
  return `Sequencer Gate: live chord/sample duration, zero/rest, restoration, timbre remapping and 48/44.1 kHz; unlimited timbre FX: 600 entries, all algorithms, serial order, preserved tails, isolated buses, modulation and JSON/timbre saves; OSC 3 matches OSC 1 gain (${gain.join(', ')} dB)`;
}
