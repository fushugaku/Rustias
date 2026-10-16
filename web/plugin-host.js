// Opt-in native host. Ordinary browser/Android builds never open this bridge.
const native=window.__RUSTIAS_PLUGIN__&&window.webkit?.messageHandlers?.rustiasHost;
const serialize=value=>JSON.stringify(value,(_key,item)=>ArrayBuffer.isView(item)?Array.from(item):item);
export const pluginHost=native?createHost():null;
function createHost(){
  // A DAW owns one editor window; browser-only links that open another interface
  // tab do not belong in that window.
  const style=document.createElement('style');style.textContent='.interface-navigation{display:none}';document.head.append(style);
  const NativeWorker=window.Worker;
  window.Worker=class extends NativeWorker{
    constructor(url,options){const name=new URL(url,location.href).pathname.split('/').at(-1),source=window.__RUSTIAS_PLUGIN_WORKERS__?.[name];
      if(!source){super(url,options);return;}
      const blob=URL.createObjectURL(new Blob([source],{type:'text/javascript'}));super(blob,{...options,type:'classic'});setTimeout(()=>URL.revokeObjectURL(blob),1000);
    }
  };
  let port,restore,parameter,request=0;const pending=new Map();
  const send=value=>native.postMessage(serialize(value));
  const host={initial:window.__RUSTIAS_PLUGIN__,applyingAutomation:false,onRestore:fn=>restore=fn,onParameter:fn=>parameter=fn,saveSession:data=>send({kind:'session',data})};
  window.__rustiasPluginReceive=data=>{
    if(data.type==='digest'){pending.get(data.request)?.(new Uint8Array(data.bytes).buffer);pending.delete(data.request);return;}
    if(data.type==='host-parameter'){host.applyingAutomation=true;try{parameter?.(data.id,data.value);}finally{host.applyingAutomation=false;}return;}
    if(data.type==='record-chunk'&&Array.isArray(data.pcm))data.pcm=Int16Array.from(data.pcm);
    port?.onmessage?.({data});
  };
  host.syncParameters=()=>host.initial.parameters?.forEach((value,id)=>window.__rustiasPluginReceive({type:'host-parameter',id,value}));
  window.__rustiasPluginRestore=data=>{host.initial=data;restore?.(data.session);host.syncParameters();};
  host.digest=async buffer=>{
    if(crypto.subtle)return crypto.subtle.digest('SHA-256',buffer);
    const id=++request,bytes=await base64(new Blob([buffer]));return new Promise(resolve=>{pending.set(id,resolve);send({kind:'digest',request:id,bytes});});
  };
  host.context=()=>{
    let offline;const context={sampleRate:host.initial.sampleRate??48000,state:'running',destination:{},audioWorklet:{addModule:async()=>{}},onstatechange:null,
      decodeAudioData:buffer=>(offline??=new OfflineAudioContext(1,1,48000)).decodeAudioData(buffer),resume:async()=>{context.state='running';send({kind:'power',enabled:true});context.onstatechange?.();},suspend:async()=>{context.state='suspended';send({kind:'power',enabled:false});context.onstatechange?.();},close:async()=>{context.state='closed';}};
    return context;
  };
  host.node=()=>{port={onmessage:null,postMessage:data=>{if(!host.applyingAutomation)send({kind:'audio',data});}};return {port,connect(){queueMicrotask(()=>port.onmessage?.({data:{type:'ready',sampleRate:host.initial.sampleRate??48000}}));},disconnect(){}};};
  host.ready=async module=>{
    const probe=(file,data,expected)=>new Promise((resolve,reject)=>{
      const worker=new Worker(new URL(file,location.href)),timer=setTimeout(()=>{worker.terminate();reject(new Error('The plugin worker did not start.'));},10000);
      worker.onmessage=({data})=>{clearTimeout(timer);worker.terminate();expected(data)?resolve():reject(new Error(data.error||'The plugin worker failed.'));};
      worker.onerror=event=>{clearTimeout(timer);worker.terminate();reject(new Error(event.message));};worker.postMessage(data);
    });
    await probe('recordings-worker.js',{type:'probe',request:-1},data=>data.error==='Unknown recording operation.');
    await probe('rdl-worker.js',{module,buffer:new ArrayBuffer(0)},data=>typeof data.error==='string');
    send({kind:'editor-ready'});
  };
  document.addEventListener('click',async event=>{const link=event.target.closest('a[download]');if(!link?.href||!link.href.startsWith('blob:'))return;event.preventDefault();const blob=await(await fetch(link.href)).blob();send({kind:'export',name:link.download||'Rustias-export',bytes:await base64(blob)});},true);
  return host;
}
function base64(blob){return new Promise((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(reader.result.slice(reader.result.indexOf(',')+1));reader.onerror=reject;reader.readAsDataURL(blob);});}
