import {compressStorage,decompressStorage} from './storage-codec.js';
const LEGACY='rustias.patches.v1',LIBRARY='rustias.patches.v2',SESSION='rustias.session.v2';

// Banks repeat kits, parameter blocks, empty sequences and original records.
// Pool identical values while keeping JSON exports as ordinary full patches.
function pack(value){
  const pool=[],known=new Map();
  function encode(value){
    let entry;
    if(Array.isArray(value)){
      entry=value.length>=64&&value.every(Number.isInteger)
        ?['n',Array.from({length:Math.ceil(value.length/16)},(_,i)=>encode(value.slice(i*16,i*16+16)))]
        :['a',value.map(encode)];
    }else if(value&&typeof value==='object')entry=['o',Object.entries(value).map(([key,v])=>[key,encode(v)])];
    else if(typeof value==='string'&&value.length>256)entry=['s',value];
    else return value;
    const key=JSON.stringify(entry);let id=known.get(key);
    if(id===undefined){id=pool.length;known.set(key,id);pool.push(entry);}
    return {$:id};
  }
  const root=encode(value),plain=JSON.stringify({format:2,pool,root});
  const compact=JSON.stringify({format:3,codec:'lzw16',data:compressStorage(plain)});return compact.length<plain.length?compact:plain;
}
function unpack(value){
  if(value?.format===3&&value.codec==='lzw16')value=JSON.parse(decompressStorage(value.data));
  if(value?.format!==2||!Array.isArray(value.pool)||value.pool.length>500000)throw new Error('Invalid patch library.');
  const memo=new Map(),visiting=new Set();
  function decode(node,depth=0){
    if(!node||typeof node!=='object')return node;
    if(depth>64||!Number.isInteger(node.$)||node.$<0||node.$>=value.pool.length||visiting.has(node.$))throw new Error('Invalid patch library reference.');
    if(memo.has(node.$))return memo.get(node.$);
    visiting.add(node.$);const [kind,data]=value.pool[node.$];let result;
    if(kind==='s'&&typeof data==='string')result=data;
    else if(kind==='a'&&Array.isArray(data))result=data.map(v=>decode(v,depth+1));
    else if(kind==='n'&&Array.isArray(data))result=data.flatMap(v=>decode(v,depth+1));
    else if(kind==='o'&&Array.isArray(data))result=Object.fromEntries(data.map(([key,v])=>[key,decode(v,depth+1)]));
    else throw new Error('Invalid patch library value.');
    visiting.delete(node.$);memo.set(node.$,result);return result;
  }
  return decode(value.root);
}
const valid=patch=>typeof patch?.id==='string'&&typeof patch.name==='string'&&patch.snapshot;
export class PatchStore{
  constructor(storage,{library=LIBRARY,legacy=LEGACY}={}){this.storage=storage;this.libraryKey=library;this.legacyKey=legacy;}
  list(){
    const current=this.storage.getItem(this.libraryKey),legacy=this.legacyKey?this.storage.getItem(this.legacyKey):null;
    if(current===this.cachedCurrent&&legacy===this.cachedLegacy&&this.cached)return this.cached;
    let patches=[],old=[];
    try{if(current){const value=unpack(JSON.parse(current));if(Array.isArray(value))patches=value.filter(valid);}}catch{}
    try{const value=JSON.parse(legacy??'[]');if(Array.isArray(value))old=value.filter(valid);}catch{}
    const merged=new Map(old.map(p=>[p.id,p]));
    for(const p of patches){const previous=merged.get(p.id);if(!previous||(p.updatedAt??0)>=(previous.updatedAt??0))merged.set(p.id,p);}
    this.cachedCurrent=current;this.cachedLegacy=legacy;this.cached=[...merged.values()];return this.cached;
  }
  write(patches){
    // One setItem is atomic, including on QuotaExceededError. Keep the old
    // key readable for patches created by an already-open older browser tab.
    this.storage.setItem(this.libraryKey,pack(patches));this.cached=null;
  }
  save(name,snapshot,id){
    const patches=[...this.list()],patch={id:id??crypto.randomUUID(),name:name.trim().slice(0,64)||'Untitled',snapshot,updatedAt:Date.now()},index=patches.findIndex(p=>p.id===patch.id);
    if(index<0)patches.push(patch);else patches[index]=patch;this.write(patches);return patch;
  }
  import(patches){
    if(!Array.isArray(patches)||!patches.every(valid))throw new Error('Invalid imported patches.');
    const existing=[...this.list()],ids=new Set(existing.map(p=>p.id));let added=0;
    for(const patch of patches)if(!ids.has(patch.id)){existing.push(patch);ids.add(patch.id);added++;}
    if(added)this.write(existing);return {added,duplicates:patches.length-added};
  }
  session(){try{return JSON.parse(this.storage.getItem(SESSION)??'null');}catch{return null;}}
  saveSession(value){this.storage.setItem(SESSION,JSON.stringify(value));}
}

export class TimbreStore extends PatchStore{
  constructor(storage){super(storage,{library:"rustias.timbres.v1",legacy:null});}
}
