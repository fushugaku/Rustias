// Golden modulation behavior comes from the existing browser implementation.
// The native check runs these scenarios entirely through the Rust ABI, with no JS.
import {ModulationAutomation,emptyModulation,newModulationLane} from '../web/modulation.js';
import {ModulationHost} from '../web/modulation-host.js';
import {emptyMacros,targetKey,macroValue} from '../web/macros.js';
import {emptyEffects,defaultEffect} from '../web/effects.js';
import {defaultCircuit,audioCircuit} from '../web/circuit.js';

export function modulationFixtures(module){
  const cases=[];
  function scenario(name,run){
    const api=new WebAssembly.Instance(module,{}).exports,operations=[],processor={wasm:api,libraryProfiles:new Map(),gain:.3};
    const host=new ModulationHost(processor),automation=new ModulationAutomation(changes=>host.write(changes));
    function call(name,...args){
      if(['rustias_control','rustias_osc3_control','rustias_drum_control','rustias_library_control','rustias_midi'].includes(name)){automation.restore();operations.push({native:'rustias_modulation_restore',args:[]});}
      const value=api[name](...args);operations.push({call:name,args,...(typeof value==='number'?{expected:value}:{})});
      if(name==='rustias_note')automation.clock.note(...args);
      if(name==='rustias_library_note')automation.clock.note(args[0],args[1]+128,args[2]);
      if(name==='rustias_midi'){
        if((args[0]&240)===144||(args[0]&240)===128){for(let t=0;t<8;t++)if(api.rustias_value(t,71)&&(api.rustias_value(t,72)===16?api.rustias_value(0,148):api.rustias_value(t,72))===(args[0]&15))automation.clock.note(t,args[1],(args[0]&240)===144?args[2]:0);}
        else automation.acceptValues(target=>target.kind==='synth'?api.rustias_value(target.timbre,target.parameter):null);
      }
      return value;
    }
    function stage(value){const bytes=new TextEncoder().encode(JSON.stringify(value));call('rustias_preset_reserve',bytes.length);const p=api.rustias_preset_buffer();operations.push({call:'rustias_preset_buffer',args:[],buffer:true});new Uint8Array(api.memory.buffer,p,bytes.length).set(bytes);operations.push({write:[...bytes]});return bytes.length;}
    function native(name,...args){operations.push({native:name,args});}
    function json(name,args,value){const length=stage(value);if(call(name,...args,length)!==1)throw new Error('Invalid modulation fixture program');}
    let state;
    const e={api,call,native,json,
      pcm(name,args,values){const p=api[name](...args,values.length);operations.push({call:name,args:[...args,values.length],buffer:true});new Float32Array(api.memory.buffer,p,values.length).set(values);operations.push({floats:[...values]});},
      profile(timbre,id,source){processor.libraryProfiles.set(timbre+':'+source,{id,timbre,source});const length=stage(source);operations.push({native:'rustias_modulation_profile',args:[timbre,id,length],expected:1});},
      setState(next){state=structuredClone(next);automation.setState(state);host.setState(state);const length=stage(next);operations.push({native:'rustias_modulation_state',args:[length],expected:1});},
      play(){automation.clock.play();native('rustias_modulation_play');},
      stop(){automation.stop();native('rustias_modulation_stop');},
      tempo(value){automation.clock.setTempo(value);native('rustias_modulation_tempo',value);},
      macro(index,value){automation.restore();automation.macros.knobs[index].value=value;const knob=automation.macros.knobs[index],changes=[];for(const binding of knob.bindings){const spec=automation.targets.get(targetKey(binding.target)),base=automation.macros.bases.find(b=>targetKey(b.target)===targetKey(binding.target));if(!spec||!base)continue;spec.base=macroValue(automation.macros,base,spec);changes.push({target:binding.target,value:spec.base});}host.write(changes);native('rustias_modulation_parameter',index,value);},
      render(blocks=6){const expected=[];for(let b=0;b<blocks;b++){automation.beforeRender(128);const p=api.rustias_render();expected.push(...new Float32Array(api.memory.buffer,p,256));}operations.push({render:blocks,expected,modulation:automation.status()});},
      snapshot(){automation.restore();native('rustias_modulation_restore');const n=api.rustias_save(),p=api.rustias_preset_buffer();operations.push({snapshot:JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer,p,n)))});},
    };
    call('rustias_init');for(const [id,value]of [[9,127],[3,0],[4,127],[5,127]])call('rustias_control',0,id,value);
    const n=api.rustias_save(),p=api.rustias_preset_buffer();e.state={config:emptyModulation(),macros:emptyMacros(),targets:[],effects:emptyEffects(),circuits:Array.from({length:8},()=>audioCircuit(defaultCircuit())),engine:JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer,p,n)))};
    run(e);e.stop();e.snapshot();cases.push({name:'Rust mod: '+name,operations});
  }
  const synth=(parameter,timbre=0)=>({kind:'synth',timbre,parameter});
  const spec=(target,base,min=0,max=127)=>({target,base,min,max,available:true});
  const lane=(id,target,values,options={})=>({...newModulationLane(id),target,values:Array.from({length:128},(_,i)=>values[i%values.length]),...options});
  for(const direction of ['Forward','Reverse','Alt1','Alt2'])for(const runMode of ['Loop','OneShot'])for(const motion of ['Step','Slide'])scenario(`${direction}/${runMode}/${motion}`,e=>{
    const target=synth(1),l=lane('clock',target,[-40,0,40],{length:3,resolution:'1/48',motion,direction,runMode});e.state.targets=[spec(target,64)];e.state.config.tracks[0]=[l];e.setState(e.state);e.call('rustias_note',0,69,100);e.play();e.render(100);e.tempo(80);e.render(32);e.stop();e.render(2);
  });
  scenario('six independent divisions on timbre eight and key synchronization',e=>{
    e.call('rustias_control',7,71,1);const divisions=['1/16','1/8','1/2','1/32','1/48','1/3'];
    e.state.targets=divisions.map((_,i)=>spec(synth([1,2,7,9,11,12][i],7),64));
    e.state.config.tracks[7]=divisions.map((resolution,i)=>lane('lane'+i,e.state.targets[i].target,[0,30,-30],{length:128,resolution,keySync:i%3===0?'Off':i%3===1?'Timbre':'Voice'}));e.setState(e.state);e.call('rustias_midi',0x97,60,100);e.render(92);e.call('rustias_midi',0x97,64,100);e.render(22);e.call('rustias_midi',0x87,60,0);e.call('rustias_midi',0x87,64,0);e.render(3);e.call('rustias_midi',0x97,67,100);e.render(12);
  });
  scenario('bipolar macros, direct offsets, later-lane priority and DAW automation',e=>{
    const target=synth(1),wheel=synth(138);e.call('rustias_control',0,1,87);e.state.targets=[spec(target,87),spec(wheel,0)];
    e.state.macros.knobs[0].value=20;e.state.macros.knobs[0].bindings=[{target,amount:-50},{target:wheel,amount:50}];e.state.macros.bases=[{target,value:100,label:'Cutoff'},{target:wheel,value:-12.7,label:'Wheel'}];
    e.state.config.tracks[0]=[lane('macro',{kind:'macro',macro:0},[40,-20],{length:2,motion:'Slide'}),lane('direct',target,[-10]),lane('later',target,[15])];
    e.setState(e.state);e.call('rustias_midi',0x90,69,100);e.play();e.render(45);e.macro(0,70);e.render(20);e.call('rustias_midi',0xb0,1,20);e.render(10);e.stop();e.render(2);
  });
  scenario('OSC 3 controls are independent, discrete values and base restoration',e=>{
    e.call('rustias_control',0,17,0);e.call('rustias_control',0,19,127);
    e.state.targets=[spec(synth(175),64,40,88),spec(synth(173),64),{...spec(synth(171),0,0,5),values:[0,1,2,3,4,5]}];e.state.config.tracks[0]=e.state.targets.map((s,i)=>lane('osc'+i,s.target,[i===2?1:20,0],{length:2,resolution:'1/48'}));e.setState(e.state);e.call('rustias_note',0,69,100);e.render(30);e.call('rustias_control',0,15,76);e.render(5);
  });
  scenario('fixed and unlimited FX chains, module parameters, gain and unavailable targets',e=>{
    const fx=defaultEffect(14),chain={...defaultEffect(14,true),id:'native-mod-fx'},graph=audioCircuit(defaultCircuit());graph.enabled=true;graph.nodes.push({id:16,kind:'vca',params:{gain:0}});graph.wires=graph.wires.filter(w=>w.to!==15);graph.wires.push({from:7,to:16,port:'in'},{from:16,to:15,port:'in'});
    e.json('rustias_effect',[0],fx);e.json('rustias_effect_chain',[0],[chain]);e.json('rustias_circuit',[0],graph);e.state.effects.slots[0]=fx;e.state.effects.chains[0]=[chain];e.state.circuits[0]=graph;
    const targets=[{kind:'effect',slot:0,effectKind:14,parameter:0},{kind:'chain-effect',timbre:0,id:chain.id,effectKind:14,parameter:0},{kind:'module',timbre:0,node:16,moduleKind:'vca',control:'gain'},{kind:'kit-gain'},{kind:'volume'}];
    e.state.targets=targets.map((target,i)=>spec(target,[30,30,0,0,30][i],[0,0,-48,-24,0][i],[100,100,24,24,100][i]));e.state.targets.push({...spec(synth(1),127),available:false});
    e.state.config.tracks[0]=e.state.targets.map((s,i)=>lane('fx'+i,s.target,[i===2?-12:20,0],{length:2,resolution:'1/48',motion:i%2?'Step':'Slide'}));e.setState(e.state);e.call('rustias_note',0,69,100);e.render(30);e.stop();e.render(2);
  });
  scenario('sample-library modulation',e=>{
    const values=Float32Array.from({length:12000},(_,i)=>.3*Math.sin(i*2*Math.PI*440/48000));
    e.pcm('rustias_library_sample_buffer',[90],values);e.call('rustias_library_sample_commit',90,values.length);e.call('rustias_control',7,71,1);e.call('rustias_control',0,141,7);e.call('rustias_control',0,140,1);e.call('rustias_library_profile',90,90,7,2);e.call('rustias_library_control',90,1,100);e.call('rustias_library_sync');e.profile(7,90,'custom:native-mod');
    const target={kind:'sample',timbre:7,source:'custom:native-mod',parameter:1};e.state.targets=[spec(target,100)];e.state.config.tracks[7]=[lane('sample',target,[-63,0],{length:2,resolution:'1/48',motion:'Slide',keySync:'Voice'})];e.setState(e.state);e.call('rustias_library_note',7,90,100);e.render(30);e.call('rustias_library_note',7,90,100);e.render(5);
  });
  return cases;
}
