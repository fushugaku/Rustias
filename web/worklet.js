import {SequenceClock,StepAudition} from "./sequence.js";
import {RecordingTap} from './recording-tap.js';
import {ModulationAutomation} from './modulation.js';
import {ModulationHost} from './modulation-host.js';
class RustiasProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.wasm = new WebAssembly.Instance(options.processorOptions.module, {}).exports;
    this.wasm.rustias_init();
    this.gain = options.processorOptions.gain ?? 0.3;
    this.nativeIndex = 128;
    this.phase = 0;
    this.currentLeft = this.currentRight = this.nextLeft = this.nextRight = 0;
    this.callbacks = 0;
    this.audibleFrames = 0;
    this.peak = 0;
    this.failed = false;
    this.recording = new RecordingTap((data, transfer) => this.port.postMessage(data, transfer ?? []), sampleRate);
    this.libraryProfiles=new Map();
    this.manualNotes=new Set();this.sequenceNotes=new Map();this.auditionNotes=new Set();
    this.sequencer=new SequenceClock((t,n,v)=>this.ownedNote(t,n,v,true));
    this.audition=new StepAudition((t,n,v)=>this.ownedNote(t,n,v,"audition"));
    this.modulationHost=new ModulationHost(this);
    this.modulation=new ModulationAutomation(changes=>this.modulationHost.write(changes));
    this.port.onmessage = ({ data }) => {
      try {
        if(['control','drum-control','library-control','effect','circuit','gain','drum-gain','midi','load'].includes(data.type))this.modulation.restore();
        if (data.type === 'record-start') this.recording.start(data.id);
        else if (data.type === 'record-stop') this.recording.stop(data.id);
        else if (data.type === "note") this.ownedNote(data.timbre,data.note,data.velocity,false);
        else if (data.type === "control") {
          if (!this.wasm.rustias_control(data.timbre, data.parameter, data.value)) this.snapshot();
          if(data.parameter===89)this.sequencer.setTempo(this.wasm.rustias_value(0,89)/10);
        }
        else if(data.type==='circuit'){
          const json=JSON.stringify(data.circuit),bytes=Uint8Array.from(json,c=>c.charCodeAt(0));
          if(bytes.length>this.wasm.rustias_preset_capacity())throw new Error('Modular patch exceeds the engine buffer.');
          new Uint8Array(this.wasm.memory.buffer,this.wasm.rustias_preset_buffer(),bytes.length).set(bytes);
          if(!this.wasm.rustias_circuit(data.timbre,bytes.length))this.port.postMessage({type:'warning',message:'The engine rejected this modular patch.'});
        }
        else if(data.type==='drum-gain')this.wasm.rustias_drum_gain(data.value);
        else if(data.type==='effect'){
          const json=JSON.stringify(data.program),bytes=Uint8Array.from(json,c=>c.charCodeAt(0));
          new Uint8Array(this.wasm.memory.buffer,this.wasm.rustias_preset_buffer(),bytes.length).set(bytes);
          if(!this.wasm.rustias_effect(data.slot,bytes.length))this.port.postMessage({type:'warning',message:'The engine rejected these effect settings.'});
        }
        else if(data.type==='library-reset'){this.wasm.rustias_library_reset();this.libraryProfiles.clear();}
        else if(data.type==='library-sample'){
          const frames=data.data.length,pointer=this.wasm.rustias_library_sample_buffer(data.asset,frames);let ok=false;
          if(pointer){new Float32Array(this.wasm.memory.buffer,pointer,frames).set(data.data);ok=!!this.wasm.rustias_library_sample_commit(data.asset,frames);}
          this.port.postMessage({type:'sample-ready',request:data.request,ok});
        }
        else if(data.type==='library-profile'){
          if(!this.wasm.rustias_library_profile(data.id,data.asset,data.timbre,data.mode))throw new Error('The sample profile could not be loaded.');
          for(let id=0;id<data.values.length;id++)if(!this.wasm.rustias_library_control(data.id,id,data.values[id]))throw new Error('Invalid sample sound parameters.');
          this.wasm.rustias_library_sync();this.libraryProfiles.set(`${data.timbre}:${data.source}`,{id:data.id,timbre:data.timbre,source:data.source});
        }
        else if(data.type==='library-control'){this.wasm.rustias_library_control(data.id,data.parameter,data.value);this.wasm.rustias_library_sync();}
        else if (data.type === "drum-control") this.wasm.rustias_drum_control(data.instrument,data.parameter,data.value);
        else if (data.type === "drum") {if(data.velocity&&!this.modulation.clock.running)this.modulation.clock.play();this.wasm.rustias_drum_pad(data.instrument, data.velocity);}
        else if (data.type === "load") {
          const json = JSON.stringify(data.program);
          if (json.length > this.wasm.rustias_preset_capacity()) throw new Error("Program exceeds the engine buffer.");
          const bytes = new Uint8Array(this.wasm.memory.buffer, this.wasm.rustias_preset_buffer(), json.length);
          for (let i = 0; i < json.length; i++) bytes[i] = json.charCodeAt(i);
          if (!this.wasm.rustias_load(json.length)) {
            this.port.postMessage({type: "warning", message: "The Rust engine rejected this program."}); this.snapshot();
          } else {
            this.wasm.rustias_rdl_mute(data.unavailable?.timbres??0,data.unavailable?.drums??0);
            this.sequencer.stop();this.audition.stop();this.manualNotes.clear();this.sequenceNotes.clear();this.auditionNotes.clear();
            this.modulation.stop();
            this.sequencer.setTempo(this.wasm.rustias_value(0,89)/10);
            this.nativeIndex = 128; this.phase = 0;
            this.currentLeft = this.currentRight = this.nextLeft = this.nextRight = 0;
          }
        }
        else if(data.type==='rdl-muted')this.wasm.rustias_rdl_mute(data.timbres,data.drums);
        else if (data.type === "midi") {
          if((data.bytes[0]&240)===144&&data.bytes[2]&&!this.modulation.clock.running)this.modulation.clock.play();
          this.wasm.rustias_midi(...data.bytes);
          if ((data.bytes[0] & 240) === 176 || (data.bytes[0] & 240) === 224) this.snapshot(data.bytes);
        }
        else if(data.type==="audition")this.audition.play(data.timbre,data.step,this.wasm.rustias_value(0,89)/10,data.resolution);
        else if(data.type==="sequencer")this.sequencer.setConfig(data.config);
        else if(data.type==='modulation'){this.modulation.setState(data.state);this.modulationHost.setState(data.state);}
        else if(data.type==="sequence-play"){this.sequencer.play();this.modulation.clock.play();}
        else if(data.type==="sequence-stop"){this.sequencer.stop();this.modulation.stop();}
        else if(data.type==="sequence-reset"){this.sequencer.reset();this.modulation.clock.reset();}
        else if (data.type === "stop") {this.sequencer.stop();this.modulation.stop();this.audition.stop();this.manualNotes.clear();this.sequenceNotes.clear();this.auditionNotes.clear();this.wasm.rustias_stop();}
        else if(data.type==="sample"){
          const frames=data.data.length,pointer=this.wasm.rustias_sample_buffer(data.instrument,frames);
          let ok=false;
          if(pointer){new Float32Array(this.wasm.memory.buffer,pointer,frames).set(data.data);ok=!!this.wasm.rustias_sample_commit(data.instrument,frames,data.mode);}
          this.port.postMessage({type:"sample-ready",request:data.request,ok});
        }
        else if(data.type==="sample-clear")this.wasm.rustias_sample_clear(data.instrument);
        else if(data.type==="sample-mode")this.wasm.rustias_sample_mode(data.instrument,data.mode);
        else if (data.type === "gain") this.gain = Math.max(0, Math.min(1, data.value));
      } catch (error) { this.fail(error); }
    };
    this.port.postMessage({ type: "ready", sampleRate });
  }
  ownedNote(timbre,note,velocity,sequence){
    if(velocity&&!this.modulation.clock.running)this.modulation.clock.play();
    const key=`${timbre}:${note}`,before=(this.sequenceNotes.get(key)??0)+(this.manualNotes.has(key)?1:0)+(this.auditionNotes.has(key)?1:0);
    if(sequence==="audition"){if(velocity)this.auditionNotes.add(key);else this.auditionNotes.delete(key);}
    else if(sequence){const count=Math.max(0,(this.sequenceNotes.get(key)??0)+(velocity?1:-1));if(count)this.sequenceNotes.set(key,count);else this.sequenceNotes.delete(key);}
    else if(velocity)this.manualNotes.add(key);else this.manualNotes.delete(key);
    const after=(this.sequenceNotes.get(key)??0)+(this.manualNotes.has(key)?1:0)+(this.auditionNotes.has(key)?1:0);
    const emit=value=>{if(typeof note==='string'){const profile=this.libraryProfiles.get(key);if(profile)this.wasm.rustias_library_note(timbre,profile.id,value);}else this.wasm.rustias_note(timbre,note,value);};
    if(velocity&&!before)emit(velocity);else if(!after&&before)emit(0);
  }
  snapshot(midi) {
    const length = this.wasm.rustias_save(), pointer = this.wasm.rustias_preset_buffer();
    const bytes = new Uint8Array(this.wasm.memory.buffer, pointer, length);
    let json = ""; for (let i = 0; i < length; i++) json += String.fromCharCode(bytes[i]);
    const program=JSON.parse(json);
    if(midi)this.modulation.acceptValues(target=>{
      if(target.kind==='synth'||target.kind==='global')return program.timbres[target.timbre??0][target.parameter];
      if(target.kind==='drum')return program.drums[target.instrument][target.parameter];
      if(target.kind==='sample'){const profile=this.libraryProfiles.get(target.timbre+':'+target.source);if(profile)return this.wasm.rustias_library_value(profile.id,target.parameter);}
    });
    this.port.postMessage({type: "state", program,midi,libraryVolumes:[...this.libraryProfiles.values()].map(p=>({timbre:p.timbre,source:p.source,value:this.wasm.rustias_library_value(p.id,117)}))});
  }
  fail(error) {
    this.failed = true;
    this.recording.stop();
    this.port.postMessage({ type: "error", message: error.message ?? String(error) });
  }
  nativeSample() {
    if(this.block&&this.block.buffer!==this.wasm.memory.buffer)this.block=new Float32Array(this.wasm.memory.buffer,this.pointer,256);
    if (this.nativeIndex === 128) {
      const tempo=this.wasm.rustias_value(0,89)/10;this.modulation.clock.setTempo(tempo);this.modulation.beforeRender(128);
      const sequenceTempo=this.wasm.rustias_value(0,89)/10;if(sequenceTempo!==this.sequencer.tempo)this.sequencer.setTempo(sequenceTempo);
      this.sequencer.beforeRender(128);this.audition.beforeRender(128);
      const pointer = this.wasm.rustias_render();
      if (this.pointer !== pointer || this.block?.buffer !== this.wasm.memory.buffer) {
        this.pointer = pointer; this.block = new Float32Array(this.wasm.memory.buffer, pointer, 256);
      }
      this.nativeIndex = 0;
    }
    const index = this.nativeIndex++ * 2;
    this.nativeLeft = this.block[index]; this.nativeRight = this.block[index + 1];
  }
  process(_inputs, outputs) {
    const [left, right] = outputs[0];
    if (this.failed || !left || !right) return true;
    try {
      for (let i = 0; i < left.length; i++) {
        if (sampleRate === 48000) {
          this.nativeSample();
          left[i] = this.nativeLeft * this.gain;
          right[i] = this.nativeRight * this.gain;
        } else {
          const fraction = this.phase / sampleRate;
          left[i] = (this.currentLeft + (this.nextLeft - this.currentLeft) * fraction) * this.gain;
          right[i] = (this.currentRight + (this.nextRight - this.currentRight) * fraction) * this.gain;
          this.phase += 48000;
          while (this.phase >= sampleRate) {
            this.phase -= sampleRate;
            this.currentLeft = this.nextLeft; this.currentRight = this.nextRight;
            this.nativeSample();
            this.nextLeft = this.nativeLeft; this.nextRight = this.nativeRight;
          }
        }
      }
      this.recording.capture(left, right);
      for (let i = 0; i < left.length; i++) {
        this.peak = Math.max(this.peak, Math.abs(left[i]), Math.abs(right[i]));
        if (left[i] !== 0 || right[i] !== 0) this.audibleFrames++;
      }
      if (++this.callbacks % 20 === 0) {
        this.port.postMessage({ type: "stats", voices: this.wasm.rustias_voices(), capacity:this.wasm.rustias_voice_capacity(), frames: this.wasm.rustias_frames(), audibleFrames: this.audibleFrames, peak: this.peak, callbacks: this.callbacks, sequence: this.sequencer.status(),modulation:this.modulation.status() });
        this.peak = 0;
      }
    } catch (error) { this.fail(error); }
    return true;
  }
}
registerProcessor("rustias", RustiasProcessor);
