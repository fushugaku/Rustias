import '../web/worklet.js';
import {writeWasmJson} from '../web/wasm-json.js';
import {engineBytes} from '../web/engine-io.js';
// Editor/control bridge only. The native Rust engine owns all modulation clocks,
// interpolation, macro arithmetic and temporary parameter values.
class NativeModulation {
  constructor(engine){
    this.engine=engine;
    this.clock={setTempo:v=>engine.rustias_modulation_tempo(v),play:()=>engine.rustias_modulation_play(),stop:()=>engine.rustias_modulation_stop(),reset:()=>engine.rustias_modulation_reset(),note(){}};
  }
  restore(){this.engine.rustias_modulation_restore();}
  stop(){this.engine.rustias_modulation_stop();}
  setState(state){if(!this.engine.rustias_modulation_state(writeWasmJson(this.engine,state)))throw new Error('The native modulation engine rejected this program.');}
  profile(data){if(!this.engine.rustias_modulation_profile(data.timbre,data.id,writeWasmJson(this.engine,data.source)))throw new Error('The native modulation sample mapping was rejected.');}
  parameter(index,value){this.engine.rustias_modulation_parameter(index,value);}
  transport(data){this.engine.rustias_modulation_transport(+data.playing,data.tempo,data.beat??0,+data.seek);}
  acceptValues(){}
  beforeRender(){}
  status(){const n=this.engine.rustias_modulation_status(),bytes=engineBytes(this.engine,this.engine.rustias_preset_buffer(),n);let json='';for(const b of bytes)json+=String.fromCharCode(b);return JSON.parse(json);}
}
// No note clock is constructed or restored in a native plugin. Browser pattern
// data can remain in imported programs without ever generating plugin notes.
const dawNotes={setConfig(){},setTempo(){},play(){},stop(){},reset(){},beforeRender(){},trigger(){return false;},status(){return null;}};
let processor,outputs,left,right;const buffers=new Map();
globalThis.pluginBoot=(engine,rate)=>{
  globalThis.sampleRate=rate;
  processor=new globalThis.PluginProcessor({processorOptions:{engine,gain:.3,nativeModulation:new NativeModulation(engine),sequencer:dawNotes}});
  left=new Float32Array(128);right=new Float32Array(128);outputs=[[left,right]];
};
globalThis.pluginMessage=message=>{
  if(message.type==='transport'){processor.modulation.transport(message);return;}
  if(['sequencer','sequence-play','sequence-stop','sequence-reset'].includes(message.type))return;
  processor.port.onmessage({data:message});
};
globalThis.pluginMidi=(a,b,c)=>pluginMessage({type:'midi',bytes:[a,b,c]});
globalThis.pluginRender=frames=>{
  if(frames!==left.length){let pair=buffers.get(frames);if(!pair){pair=[new Float32Array(frames),new Float32Array(frames)];buffers.set(frames,pair);}[left,right]=pair;outputs=[pair];}
  processor.process([],outputs);if(processor.failed)throw new Error('The shared audio worklet rejected a message.');return [left,right];
};
globalThis.pluginParameter=(index,value)=>{
  processor.modulation.parameter(index,value);
};
globalThis.pluginFailed=()=>processor.failed;
globalThis.pluginStats=()=>JSON.stringify({frames:processor.wasm.rustias_frames(),voices:processor.wasm.rustias_voices(),capacity:processor.wasm.rustias_voice_capacity(),failed:processor.failed,backend:"native-rust",modulation:processor.modulation.status()});
