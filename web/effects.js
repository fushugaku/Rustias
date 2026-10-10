// Definitions are exported from the shared Rust effect catalog at build time.
let catalog;
export function setEffectCatalog(value){if(value?.version!==1||value.insert?.length!==31||value.master?.length!==31)throw new Error('Invalid effect catalog.');catalog=value;}
export const effectDefinitions=master=>catalog?.[master?'master':'insert']??[];
export function defaultEffect(kind=0,master=false){const def=effectDefinitions(master)[kind];return {kind,master,enabled:kind!==0,parameters:def?[...def.defaults]:Array(20).fill(0)};}
export const emptyEffects=()=>({version:1,slots:Array.from({length:9},(_,i)=>defaultEffect(0,i===8))});
export function validateEffect(raw,master){
  if(!raw||!Number.isInteger(raw.kind)||raw.kind<0||raw.kind>30||typeof raw.enabled!=='boolean'||raw.master!==master||raw.parameters?.length!==20||raw.parameters.some(v=>!Number.isInteger(v)||v<0||v>255))throw new Error('Invalid effect settings.');
  const def=effectDefinitions(master)[raw.kind];
  if(raw.kind!==0&&def&&def.properties.some((p,i)=>raw.parameters[i]-p.zero<p.min||raw.parameters[i]-p.zero>p.max))throw new Error('Effect parameter outside its range.');
  return structuredClone(raw);
}
export function validateEffects(raw){if(raw==null)return emptyEffects();if(raw.version!==1||raw.slots?.length!==9)throw new Error('Invalid effect rack.');return {version:1,slots:raw.slots.map((p,i)=>validateEffect(p,i===8))};}
// Lossless stored-program layout, also covered against the Rust RDL reader.
export function effectsFromRdl(base64){
  if(!base64)return emptyEffects();const bytes=Uint8Array.from(atob(base64),c=>c.charCodeAt(0));if(bytes.length<1060)return emptyEffects();
  return {version:1,slots:Array.from({length:9},(_,slot)=>{const master=slot===8,offset=master?1038:168+228*Math.floor(slot/2)+24*(slot%2),kind=bytes[offset]&127;if(kind>30)return defaultEffect(0,master);
    const p={kind,master,enabled:!!(bytes[offset]&128),parameters:[...bytes.slice(offset+(master?2:4),offset+(master?22:24))]},def=effectDefinitions(master)[kind];
    def?.properties.forEach((prop,i)=>p.parameters[i]=Math.max(prop.min,Math.min(prop.max,p.parameters[i]-prop.zero))+prop.zero);return p;})};
}
