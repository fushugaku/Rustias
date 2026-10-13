import {modulationKey} from './modulation.js';
import {OSC3_BASE} from './limits.js';
import {writeWasmJson} from './wasm-json.js';
// Browser adapter only: every sound parameter still goes through the shared Rust API.
const copy=value=>JSON.parse(JSON.stringify(value));
export class ModulationHost {
  constructor(processor){this.processor=processor;this.effects=[];this.chains=[];this.circuits=[];this.engine=null;}
  setState(state){this.effects=copy(state.effects.slots);this.chains=copy(state.effects.chains??[]);this.circuits=copy(state.circuits);this.engine=copy(state.engine);}
  jsonCall(name,args,value){
    const wasm=this.processor.wasm,length=writeWasmJson(wasm,value);
    return !!wasm[name](...args,length);
  }
  write(changes){
    const wasm=this.processor.wasm,applied=[],effects=new Map(),chainEffects=new Map(),circuits=new Map();let library=false;
    // Wave changes precede Mode so Noise/Formant keep the native Waveform constraint.
    const waveFirst=c=>c.target.parameter===0||c.target.parameter===OSC3_BASE?-1:0;
    changes.sort((a,b)=>waveFirst(a)-waveFirst(b));
    for(const change of changes){
      const {target}=change;let value=change.value,ok=false;
      if(target.kind==='synth'||target.kind==='global'){
        const t=target.timbre??0;
        const wave=target.parameter===10?0:target.parameter===OSC3_BASE+1?OSC3_BASE:null;
        if(wave!=null&&wasm.rustias_value(t,wave)>=4)value=change.value=0;
        ok=!!(target.parameter>=OSC3_BASE?wasm.rustias_osc3_control(t,target.parameter-OSC3_BASE,value):wasm.rustias_control(t,target.parameter,value));
      }else if(target.kind==='drum'){
        const values=this.engine?.drums[target.instrument];
        const wave=target.parameter===10?0:target.parameter===OSC3_BASE+1?OSC3_BASE:null;
        if(wave!=null&&values?.[wave]>=4)value=change.value=0;
        ok=!!wasm.rustias_drum_control(target.instrument,target.parameter,value);
        if(ok&&values){values[target.parameter]=value;if(target.parameter===0&&value>=4)values[10]=0;if(target.parameter===OSC3_BASE&&value>=4)values[OSC3_BASE+1]=0;}
      }else if(target.kind==='sample'){
        const profile=this.processor.libraryProfiles.get(target.timbre+':'+target.source);
        if(profile){const wave=target.parameter===10?0:target.parameter===OSC3_BASE+1?OSC3_BASE:null;if(wave!=null&&wasm.rustias_library_value(profile.id,wave)>=4)value=change.value=0;ok=!!wasm.rustias_library_control(profile.id,target.parameter,value);library=library||ok;}
      }else if(target.kind==='effect'){
        const program=this.effects[target.slot];
        if(program?.kind===target.effectKind){program.parameters[target.parameter]=value;if(!effects.has(target.slot))effects.set(target.slot,[]);effects.get(target.slot).push(modulationKey(target));}
      }else if(target.kind==='chain-effect'){
        const program=this.chains[target.timbre]?.find(fx=>fx.id===target.id);
        if(program?.kind===target.effectKind){program.parameters[target.parameter]=value;const key=target.timbre+':'+target.id;if(!chainEffects.has(key))chainEffects.set(key,{timbre:target.timbre,program,keys:[]});chainEffects.get(key).keys.push(modulationKey(target));}
      }else if(target.kind==='module'){
        const circuit=this.circuits[target.timbre],node=circuit?.nodes.find(n=>n.id===target.node&&n.kind===target.moduleKind);
        if(node){node.params[target.control]=value;if(node.kind==='oscillator1'&&node.params.wave>=4){node.params.mode=0;if(target.control==='mode')change.value=0;}if(!circuits.has(target.timbre))circuits.set(target.timbre,[]);circuits.get(target.timbre).push(modulationKey(target));}
      }else if(target.kind==='kit-gain'){wasm.rustias_drum_gain(value);ok=true;}
      else if(target.kind==='sequence-gate'){this.processor.sequencer.setGateOffset(target.timbre,value);ok=true;}
      else if(target.kind==='volume'){this.processor.gain=value/100;ok=true;}
      if(ok)applied.push(modulationKey(target));
    }
    if(library)wasm.rustias_library_sync();
    for(const [slot,keys]of effects)if(this.jsonCall('rustias_effect',[slot],this.effects[slot]))applied.push(...keys);
    for(const {timbre,program,keys}of chainEffects.values())if(this.jsonCall('rustias_chain_effect',[timbre],program))applied.push(...keys);
    for(const [t,keys]of circuits)if(this.jsonCall('rustias_circuit',[t],this.circuits[t]))applied.push(...keys);
    return applied;
  }
}
