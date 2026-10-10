import {makePicker,makeDial} from "./panel.js";
import {emptySamples,validateSamples,sampleValues} from './sample-state.js';
export {emptySamples,validateSamples} from './sample-state.js';
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
export function createDrumSamples({getInstrument,isDrum,onAssign,onKit,onChange,onError,ensureAudio,parameters,getDrumValues,getEditingSample,onLibraryChange,onPreview}){
  const store=new SampleStore();let manifest=[],custom=[],config=emptySamples(),connection,requestId=0,loading=0,generation=0,nextAsset=0,nextProfile=0;
  const assets=new Map(),profiles=new Map(),assetTasks=new Map(),profileTasks=new Map();
  const pending=new Map(),loaded=new Map(),versions=Array(16).fill(0),decoded=new Map();
  const host=document.createElement("div");host.className="sample-controls";host.innerHTML='<div class="sample-source"><label>Source</label><div id="sample-source"></div></div><div class="sample-mode"><label>Playback</label><div id="sample-mode"></div></div><div class="sample-gain"><label>Gain · dB</label></div><button id="sample-upload">Upload</button><input id="sample-file" type="file" accept="audio/*,.wav,.aif,.aiff,.flac,.ogg,.mp3,.m4a" hidden>';
  $(".module-drums").append(host);
  const kitButton=document.createElement("button");kitButton.id="sample-kit";kitButton.className="sample-kit";kitButton.textContent="808 kit";$(".module-drums .module-heading").append(kitButton);
  const kit909=document.createElement('button');kit909.id='sample-kit-909';kit909.className='sample-kit';kit909.textContent='909 kit';$('.module-drums .module-heading').append(kit909);
  const picker=makePicker({label:"Drum sample source",value:"synth",options:[{value:"synth",label:"Synth engine"}],searchable:true,searchLabel:'Search samples',onChange:source=>setSource(getInstrument(),source).catch(onError)});
  const playback=makePicker({label:"Sample playback",value:0,options:modes,onChange:mode=>{
    const editing=getEditingSample?.();
    if(editing){const profile=findProfile(editing.timbre,editing.source);profile.mode=mode;sendProfile(profile);}
    else{const instrument=getInstrument();config.slots[instrument].mode=mode;connection?.node.port.postMessage({type:"sample-mode",instrument,mode});}playback.render(mode);onChange();
  }});
  const gain=makeDial({label:'Drum Kit gain',min:-24,max:24,defaultValue:12,read:()=>config.kitGain,onChange:value=>{config.kitGain=value;gain.render();connection?.node.port.postMessage({type:'drum-gain',value});onChange({kind:'kit-gain'});},macroTarget:()=>({kind:'kit-gain'}),format:value=>`${value>0?'+':''}${value} dB`});
  $('.sample-gain').append(gain.button,gain.number);
  $("#sample-source").append(picker.button);$("#sample-mode").append(playback.button);
  function rebuildOptions(){
    picker.options=[{value:"synth",label:"Synth engine"},...manifest.map(s=>({value:s.id,label:s.name})),...custom.map(s=>({value:s.id,label:s.name}))];
    for(const slot of [...config.slots,...config.library])if(!picker.options.some(o=>o.value===slot.source))picker.options.push({value:slot.source,label:slot.name??"Missing local sample"});
    onLibraryChange?.();
  }
  function render(){
    const editing=getEditingSample?.(),slot=editing?findProfile(editing.timbre,editing.source):config.slots[getInstrument()];picker.render(slot?.source??"synth");playback.render(slot?.mode??0);gain.render();
    picker.button.disabled=!!editing||!isDrum();playback.button.disabled=!editing&&(!isDrum()||slot.source==="synth");
    host.classList.toggle("inactive",!isDrum()&&!editing);host.setAttribute("aria-busy",loading>0);$("#sample-upload").textContent=loading?"Loading…":editing?"Preview":"Upload";
  }
  const ready=(async()=>{
    const response=await fetch(new URL("./samples/manifest.json",import.meta.url));if(!response.ok)throw new Error("Could not load the drum sample library.");
    const library=await response.json(),expected=library.banks?.reduce((sum,bank)=>sum+bank.count,0)??64;if(library.samples?.length!==expected)throw new Error("The drum library is incomplete.");manifest=library.samples;
    try{custom=await store.list();}catch(error){onError(new Error(`Could not open the local sample library: ${error.message}`));}
    rebuildOptions();render();
  })();ready.catch(onError);
  function requestUpload(instrument,data,mode,asset){
    const request=++requestId;return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{pending.delete(request);reject(new Error("The sample did not reach the audio engine."));},15000);
      pending.set(request,{resolve,reject,timer});connection.node.port.postMessage(asset?{type:"library-sample",request,asset,data}:{type:"sample",request,instrument,data,mode},[data.buffer]);
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
  function pruneDecoded(){const used=new Set([...config.slots,...config.library].map(s=>s.source));for(const source of decoded.keys())if(!used.has(source))decoded.delete(source);}
  async function setSource(instrument,source){
    await ready;if(config.slots[instrument].source===source)return;
    const previous=config.slots[instrument].source,name=picker.options.find(o=>o.value===source)?.label;
    config.slots[instrument]={...config.slots[instrument],source,...(source!=="synth"?{name}:{name:undefined})};versions[instrument]++;
    pruneDecoded();onAssign(instrument,previous==="synth"&&source!=="synth");render();onChange();
    await ensureAudio();await loadSlot(instrument);
  }
  let uploadInstrument=0,sequenceUpload;
  $("#sample-upload").addEventListener("click",()=>{const editing=getEditingSample?.();if(editing){onPreview(editing.timbre,editing.source);return;}sequenceUpload=null;uploadInstrument=getInstrument();$("#sample-file").click();});
  $("#sample-file").addEventListener("change",async event=>{
    try{const file=event.target.files[0];if(!file)return;if(file.size>20*1024*1024)throw new Error("Choose a sample smaller than 20 MB.");
      await ensureAudio();const id=`custom:${crypto.randomUUID()}`;loading++;render();
      try{await decode(id,file);await store.put({id,name:file.name,blob:file});custom.push({id,name:file.name});rebuildOptions();if(sequenceUpload){const target=sequenceUpload;sequenceUpload=null;await ensureLibrary(target.timbre,id);await target.onReady(id);}else await setSource(uploadInstrument,id);}finally{loading--;render();}
    }catch(error){onError(error);}finally{event.target.value="";}
  });
  async function loadKit(bank){
    try{await ready;const categories=["kick","snare","clap","closed-hat","open-hat","low-tom","mid-tom","high-tom","cowbell","rim","maracas","claves","low-conga","mid-conga","high-conga","cymbal"];
      const samples=bank==='909'?['BT7A0D7','ST7T7S7','HANDCLP1','HHCD4','HHOD6','LT7D7','MT7D7','HT7D7','RIM127','RIDED4','CSHD4','BT0AADA','ST0TAS7','HANDCLP2','HHCD8','HHOD8'].map(file=>manifest.find(s=>s.id===`909:${file.toLowerCase()}`)):categories.map(category=>{const choices=manifest.filter(s=>s.id.startsWith('808:')&&s.category===category);return choices[Math.floor(choices.length/2)];});
      if(samples.some(sample=>!sample))throw new Error(`The ${bank} kit is incomplete.`);
      config={...config,version:3,slots:samples.map(s=>({source:s.id,name:s.name,mode:0}))};
      for(let i=0;i<16;i++)versions[i]++;pruneDecoded();onKit(bank);render();onChange();await ensureAudio();await Promise.all(config.slots.map((_,i)=>loadSlot(i)));
    }catch(error){onError(error);}
  }
  kitButton.addEventListener('click',()=>loadKit('808'));kit909.addEventListener('click',()=>loadKit('909'));
  const keyFor=(timbre,source)=>`${timbre}:${source}`;
  function findProfile(timbre,source){return config.library.find(p=>p.timbre===timbre&&p.source===source);}
  function makeProfile(timbre,source,copy){
    let profile=findProfile(timbre,source);if(profile)return profile;
    if(config.library.length>=1024)throw new Error('The patch sample library is full.');
    const slot=config.slots.findIndex(s=>s.source===source);
    const values=copy?[...copy.values]:slot>=0?[...getDrumValues(slot)]:sampleValues(parameters.map(p=>p.default));
    profile={source,timbre,mode:copy?.mode??(slot>=0?config.slots[slot].mode:0),name:sampleName(source),values};config.library.push(profile);return profile;
  }
  function sampleName(source){return picker.options.find(o=>o.value===source)?.label??config.library.find(p=>p.source===source)?.name??source;}
  function sendProfile(profile){
    const loaded=profiles.get(keyFor(profile.timbre,profile.source)),asset=assets.get(profile.source);if(!loaded||!asset||!connection)return;
    connection.node.port.postMessage({type:'library-profile',id:loaded.id,asset,timbre:profile.timbre,source:profile.source,mode:profile.mode,values:profile.values});
  }
  async function loadAsset(source,epoch){
    if(assets.has(source))return assets.get(source);
    if(assetTasks.has(source))return assetTasks.get(source);
    const task=(async()=>{const data=await decode(source);if(epoch!==generation||!connection)return null;
      const id=++nextAsset;await requestUpload(null,data.slice(),null,id);if(epoch!==generation)return null;assets.set(source,id);return id;})();
    assetTasks.set(source,task);try{return await task;}finally{if(assetTasks.get(source)===task)assetTasks.delete(source);}
  }
  async function loadProfile(profile){
    if(!connection)return;await ready;const key=keyFor(profile.timbre,profile.source),epoch=generation;
    if(profiles.has(key))return;
    if(profileTasks.has(key))return profileTasks.get(key);
    loading++;render();const task=(async()=>{const asset=await loadAsset(profile.source,epoch);if(!asset||epoch!==generation)return;
      profiles.set(key,{id:++nextProfile});sendProfile(profile);})();profileTasks.set(key,task);
    try{await task;}finally{if(profileTasks.get(key)===task)profileTasks.delete(key);loading--;render();}
  }
  async function ensureLibrary(timbre,source){await ready;const existed=!!findProfile(timbre,source),profile=makeProfile(timbre,source);if(!existed)onChange();await ensureAudio();if(findProfile(timbre,source)===profile)await loadProfile(profile);return profile;}
  function resetLibrary(){generation++;assets.clear();profiles.clear();assetTasks.clear();profileTasks.clear();connection?.node.port.postMessage({type:'library-reset'});}
  render();
  return {ready,render,setGain:value=>{config.kitGain=value;gain.render();connection?.node.port.postMessage({type:'drum-gain',value});},validateConfig:value=>validateSamples(value,parameters),getConfig:()=>structuredClone(config),assigned:i=>config.slots[i].source!=="synth",
    editProfile:(timbre,source)=>makeProfile(timbre,source),options:()=>picker.options.filter(o=>o.value!=='synth'),sampleName,findProfile,ensureLibrary,
    controlLibrary(timbre,source,parameter,value){const profile=findProfile(timbre,source);if(!profile)return;profile.values[parameter]=value;const loaded=profiles.get(keyFor(timbre,source));if(loaded)connection?.node.port.postMessage({type:'library-control',id:loaded.id,parameter,value});},
    copyProfiles(from,to,sources){if(from===to)return;for(const source of new Set(sources)){const original=findProfile(from,source)??makeProfile(from,source);if(!findProfile(to,source)){makeProfile(to,source,original);}}onChange();},
    uploadForSequence(timbre,onReady){sequenceUpload={timbre,onReady};$('#sample-file').click();},
    acceptVolumes(values,program,midi){for(const {timbre,source,value} of values??[]){const profile=findProfile(timbre,source);if(profile)profile.values[117]=value;}
      if(midi&&(midi[0]&240)===176&&midi[1]===7)for(const profile of config.library){const channel=program.timbres[profile.timbre][72];if(profile.values[116]&&(channel===16?program.timbres[0][148]:channel)===(midi[0]&15))profile.values[117]=midi[2];}
    },
    instrumentNames:()=>config.slots.map((slot,i)=>slot.source==='synth'?`Drum ${String(i+1).padStart(2,'0')}`:sampleName(slot.source)),
    async setConfig(value){config=validateSamples(value,parameters);resetLibrary();for(let i=0;i<16;i++)versions[i]++;pruneDecoded();rebuildOptions();render();if(connection){connection.node.port.postMessage({type:'drum-gain',value:config.kitGain});await Promise.all(config.slots.map((_,i)=>loadSlot(i)));}},
    async attach(node,context){connection={node,context};loaded.clear();resetLibrary();await ready;connection.node.port.postMessage({type:'drum-gain',value:config.kitGain});await Promise.all(config.slots.map((_,i)=>loadSlot(i)));},
    async waitReady(){await ready;if(connection)await Promise.all(config.slots.map((_,i)=>loadSlot(i)));},
    detach(){connection=undefined;loaded.clear();resetLibrary();for(const request of pending.values()){clearTimeout(request.timer);request.reject(new Error("The audio connection closed."));}pending.clear();},
    handleMessage(data){if(data.type!=="sample-ready")return false;const request=pending.get(data.request);if(request){clearTimeout(request.timer);pending.delete(data.request);if(data.ok)request.resolve();else request.reject(new Error(data.message??"The Rust engine rejected this sample."));}return true;}
  };
}
