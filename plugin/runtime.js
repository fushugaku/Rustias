import '../web/worklet.js';
import {macroOffset,targetKey} from '../web/macros.js';
let processor,outputs,left,right;const buffers=new Map();
globalThis.pluginBoot=(bytes,rate)=>{
  globalThis.sampleRate=rate;
  processor=new globalThis.PluginProcessor({processorOptions:{module:new WebAssembly.Module(bytes),gain:.3}});
  left=new Float32Array(128);right=new Float32Array(128);outputs=[[left,right]];
};
globalThis.pluginMessage=message=>processor.port.onmessage({data:message});
globalThis.pluginMidi=(a,b,c)=>pluginMessage({type:'midi',bytes:[a,b,c]});
globalThis.pluginRender=frames=>{
  if(frames!==left.length){let pair=buffers.get(frames);if(!pair){pair=[new Float32Array(frames),new Float32Array(frames)];buffers.set(frames,pair);}[left,right]=pair;outputs=[pair];}
  processor.process([],outputs);if(processor.failed)throw new Error('The shared audio worklet rejected a message.');return [left,right];
};
globalThis.pluginParameter=(index,value)=>{
  if(index===8){pluginMessage({type:'gain',value:value/100});return;}
  const m=processor.modulation,knob=m.macros.knobs[index];if(!knob)return;
  m.restore();knob.value=value;
  const bases=new Map(m.macros.bases.map(b=>[targetKey(b.target),b.value])),changes=[];
  for(const binding of knob.bindings){const key=targetKey(binding.target),spec=m.targets.get(key);if(!spec||!bases.has(key))continue;
    const value=Math.max(spec.min,Math.min(spec.max,bases.get(key)+macroOffset(m.macros,binding.target,spec)));
    spec.base=spec.values?spec.values.reduce((a,b)=>Math.abs(b-value)<Math.abs(a-value)?b:a):Math.round(value/(spec.step??1))*(spec.step??1);
    changes.push({target:binding.target,value:spec.base});
  }
  processor.modulationHost.write(changes);
};
globalThis.pluginFailed=()=>processor.failed;
globalThis.pluginStats=()=>JSON.stringify({frames:processor.wasm.rustias_frames(),voices:processor.wasm.rustias_voices(),capacity:processor.wasm.rustias_voice_capacity(),failed:processor.failed});
