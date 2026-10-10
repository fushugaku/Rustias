import {modulationKey} from './modulation.js';
// Browser adapter only: every sound parameter still goes through the shared Rust API.
const copy=value=>JSON.parse(JSON.stringify(value));
export class ModulationHost {
  constructor(processor){this.processor=processor;this.effects=[];this.circuits=[];this.engine=null;}
  setState(state){this.effects=copy(state.effects.slots);this.circuits=copy(state.circuits);this.engine=copy(state.engine);}
  jsonCall(name,args,value){
    const wasm=this.processor.wasm,json=JSON.stringify(value);
    if(json.length>wasm.rustias_preset_capacity())throw new Error('Modulation data exceeds the engine buffer.');
    new Uint8Array(wasm.memory.buffer,wasm.rustias_preset_buffer(),json.length).set(Uint8Array.from(json,c=>c.charCodeAt(0)));
    return !!wasm[name](...args,json.length);
  }
  write(changes){
    const wasm=this.processor.wasm,applied=[],effects=new Map(),circuits=new Map();let library=false;
    // Wave changes precede Mode so Noise/Formant keep the native Waveform constraint.
    changes.sort((a,b)=>(a.target.parameter===0?-1:0)-(b.target.parameter===0?-1:0));
    for(const change of changes){
      const {target}=change;let value=change.value,ok=false;
      if(target.kind==='synth'||target.kind==='global'){
        const t=target.timbre??0;
        if(target.parameter===10&&wasm.rustias_value(t,0)>=4)value=change.value=0;
        ok=!!wasm.rustias_control(t,target.parameter,value);
      }else if(target.kind==='drum'){
        const values=this.engine?.drums[target.instrument];
        if(target.parameter===10&&values?.[0]>=4)value=change.value=0;
        ok=!!wasm.rustias_drum_control(target.instrument,target.parameter,value);
        if(ok&&values){values[target.parameter]=value;if(target.parameter===0&&value>=4)values[10]=0;}
      }else if(target.kind==='sample'){
        const profile=this.processor.libraryProfiles.get(target.timbre+':'+target.source);
        if(profile){if(target.parameter===10&&wasm.rustias_library_value(profile.id,0)>=4)value=change.value=0;ok=!!wasm.rustias_library_control(profile.id,target.parameter,value);library=library||ok;}
      }else if(target.kind==='effect'){
        const program=this.effects[target.slot];
        if(program?.kind===target.effectKind){program.parameters[target.parameter]=value;if(!effects.has(target.slot))effects.set(target.slot,[]);effects.get(target.slot).push(modulationKey(target));}
      }else if(target.kind==='module'){
        const circuit=this.circuits[target.timbre],node=circuit?.nodes.find(n=>n.id===target.node&&n.kind===target.moduleKind);
        if(node){node.params[target.control]=value;if(node.kind==='oscillator1'&&node.params.wave>=4){node.params.mode=0;if(target.control==='mode')change.value=0;}if(!circuits.has(target.timbre))circuits.set(target.timbre,[]);circuits.get(target.timbre).push(modulationKey(target));}
      }else if(target.kind==='kit-gain'){wasm.rustias_drum_gain(value);ok=true;}
      else if(target.kind==='volume'){this.processor.gain=value/100;ok=true;}
      if(ok)applied.push(modulationKey(target));
    }
    if(library)wasm.rustias_library_sync();
    for(const [slot,keys]of effects)if(this.jsonCall('rustias_effect',[slot],this.effects[slot]))applied.push(...keys);
    for(const [t,keys]of circuits)if(this.jsonCall('rustias_circuit',[t],this.circuits[t]))applied.push(...keys);
    return applied;
  }
}
