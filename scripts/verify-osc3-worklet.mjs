import assert from 'node:assert/strict';
import vm from 'node:vm';
import {RecordingTap} from '../web/recording-tap.js';
import {ModulationAutomation,emptyModulation} from '../web/modulation.js';
import {ModulationHost} from '../web/modulation-host.js';
import {emptyMacros} from '../web/macros.js';
import {audioCircuit,defaultCircuit} from '../web/circuit.js';
import {MAX_TIMBRES,INITIAL_TIMBRES,OSC3_BASE,timbreArray} from '../web/limits.js';
import {validSampleSource} from '../web/sample-state.js';

// Render the actual browser message path into arrays. No audio device is opened.
export function verifyOsc3Worklet({module,parameters,workletSource,sequenceSource}){
  const b=OSC3_BASE,reports=[];
  const rms=a=>Math.sqrt(a.reduce((sum,x)=>sum+x*x,0)/a.length);
  const difference=(a,c)=>rms(a.map((x,i)=>x-c[i]));
  const frequency=(audio,rate)=>{let crossings=0;for(let i=2001;i<audio.length;i++)if(audio[i]>=0&&audio[i-1]<0)crossings++;return crossings*rate/(audio.length-2000);};
  for(const sampleRate of [48000,44100]){
    const processors=[];let initialEngine;
    function fixture(options={},slot=0){
      if(!processors[slot]){
        let Processor;const messages=[];
        vm.runInNewContext(sequenceSource+'\n'+workletSource.replace(/^import .*;\n/gm,''),{
          sampleRate,WebAssembly,Float32Array,Uint8Array,validSampleSource,RecordingTap,MAX_TIMBRES,INITIAL_TIMBRES,timbreArray,ModulationAutomation,ModulationHost,
          AudioWorkletProcessor:class{constructor(){this.port={postMessage:data=>messages.push(data)};}},registerProcessor:(_name,p)=>Processor=p,
        });
        processors[slot]={p:new Processor({processorOptions:{module,gain:1}}),messages};
      }
      const {p,messages}=processors[slot],wasm=p.wasm;messages.length=0;
      const send=data=>{p.port.onmessage({data});assert.equal(p.failed,false);};
      const save=()=>{const size=wasm.rustias_save();return JSON.parse(Buffer.from(new Uint8Array(wasm.memory.buffer,wasm.rustias_preset_buffer(),size)));};
      initialEngine??=save();const engine=structuredClone(initialEngine),v=engine.timbres[0];
      for(const [id,value]of Object.entries({17:0,18:0,19:127,9:127,3:0,4:127,5:127,6:8,[b]:3,[b+1]:0,[b+2]:0,[b+3]:0,[b+4]:64,[b+5]:64,[b+6]:0,[b+7]:0,...options.values}))v[+id]=value;
      if(options.legacy)for(const values of [...engine.timbres,...engine.drums])values.length=options.legacy;
      send({type:'load',program:engine});
      const circuits=Array.from({length:MAX_TIMBRES},()=>audioCircuit(defaultCircuit()));
      if(options.graph)circuits[0]=audioCircuit(options.graph);
      circuits.forEach((circuit,timbre)=>send({type:'circuit',timbre,circuit}));
      const state={config:emptyModulation(),macros:emptyMacros(),targets:[],engine:save(),effects:save().effects,circuits};
      send({type:'modulation',state});
      const render=(frames=24000)=>{const audio=[];for(let i=0;i<frames;i+=128){const left=new Float32Array(128),right=new Float32Array(128);p.process([],[[left,right]]);assert.equal(p.failed,false);assert.ok([...left,...right].every(Number.isFinite));audio.push(...left);}return audio;};
      const control=(id,value)=>{send({type:'control',timbre:0,parameter:id,value});assert.equal(wasm.rustias_value(0,id),value);state.engine=save();send({type:'modulation',state});};
      send({type:'note',timbre:0,note:69,velocity:100});render(4096);
      return {p,wasm,send,save,state,control,render,messages};
    }
    const tone=fixture(),base=tone.render();assert.ok(Math.abs(frequency(base,sampleRate)-440)<6);
    tone.control(b+4,76);const octave=tone.render();assert.ok(Math.abs(frequency(octave,sampleRate)-880)<6,'UI control messages retune a held OSC 3 by twelve semitones');
    tone.control(b+4,64);tone.control(b+5,127);const fine=tone.render();assert.ok(Math.abs(frequency(fine,sampleRate)-466.16)<6,'UI Fine messages reach the actual device-rate output');
    assert.equal(tone.wasm.rustias_voices(),1,'Manual edits keep the existing held voice');
    const pairs=[
      {name:'Waveform',values:{[b]:3},id:b,value:1},
      {name:'CTRL 1',values:{[b]:0},id:b+2,value:100},
      {name:'Mode',values:{[b]:0,[b+2]:80,[b+3]:60},id:b+1,value:3},
      {name:'CTRL 2',values:{[b]:3,[b+1]:3,[b+2]:80},id:b+3,value:100},
      {name:'Noise CTRL 1',values:{[b]:4},id:b+2,value:100},
      {name:'Noise CTRL 2',values:{[b]:4,[b+2]:80},id:b+3,value:100},
      {name:'Sync',values:{0:3,[b]:3,[b+4]:59},id:b+6,value:2},
    ];
    for(const graphEnabled of [false,true])for(const test of pairs){
      const graph=defaultCircuit();graph.enabled=graphEnabled;
      const dry=fixture({values:test.values,graph}),wet=fixture({values:test.values,graph},1);
      // Same phase history and audio clock; a no-op edit would produce identical PCM.
      wet.control(test.id,test.value);
      if(graphEnabled)wet.send({type:'circuit',timbre:0,circuit:audioCircuit(graph)});
      const a=dry.render(),c=wet.render();assert.ok(difference(a,c)>.0001,`${test.name} changes real worklet audio with Routing ${graphEnabled?'On':'Off'}`);
      if(test.name==='Sync'&&!graphEnabled)assert.ok(Math.abs(frequency(c,sampleRate)-440)<6,'OSC 1 hard sync reaches the browser output');
    }
    const graph=defaultCircuit();graph.enabled=true;graph.wires.push({from:14,to:2,port:'pitch'});
    const pitch=fixture({graph});assert.ok(frequency(pitch.render(),sampleRate)>800,'Build Pitch CV reaches OSC 3 through browser circuit messages');
    const legacy=fixture({legacy:171});assert.equal(legacy.wasm.rustias_value(0,b),4);legacy.control(b,3);legacy.control(b+4,76);assert.ok(Math.abs(frequency(legacy.render(),sampleRate)-880)<6,'Old program imports retain working live OSC 3 controls');

    const ordered=fixture({values:{[b]:4}}),host=ordered.p.modulationHost;
    const change=(parameter,value,kind='synth')=>({target:kind==='drum'?{kind,instrument:0,parameter}:{kind,timbre:0,parameter},value});
    host.write([change(b+1,3),change(b,3)]);
    assert.equal(ordered.wasm.rustias_value(0,b+1),3,'Simultaneous Wave/Mode modulation must leave Noise before applying VPM');
    host.write([change(b+1,3),change(b,4)]);assert.equal(ordered.wasm.rustias_value(0,b+1),0);
    host.write([change(b+1,3,'drum'),change(b,3,'drum')]);assert.equal(ordered.save().drums[0][b+1],3,'Drum OSC 3 follows the same modulation dependency');
    ordered.send({type:'library-sample',asset:90,request:90,data:new Float32Array(512).fill(.1)});
    ordered.send({type:'library-profile',id:90,asset:90,timbre:0,source:'custom:osc3-dependency',mode:2,values:ordered.save().timbres[0]});
    const sampleChange=(parameter,value)=>({target:{kind:'sample',timbre:0,source:'custom:osc3-dependency',parameter},value});
    host.write([sampleChange(b+1,3),sampleChange(b,3)]);assert.equal(ordered.wasm.rustias_library_value(90,b+1),3);
    host.write([sampleChange(b+1,3),sampleChange(b,4)]);assert.equal(ordered.wasm.rustias_library_value(90,b+1),0,'Sample sound profiles preserve the Noise/Formant constraint during automation');
    assert.ok(!ordered.messages.some(m=>m.type==='error'||m.type==='warning'));
    reports.push({sampleRate,a4:Math.round(frequency(base,sampleRate)),semitone12:Math.round(frequency(octave,sampleRate)),fine63:Number(frequency(fine,sampleRate).toFixed(1))});
  }
  return {features:'OSC 3 browser control messages: held Semi/Fine, Mode, CTRL1/2, Sync, Pitch CV, Routing On/Off, old-program loading and simultaneous Wave/Mode modulation; silent buffer verification',reports};
}
