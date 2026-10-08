const LIBRARY='rustias.patches.v1',SESSION='rustias.session.v2';
export class PatchStore{
  constructor(storage){this.storage=storage;}
  list(){try{const value=JSON.parse(this.storage.getItem(LIBRARY)??'[]');return Array.isArray(value)?value.filter(p=>typeof p.id==='string'&&typeof p.name==='string'&&p.snapshot):[];}catch{return [];}}
  save(name,snapshot,id){const patches=this.list(),patch={id:id??crypto.randomUUID(),name:name.trim().slice(0,64)||'Untitled',snapshot,updatedAt:Date.now()},index=patches.findIndex(p=>p.id===patch.id);if(index<0)patches.push(patch);else patches[index]=patch;this.storage.setItem(LIBRARY,JSON.stringify(patches));return patch;}
  session(){try{return JSON.parse(this.storage.getItem(SESSION)??'null');}catch{return null;}}
  saveSession(value){this.storage.setItem(SESSION,JSON.stringify(value));}
}
