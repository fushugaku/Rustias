// The modular schema is browser-only. Native controls still feed the shared Rust kernels.
export const MODULES={
  osc1:{name:'OSC 1',panel:'osc1',out:'audio'},osc2:{name:'OSC 2',panel:'osc2',out:'audio'},noise:{name:'NOISE',out:'audio'},
  mixer:{name:'MIXER / AMP',panel:'mixer',out:'audio',inputs:{a:'audio',b:'audio',c:'audio'}},
  filter1:{name:'FILTER 1',panel:'filter1',out:'audio',inputs:{in:'audio',cutoff:'cv'}},filter2:{name:'FILTER 2',panel:'filter2',out:'audio',inputs:{in:'audio',cutoff:'cv'}},
  drive:{name:'DRIVE / WS',panel:'drive',out:'audio',inputs:{in:'audio'}},amp:{name:'AMPLIFIER',panel:'amp',out:'audio',inputs:{in:'audio',gain:'cv'}},
  eg1:{name:'EG 1',panel:'eg1',out:'cv'},eg2:{name:'EG 2',panel:'eg2',out:'cv'},eg3:{name:'EG 3',panel:'eg3',out:'cv'},lfo1:{name:'LFO 1',panel:'lfo1',out:'cv'},lfo2:{name:'LFO 2',panel:'lfo2',out:'cv'},
  gate:{name:'KEY GATE',out:'cv'},velocity:{name:'VELOCITY',out:'cv'},output:{name:'OUTPUT',inputs:{in:'audio'}},
  oscillator:{name:'OSCILLATOR',out:'audio',inputs:{pitch:'cv'},controls:{wave:['Wave',0,3,0,['Saw','Pulse','Triangle','Sine']],semitone:['Semi',-48,48,0],level:['Level',0,127,64]}},
  filter:{name:'FILTER',out:'audio',inputs:{in:'audio',cutoff:'cv'},controls:{cutoff:['Cutoff',0,127,96],resonance:['Resonance',0,127,0],morph:['Morph',0,127,0]}},
  shaper:{name:'DRIVE / WS',out:'audio',inputs:{in:'audio'},controls:{mode:['Mode',0,2,1,['Off','Drive','WS']],type:['WS Type',0,10,1,['Decimator','Hard Clip','Oct Saw','Multi Triangle','Multi Sine','Sub Saw','Sub Square','Sub Triangle','Sub Sine','Pickup','Level Boost']],depth:['Depth',0,127,32]}},
  vca:{name:'VCA',out:'audio',inputs:{in:'audio',gain:'cv'},controls:{gain:['Gain dB',-48,24,0]}},
  sum:{name:'MIXER',out:'audio',inputs:{a:'audio',b:'audio',c:'audio'},controls:{a:['A',0,127,64],b:['B',0,127,64],c:['C',0,127,64]}},
  lfo:{name:'LFO',out:'cv',inputs:{rate:'cv'},controls:{rate:['Rate Hz',.01,40,1],shape:['Wave',0,3,0,['Sine','Triangle','Square','Saw']],depth:['Depth %',0,100,100]}},
  envelope:{name:'ENVELOPE',out:'cv',inputs:{gate:'cv'},controls:{attack:['Attack',0,127,0],decay:['Decay',0,127,48],sustain:['Sustain',0,127,100],release:['Release',0,127,32]}},
};
export const EXTRA_MODULES=['oscillator','filter','shaper','vca','sum','lfo','envelope'];
const BUILTINS=['osc1','osc2','noise','mixer','filter1','filter2','drive','amp','eg1','eg2','eg3','lfo1','lfo2','gate','velocity','output'];
export function defaultCircuit(v=[]){
  const positions=[[24,24],[24,400],[24,780],[350,24],[700,24],[1050,24],[1400,24],[1750,24],[350,580],[700,580],[1050,580],[1400,580],[1750,580],[350,1120],[700,1120],[2100,24]];
  const nodes=BUILTINS.map((kind,id)=>({id,kind,x:positions[id][0]+(positions[id][0]>=350?24:0),y:positions[id][1],params:{}}));
  const wires=[{from:0,to:3,port:'a'},{from:1,to:3,port:'b'},{from:2,to:3,port:'c'}];
  const chain=v[30]===0?[3,6,4,5,7,15]:[3,4,5,6,7,15];
  for(let i=1;i<chain.length;i++)wires.push({from:chain[i-1],to:chain[i],port:'in'});
  return {enabled:false,nodes,wires,panels:Object.fromEntries(['patch1','patch2','patch3','patch4','patch5','patch6','voice','midi','scale','drums'].map((key,i)=>[key,{x:24+i%6*350,y:i<6?1310:1760}]))};
}
export const emptyCircuits=values=>({version:1,tracks:Array.from({length:4},(_,i)=>defaultCircuit(values?.[i]))});
export function validateCircuit(raw){
  const c=structuredClone(raw);
  if(!c||typeof c.enabled!=='boolean'||!Array.isArray(c.nodes)||!c.nodes.length||c.nodes.length>64||!Array.isArray(c.wires)||c.wires.length>256)throw new Error('Invalid modular patch.');
  const ids=new Map();let outputs=0;
  for(const n of c.nodes){const def=Object.hasOwn(MODULES,n.kind)?MODULES[n.kind]:null;if(!def||!Number.isInteger(n.id)||n.id<0||n.id>=64||ids.has(n.id))throw new Error('Invalid module.');ids.set(n.id,n);if(n.kind==='output')outputs++;
    for(const axis of ['x','y'])if(!Number.isFinite(n[axis])||n[axis]<0||n[axis]>10000)throw new Error('Invalid module position.');
    n.params??={};for(const [key,control]of Object.entries(def.controls??{})){const value=n.params[key]??control[3];if(!Number.isFinite(value)||value<control[1]||value>control[2]||control[4]&&!Number.isInteger(value))throw new Error(`Invalid ${control[0]}.`);n.params[key]=value;}
  }
  if(outputs!==1)throw new Error('Keep one Output module.');const inputs=new Set();
  for(const w of c.wires){const a=ids.get(w.from),b=ids.get(w.to),type=b&&MODULES[b.kind].inputs?.[w.port],key=`${w.to}:${w.port}`;
    if(!a||!b||!type||MODULES[a.kind].out!==type||inputs.has(key))throw new Error('Connect audio to audio and CV to CV; use a Mixer to combine signals.');inputs.add(key);}
  const done=new Set();while(done.size<c.nodes.length){const before=done.size;for(const n of c.nodes)if(!done.has(n.id)&&c.wires.filter(w=>w.to===n.id).every(w=>done.has(w.from)))done.add(n.id);if(done.size===before)throw new Error('This cable would create a feedback loop.');}
  c.panels??={};for(const [key,pos]of Object.entries(c.panels))if(!/^[a-z][a-z0-9]*$/.test(key)||!['x','y'].every(a=>Number.isFinite(pos[a])&&pos[a]>=0&&pos[a]<=10000))throw new Error('Invalid panel position.');return c;
}
export function validateCircuits(raw,values){if(raw==null)return emptyCircuits(values);if(raw.version!==1||raw.tracks?.length!==4)throw new Error('Invalid modular patch.');return {version:1,tracks:raw.tracks.map(validateCircuit)};}
export function connect(circuit,from,to,port){const copy=structuredClone(circuit);copy.wires=copy.wires.filter(w=>w.to!==to||w.port!==port);copy.wires.push({from,to,port});copy.enabled=true;return validateCircuit(copy);}
export function audioCircuit(c){return {enabled:c.enabled,nodes:c.nodes.map(({id,kind,params})=>({id,kind,params})),wires:c.wires};}
