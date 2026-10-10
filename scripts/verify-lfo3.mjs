import assert from 'node:assert/strict';
import {LFO3_BASE,PARAMETER_COUNT,MAX_TIMBRES} from '../web/limits.js';
import {extendValues} from '../web/parameters.js';
import {defaultCircuit,connect,removeModule,availableModules} from '../web/circuit.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {sampleValues,emptySamples,validateSamples} from '../web/sample-state.js';
import {MacroBank} from '../web/macros.js';
import {newModulationLane,ModulationAutomation} from '../web/modulation.js';

export function verifyLfo3({api,parameters,control,render,rms,save,load,setCircuit}){
  const b=LFO3_BASE;
  assert.equal(parameters.length,PARAMETER_COUNT);
  assert.deepEqual(parameters.slice(b).map(p=>[p.min,p.max,p.default]),parameters.slice(73,81).map(p=>[p.min,p.max,p.default]));
  assert.deepEqual(extendValues(parameters.slice(0,155).map(p=>p.default),parameters),parameters.map(p=>p.default));
  const energyDifference=(a,c)=>rms(a.map((v,i)=>v-c[i]));
  function voice({t=0,wave=0,rate=86,phase=0,sync=0,division=8,key=2,route=true,offset=0,feedback=false}={}){
    api.rustias_init();control(71,1,t);control(3,0,t);control(4,127,t);control(5,127,t);
    control(b,wave,t);control(b+2,rate,t);control(b+3,key,t);control(b+4,phase,t);control(b+5,sync,t);control(b+6,division,t);control(b+7,offset,t);
    if(route){control(90,16,t);control(91,11,t);control(92,110,t);}
    if(feedback){control(94,4,t);control(95,42,t);control(96,110,t);}
    assert.equal(api.rustias_note(t,69,100),1);return render(24000);
  }
  const dry=voice({route:false});
  for(let wave=0;wave<4;wave++)assert.ok(energyDifference(voice({wave}),dry)>.0001,`LFO 3 waveform ${wave} modulates the actual amplifier`);
  assert.ok(energyDifference(voice({rate:30}),voice({rate:100}))>.001,'LFO 3 has an independent live rate');
  assert.ok(energyDifference(voice({offset:20}),voice({offset:-20}))>.001,'Its bipolar rate offset reaches the native controller');
  assert.ok(energyDifference(voice({wave:2,phase:0}),voice({wave:2,phase:16}))>.001,'Voice key sync uses the selected initial phase');
  assert.ok(energyDifference(voice({feedback:true}),voice())>.0001,'Virtual Patch can modulate LFO 3 Rate');
  for(let t=0;t<MAX_TIMBRES;t++)assert.ok(energyDifference(voice({t}),voice({t,route:false}))>.0001,`LFO 3 belongs to timbre ${t+1}`);
  for(let division=0;division<=16;division++)assert.ok(rms(voice({sync:1,division}))>.0001,`Tempo division ${division} renders without nonfinite output`);
  assert.ok(energyDifference(voice({sync:1,division:8}),voice({sync:1,division:11}))>.001,'Tempo sync divisions change LFO 3 timing');
  for(const key of [0,1,2])assert.ok(rms(voice({key}))>.0001,`Key sync ${key} renders`);
  voice();const before=render(6000);control(b+2,24);const after=render(6000);assert.equal(api.rustias_voices(),1,'Editing LFO 3 keeps the held voice');assert.ok(energyDifference(before,after)>.0001);

  const graph=defaultCircuit();graph.enabled=true;const lfo3=graph.nodes.find(n=>n.kind==='lfo3');
  assert.ok(lfo3);const wired=connect(graph,lfo3.id,7,'gain');
  function circuitSound(c,rate=86){api.rustias_init();control(3,0);control(4,127);control(5,127);control(b+2,rate);setCircuit(c);api.rustias_note(0,69,100);return render(24000);}
  assert.ok(energyDifference(circuitSound(wired),circuitSound(graph))>.0001,'LFO 3 CV port controls the modular amplifier');
  assert.ok(energyDifference(circuitSound(wired,20),circuitSound(wired,100))>.0001,'The CV port follows the panel rate');
  assert.ok(availableModules(removeModule(wired,lfo3.id)).includes('lfo3'),'Removed LFO 3 can be added again');

  // PCM and direct pattern samples use the same third LFO, with independent
  // per-instrument/per-source controls rather than a synthetic-only overlay.
  const pcm=Float32Array.from({length:4800},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));
  const sampleParameters=[1,3,4,5,6,9,90,91,92,...Array.from({length:8},(_,i)=>b+i)];
  function sampleSound(modulated,library=false){
    api.rustias_init();const owner=library?7:0;if(library)control(71,1,owner);else control(140,1);
    const v=sampleValues(parameters.map(p=>p.default));v[b+2]=86;if(modulated){v[90]=16;v[91]=11;v[92]=110;}
    if(library){
      const ptr=api.rustias_library_sample_buffer(333,pcm.length);new Float32Array(api.memory.buffer,ptr,pcm.length).set(pcm);assert.equal(api.rustias_library_sample_commit(333,pcm.length),1);assert.equal(api.rustias_library_profile(333,333,owner,2),1);
      for(const id of sampleParameters)assert.equal(api.rustias_library_control(333,id,v[id]),1,`Library parameter ${id}`);
      api.rustias_library_sync();assert.equal(api.rustias_library_note(owner,333,100),1);
    }else{
      const ptr=api.rustias_sample_buffer(0,pcm.length);new Float32Array(api.memory.buffer,ptr,pcm.length).set(pcm);assert.equal(api.rustias_sample_commit(0,pcm.length,2),1);
      for(const id of sampleParameters)assert.equal(api.rustias_drum_control(0,id,v[id]),1,`Drum parameter ${id}`);
      api.rustias_drum_pad(0,100);
    }
    return render(24000);
  }
  for(const library of [false,true])assert.ok(energyDifference(sampleSound(true,library),sampleSound(false,library))>.001,'LFO 3 modulates PCM in kits and direct sample profiles');

  voice({t:7,wave:2,phase:8,sync:1,division:11,offset:-12});const engine=save();assert.equal(load(engine),1);assert.deepEqual(save(),engine);
  const old=structuredClone(engine);for(const v of [...old.timbres,...old.drums])v.length=b;
  assert.equal(load(old),1);const upgraded=save();assert.deepEqual(upgraded.timbres.map(v=>v.slice(0,b)),old.timbres,'163-parameter programs retain every existing value');assert.deepEqual(upgraded.timbres[7].slice(b),parameters.slice(b).map(p=>p.default));
  const profile=emptySamples();profile.library=[{source:'808:bd-tone1',timbre:7,mode:2,values:old.timbres[7]}];assert.equal(validateSamples(profile,parameters).library[0].values.length,PARAMETER_COUNT);
  const program=normalizeProgram(engine,parameters),target={kind:'synth',timbre:7,parameter:b+2};
  const bank=new MacroBank({resolve:()=>({label:'T8 · LFO 3 · Frequency',min:0,max:127,read:()=>40}),write:()=>{}});bank.assign(0,target);bank.setAmount(0,target,-50);program.macros=bank.getConfig();
  const lane=newModulationLane('lfo3-motion');lane.target={kind:'synth',timbre:7,parameter:b+7};lane.values[0]=12;program.modulation.tracks[7]=[lane];
  const moved=applyTimbre(program,6,captureTimbre(program,7,parameters),parameters);assert.deepEqual(moved.engine.timbres[6].slice(b),program.engine.timbres[7].slice(b));assert.equal(moved.modulation.tracks[6][0].target.timbre,6);assert.deepEqual(normalizeProgram(JSON.parse(JSON.stringify(program)),parameters).macros,program.macros);
  const changes=[],automation=new ModulationAutomation(values=>{changes.push(...values);});automation.setState({config:program.modulation,macros:program.macros,targets:[{target:lane.target,min:-64,max:63,base:0,available:true}]});automation.clock.play();automation.beforeRender();assert.equal(changes[0].value,12,'Mod sequencers can target third-LFO parameters');automation.stop();assert.equal(changes.at(-1).value,0,'Stopping restores the saved LFO 3 value');
  api.rustias_stop();return 'Browser LFO 3: shared Rust waveforms, live rate/phase/key/tempo sync, all eight timbres, Virtual Patch source/rate feedback, CV wiring, kit/direct PCM, macros, modulation lanes, program/timbre persistence and 163-parameter migration';
}
