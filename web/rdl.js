import {MAX_TIMBRES} from './limits.js';
import {emptySequence} from './sequence.js';
export const MAX_RDL_BYTES=8*1024*1024;

export function parseRdl(api,buffer){
  const bytes=new Uint8Array(buffer);
  if(!bytes.length||bytes.length>MAX_RDL_BYTES)throw new Error('Choose an RDL file up to 8 MiB.');
  const pointer=api.rustias_rdl_buffer(bytes.length);
  if(!pointer)throw new Error('Could not allocate the RDL import buffer.');
  new Uint8Array(api.memory.buffer,pointer,bytes.length).set(bytes);
  const length=api.rustias_rdl_import(bytes.length);
  const result=JSON.parse(new TextDecoder().decode(new Uint8Array(api.memory.buffer,api.rustias_rdl_result_buffer(),length)));
  if(!result.ok)throw new Error(`Could not import this RDL file: ${result.error}.`);return result.library;
}
export function readRdl(module,buffer){
  return new Promise((resolve,reject)=>{
    const worker=new Worker(new URL('./rdl-worker.js',import.meta.url),{type:'module'});
    worker.onmessage=({data})=>{worker.terminate();if(data.error)reject(new Error(data.error));else resolve(data.library);};
    worker.onerror=event=>{worker.terminate();reject(new Error(event.message||'Could not read this RDL file.'));};
    worker.postMessage({module,buffer},[buffer]);
  });
}
export function rdlPatches(library,file,digest){
  return library.programs.map(program=>({
    id:`rdl:${digest}:${program.slot}`,name:program.name,updatedAt:Date.now(),
    snapshot:{version:2,engine:program.engine,sequencer:emptySequence(),samples:null,
      rdl:{...program.rdl,file,slot:program.slot,digest}},
  }));
}
export function validateRdlSource(value){
  if(value==null)return null;
  const validBytes=bytes=>typeof bytes==='string'&&bytes.length<=MAX_RDL_BYTES*2&&/^[A-Za-z0-9+/]*={0,2}$/.test(bytes);
  if(value.version!==1||!validBytes(value.program)||value.global!=null&&!validBytes(value.global)||value.drum_kit!=null&&!validBytes(value.drum_kit)||
    !Number.isInteger(value.slot)||value.slot<0||value.slot>255||typeof value.file!=='string'||value.file.length>256||typeof value.digest!=='string'||!/^[a-f0-9]{64}$/.test(value.digest)||
    !Array.isArray(value.notices)||value.notices.length>1000||value.notices.some(n=>typeof n!=='string'||n.length>1000)||
    !Array.isArray(value.unavailable)||value.unavailable.length>20||value.unavailable.some(s=>!Number.isInteger(s.timbre)||s.timbre<0||s.timbre>=MAX_TIMBRES||s.drum!=null&&(!Number.isInteger(s.drum)||s.drum<0||s.drum>15)||typeof s.label!=='string'||s.label.length>128||!Number.isInteger(s.selection)||s.selection<0||s.selection>255)){
    throw new Error('Invalid RDL source information.');
  }
  return structuredClone(value);
}
export function rdlMasks(source){
  let timbres=0,drums=0;
  for(const entry of source?.unavailable??[])if(entry.drum==null)timbres|=1<<entry.timbre;else drums|=1<<entry.drum;
  return {timbres,drums};
}
