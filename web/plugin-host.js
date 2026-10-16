// Opt-in native host. Ordinary browser/Android builds never open this bridge.
import {installPluginCrypto} from './plugin-platform.js';
const native=window.__RUSTIAS_PLUGIN__&&window.webkit?.messageHandlers?.rustiasHost;
const serialize=value=>JSON.stringify(value,(_key,item)=>ArrayBuffer.isView(item)?Array.from(item):item);
export const pluginHost=native?createHost():null;
function createHost(){
  installPluginCrypto();
  // A DAW owns one editor window; browser-only links that open another interface
  // tab do not belong in that window.
  document.documentElement.classList.add('native-plugin');
  const style=document.createElement('style');style.textContent=`
    .interface-navigation,#seq-play,#seq-reset,#midi{display:none!important}
    .plugin-modulation-tabs{display:flex;gap:4px;flex-wrap:wrap}
    .plugin-modulation-tabs button{min-width:38px}
    .plugin-modulation-tabs [aria-selected="true"]{color:var(--accent);border-color:currentColor}
    .plugin-modulation-panels{min-width:0}
    .plugin-modulation-rack .modulation-panel{border-top:0}
    .plugin-modulation-rack .modulation-heading h3{display:none}
    .new-interface.native-plugin #new-sequence-dock{overflow:auto}
    .new-interface.native-plugin .plugin-modulation-rack{padding:8px 12px}
    .new-interface.native-plugin .plugin-modulation-rack>.sequence-heading{margin:0 0 8px}
    .new-interface.native-plugin .plugin-workspace .nw-transport{flex-wrap:wrap}
    .plugin-workspace-tabs{display:flex;align-items:center;gap:4px;padding:2px;background:var(--control-inset);border:1px solid var(--line-subtle);border-radius:6px;flex:none}
    .new-interface .plugin-workspace-tabs button{height:32px;min-height:32px;border:0;background:transparent;color:var(--text-secondary);padding:0 12px}
    .plugin-workspace-tabs button[aria-selected="true"]{background:var(--accent-dim);color:var(--accent-text)}
    .plugin-workspace-tabs button:hover{background:var(--control-hover);color:var(--text)}
    .plugin-sound-page{display:flex;flex:1;flex-direction:column;min-height:0;min-width:0;overflow:hidden}
    .new-interface .plugin-workspace #new-sequence-dock{flex:1;height:auto;min-height:0;margin:8px 16px;overflow:auto;border-radius:8px}
    .new-interface .plugin-workspace .plugin-modulation-rack{flex:1;overflow:visible}
    .plugin-workspace .plugin-modulation-tabs{display:none}
    .plugin-editor-resize{position:fixed;right:0;bottom:0;width:24px;height:24px;min-height:24px;padding:0;border:0;border-radius:0;background:transparent;color:var(--text-tertiary);cursor:nwse-resize;touch-action:none;z-index:20}
    .plugin-editor-resize::after{content:'';position:absolute;right:5px;bottom:5px;width:10px;height:10px;border-right:2px solid currentColor;border-bottom:2px solid currentColor}
    .plugin-editor-resize:hover,.plugin-editor-resize:focus-visible{color:var(--accent);background:var(--control-hover)}
    .native-plugin .performance-footer{padding-right:32px}
    @media(max-width:800px){.plugin-modulation-rack>.sequence-heading{align-items:flex-start;gap:8px}.plugin-modulation-tabs button{min-height:44px;min-width:44px}}
    @media(max-width:800px){
      .new-interface .plugin-workspace-tabs{grid-column:1/-1;width:100%}
      .new-interface .plugin-workspace-tabs button{flex:1;height:44px;min-height:44px}
      .new-interface .plugin-workspace #new-sequence-dock{margin:8px;min-height:320px}
      .new-interface .plugin-workspace .plugin-sound-page{flex:none;overflow:visible}
    }
  `;document.head.append(style);
  const NativeWorker=window.Worker;
  window.Worker=class extends NativeWorker{
    constructor(url,options){const name=new URL(url,location.href).pathname.split('/').at(-1),source=window.__RUSTIAS_PLUGIN_WORKERS__?.[name];
      if(!source){super(url,options);return;}
      const blob=URL.createObjectURL(new Blob([source],{type:'text/javascript'}));super(blob,{...options,type:'classic'});setTimeout(()=>URL.revokeObjectURL(blob),1000);
    }
  };
  let port,restore,parameter,context,request=0;const pending=new Map();
  const send=value=>native.postMessage(serialize(value));
  const host={initial:window.__RUSTIAS_PLUGIN__,applyingAutomation:false,onRestore:fn=>restore=fn,onParameter:fn=>parameter=fn,saveSession:data=>send({kind:'session',data})};
  const resize=document.createElement('button');resize.type='button';resize.className='plugin-editor-resize';resize.setAttribute('aria-label','Resize plugin window');resize.title='Resize window';document.body.append(resize);
  let drag,resizeFrame=0,requested;
  const requestSize=(width,height)=>{requested={kind:'editor-resize',width:Math.round(width),height:Math.round(height)};if(!resizeFrame)resizeFrame=requestAnimationFrame(()=>{resizeFrame=0;send(requested);});};
  resize.addEventListener('pointerdown',event=>{if(event.button!==0)return;resize.focus();drag={id:event.pointerId,x:event.screenX,y:event.screenY,width:innerWidth,height:innerHeight};resize.setPointerCapture(event.pointerId);event.preventDefault();});
  resize.addEventListener('pointermove',event=>{if(drag?.id===event.pointerId)requestSize(Math.max(640,drag.width+event.screenX-drag.x),Math.max(480,drag.height+event.screenY-drag.y));});
  for(const type of ['pointerup','pointercancel','lostpointercapture'])resize.addEventListener(type,()=>{drag=null;});
  window.addEventListener('blur',()=>{drag=null;});
  resize.addEventListener('keydown',event=>{const delta=event.shiftKey?64:16;if(['ArrowRight','ArrowLeft','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();requestSize(innerWidth+(event.key==='ArrowRight'?delta:event.key==='ArrowLeft'?-delta:0),innerHeight+(event.key==='ArrowDown'?delta:event.key==='ArrowUp'?-delta:0));}});
  window.__rustiasPluginReceive=data=>{
    if(data.type==='host-rate'){host.initial.sampleRate=data.sampleRate;context?.onstatechange?.();return;}
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
    let offline;context={get sampleRate(){return host.initial.sampleRate??48000;},state:'running',destination:{},audioWorklet:{addModule:async()=>{}},onstatechange:null,
      decodeAudioData:buffer=>(offline??=new OfflineAudioContext(1,1,48000)).decodeAudioData(buffer),resume:async()=>{context.state='running';send({kind:'power',enabled:true});context.onstatechange?.();},suspend:async()=>{context.state='suspended';send({kind:'power',enabled:false});context.onstatechange?.();},close:async()=>{context.state='closed';}};
    return context;
  };
  host.node=()=>{port={onmessage:null,postMessage:data=>{if(!host.applyingAutomation)send({kind:'audio',data});}};return {port,connect(){queueMicrotask(()=>port.onmessage?.({data:{type:'ready',sampleRate:host.initial.sampleRate??48000}}));},disconnect(){}};};
  host.ready=async module=>{
    if(!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID()))throw new Error('Cannot create local program identifiers.');
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
