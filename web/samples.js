import {makePicker} from "./panel.js";

export const emptySamples=()=>({version:1,slots:Array.from({length:16},()=>({source:"synth",mode:0}))});
export function validateSamples(value){
  if(value==null)return emptySamples();
  if(value.version!==1||!Array.isArray(value.slots)||value.slots.length!==16)throw new Error("Invalid drum sample assignments.");
  return {version:1,slots:value.slots.map(slot=>{
    if(typeof slot?.source!=="string"||!(/^(synth|808:[a-z0-9-]+|custom:[a-zA-Z0-9-]+)$/.test(slot.source))||![0,1,2].includes(slot.mode))throw new Error("Invalid drum sample assignment.");
    return {source:slot.source,mode:slot.mode,...(slot.name?{name:String(slot.name).slice(0,100)}:{})};
  })};
}
class SampleStore {
  async open(){
    this.database??=new Promise((resolve,reject)=>{
      const request=indexedDB.open("rustias-samples",1);
      request.onupgradeneeded=()=>request.result.createObjectStore("files",{keyPath:"id"});
      request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
    });return this.database;
  }
  async operation(mode,method,value){
    const db=await this.open();return new Promise((resolve,reject)=>{
      const transaction=db.transaction("files",mode),request=transaction.objectStore("files")[method](value);
      let result;request.onsuccess=()=>{result=request.result;};transaction.oncomplete=()=>resolve(result);transaction.onerror=()=>reject(transaction.error);transaction.onabort=()=>reject(transaction.error);
    });
  }
  async list(){return (await this.operation("readonly","getAll")).map(({id,name})=>({id,name}));}
  get(id){return this.operation("readonly","get",id);}
  put(value){return this.operation("readwrite","put",value);}
}
const $=selector=>document.querySelector(selector);
const modes=[{value:0,label:"One-shot"},{value:1,label:"Gate"},{value:2,label:"Loop"}];
export function createDrumSamples({getInstrument,isDrum,onAssign,onKit,onChange,onError,ensureAudio}){
  const store=new SampleStore();let manifest=[],custom=[],config=emptySamples(),connection,requestId=0,loading=0;
  const pending=new Map(),loaded=new Map(),versions=Array(16).fill(0),decoded=new Map();
  const host=document.createElement("div");host.className="sample-controls";host.innerHTML='<div class="sample-source"><label>Source</label><div id="sample-source"></div></div><div class="sample-mode"><label>Playback</label><div id="sample-mode"></div></div><button id="sample-upload">Upload</button><input id="sample-file" type="file" accept="audio/*,.wav,.aif,.aiff,.flac,.ogg,.mp3,.m4a" hidden>';
  $(".module-drums").append(host);
  const kitButton=document.createElement("button");kitButton.id="sample-kit";kitButton.className="sample-kit";kitButton.textContent="808 kit";$(".module-drums .module-heading").append(kitButton);
  const picker=makePicker({label:"Drum sample source",value:"synth",options:[{value:"synth",label:"Synth engine"}],onChange:source=>setSource(getInstrument(),source).catch(onError)});
  const playback=makePicker({label:"Sample playback",value:0,options:modes,onChange:mode=>{
    const instrument=getInstrument();config.slots[instrument].mode=mode;playback.render(mode);connection?.node.port.postMessage({type:"sample-mode",instrument,mode});onChange();
  }});
  $("#sample-source").append(picker.button);$("#sample-mode").append(playback.button);
  function rebuildOptions(){
    picker.options=[{value:"synth",label:"Synth engine"},...manifest.map(s=>({value:s.id,label:s.name})),...custom.map(s=>({value:s.id,label:s.name}))];
    for(const slot of config.slots)if(!picker.options.some(o=>o.value===slot.source))picker.options.push({value:slot.source,label:slot.name??"Missing local sample"});
  }
  function render(){
    const slot=config.slots[getInstrument()];picker.render(slot.source);playback.render(slot.mode);
    picker.button.disabled=!isDrum();playback.button.disabled=!isDrum()||slot.source==="synth";
    host.classList.toggle("inactive",!isDrum());host.setAttribute("aria-busy",loading>0);$("#sample-upload").textContent=loading?"Loading…":"Upload";
  }
  const ready=(async()=>{
    const response=await fetch(new URL("./samples/manifest.json",import.meta.url));if(!response.ok)throw new Error("Could not load the drum sample library.");
    const library=await response.json();if(library.samples?.length!==64)throw new Error("The drum library is incomplete.");manifest=library.samples;
    try{custom=await store.list();}catch(error){onError(new Error(`Could not open the local sample library: ${error.message}`));}
    rebuildOptions();render();
  })();ready.catch(onError);
  function requestUpload(instrument,data,mode){
    const request=++requestId;return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{pending.delete(request);reject(new Error("The sample did not reach the audio engine."));},15000);
      pending.set(request,{resolve,reject,timer});connection.node.port.postMessage({type:"sample",request,instrument,data,mode},[data.buffer]);
    });
  }
  async function decode(source,blob){
    if(decoded.has(source))return decoded.get(source);
    const task=(async()=>{
      if(!blob){const builtIn=manifest.find(s=>s.id===source);if(builtIn){const response=await fetch(new URL(`./samples/${builtIn.file}`,import.meta.url));if(!response.ok)throw new Error(`Could not load ${builtIn.name}.`);blob=await response.blob();}
        else{const local=await store.get(source);if(!local)throw new Error(`Local sample “${config.slots.find(s=>s.source===source)?.name??source}” is missing. Upload it again on this device.`);blob=local.blob;}}
      const audio=await connection.context.decodeAudioData(await blob.arrayBuffer());
      if(audio.duration>30||audio.length<2)throw new Error("Choose an audio sample up to 30 seconds long.");
      const frames=Math.round(audio.duration*48000),channels=Array.from({length:audio.numberOfChannels},(_,i)=>audio.getChannelData(i)),data=new Float32Array(frames);
      const ratio=audio.sampleRate/48000;
      for(let i=0;i<frames;i++){const position=i*ratio,index=Math.min(Math.floor(position),audio.length-1),next=Math.min(index+1,audio.length-1),fraction=position-index;let value=0;
        for(const channel of channels)value+=channel[index]+(channel[next]-channel[index])*fraction;data[i]=Math.max(-1,Math.min(1,value/channels.length));}
      return data;
    })();decoded.set(source,task);try{return await task;}catch(error){decoded.delete(source);throw error;}
  }
  async function loadSlot(instrument){
    if(!connection)return;await ready;const slot={...config.slots[instrument]},version=versions[instrument];
    if(loaded.get(instrument)===slot.source){connection.node.port.postMessage({type:"sample-mode",instrument,mode:slot.mode});return;}
    if(slot.source==="synth"){connection.node.port.postMessage({type:"sample-clear",instrument});loaded.set(instrument,"synth");return;}
    loading++;render();try{const data=await decode(slot.source);if(version!==versions[instrument])return;
      await requestUpload(instrument,data.slice(),config.slots[instrument].mode);if(version===versions[instrument])loaded.set(instrument,slot.source);
    }finally{loading--;render();}
  }
  function pruneDecoded(){const used=new Set(config.slots.map(s=>s.source));for(const source of decoded.keys())if(!used.has(source))decoded.delete(source);}
  async function setSource(instrument,source){
    await ready;if(config.slots[instrument].source===source)return;
    const previous=config.slots[instrument].source,name=picker.options.find(o=>o.value===source)?.label;
    config.slots[instrument]={...config.slots[instrument],source,...(source!=="synth"?{name}:{name:undefined})};versions[instrument]++;
    pruneDecoded();onAssign(instrument,previous==="synth"&&source!=="synth");render();onChange();
    await ensureAudio();await loadSlot(instrument);
  }
  let uploadInstrument=0;
  $("#sample-upload").addEventListener("click",()=>{uploadInstrument=getInstrument();$("#sample-file").click();});
  $("#sample-file").addEventListener("change",async event=>{
    try{const file=event.target.files[0];if(!file)return;if(file.size>20*1024*1024)throw new Error("Choose a sample smaller than 20 MB.");
      await ensureAudio();const id=`custom:${crypto.randomUUID()}`;loading++;render();
      try{await decode(id,file);await store.put({id,name:file.name,blob:file});custom.push({id,name:file.name});rebuildOptions();await setSource(uploadInstrument,id);}finally{loading--;render();}
    }catch(error){onError(error);}finally{event.target.value="";}
  });
  kitButton.addEventListener("click",async()=>{
    try{await ready;const categories=["kick","snare","clap","closed-hat","open-hat","low-tom","mid-tom","high-tom","cowbell","rim","maracas","claves","low-conga","mid-conga","high-conga","cymbal"];
      config={version:1,slots:categories.map(category=>{const choices=manifest.filter(s=>s.category===category),s=choices[Math.floor(choices.length/2)];return {source:s.id,name:s.name,mode:0};})};
      for(let i=0;i<16;i++)versions[i]++;pruneDecoded();onKit();render();onChange();await ensureAudio();await Promise.all(config.slots.map((_,i)=>loadSlot(i)));
    }catch(error){onError(error);}
  });
  render();
  return {ready,render,validateConfig:validateSamples,getConfig:()=>structuredClone(config),assigned:i=>config.slots[i].source!=="synth",
    async setConfig(value){config=validateSamples(value);for(let i=0;i<16;i++)versions[i]++;pruneDecoded();rebuildOptions();render();if(connection)await Promise.all(config.slots.map((_,i)=>loadSlot(i)));},
    async attach(node,context){connection={node,context};loaded.clear();await ready;await Promise.all(config.slots.map((_,i)=>loadSlot(i)));},
    async waitReady(){await ready;if(connection)await Promise.all(config.slots.map((_,i)=>loadSlot(i)));},
    detach(){connection=undefined;loaded.clear();for(const request of pending.values()){clearTimeout(request.timer);request.reject(new Error("The audio connection closed."));}pending.clear();},
    handleMessage(data){if(data.type!=="sample-ready")return false;const request=pending.get(data.request);if(request){clearTimeout(request.timer);pending.delete(data.request);if(data.ok)request.resolve();else request.reject(new Error(data.message??"The Rust engine rejected this sample."));}return true;}
  };
}
