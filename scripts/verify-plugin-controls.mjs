import assert from 'node:assert/strict';
import {createPluginAutomationSync} from '../web/plugin-automation.js';
import {emptyMacros} from '../web/macros.js';
import {emptyModulation,newModulationLane} from '../web/modulation.js';
import {emptyCircuits,audioCircuit} from '../web/circuit.js';
import {emptyEffects} from '../web/effects.js';

const sent=[],sync=createPluginAutomationSync(message=>sent.push(JSON.parse(JSON.stringify(message))));
const state={config:emptyModulation(),macros:emptyMacros(),targets:[],effects:emptyEffects(),circuits:emptyCircuits().tracks.map(audioCircuit),engine:{timbres:Array.from({length:8},()=>Array(179).fill(0)),drums:Array.from({length:16},()=>Array(179).fill(0))}};
assert.ok(sync.send(state));
assert.ok(JSON.stringify(sent.at(-1)).length<1000,'An unused native modulation rack never carries the engine or default circuit/FX programs');
assert.ok(!('engine' in sent.at(-1).state));
assert.equal(sync.send(state),false,'UI refreshes do not rebuild native modulation');

const target={kind:'synth',timbre:0,parameter:1};
state.targets=[{target,min:0,max:127,base:80,available:true}];
state.macros.knobs[0].bindings=[{target,amount:50}];state.macros.bases=[{target,value:80,label:'Cutoff'}];
assert.ok(sync.send(state));const beforeDrag=sent.length;
for(let value=0;value<128;value++){
  state.targets[0].base=value;state.macros.bases[0].value=value;state.engine.timbres[0][1]=value;
  assert.equal(sync.send(state),false,'Scalar control messages own live synthesis/base edits');
}
assert.equal(sent.length,beforeDrag,'A complete knob drag sends zero modulation configurations');
state.targets[0].available=false;assert.ok(sync.send(state),'Availability edits reach Rust');
state.targets[0].available=true;state.macros.knobs[0].bindings[0].amount=-30;assert.ok(sync.send(state),'Bipolar binding edits reach Rust');
const lane=newModulationLane('knob-regression');lane.target=target;state.config.tracks[0].push(lane);assert.ok(sync.send(state));
lane.resolution='1/8';assert.ok(sync.send(state),'Clock edits reach Rust');
lane.values[0]=20;assert.ok(sync.send(state),'Modulation step edits reach Rust');

for(const scalar of [{kind:'global',parameter:121},{kind:'drum',instrument:2,parameter:1},{kind:'sample',timbre:2,source:'user:clip',parameter:1}]){
  state.targets=[{target:scalar,min:0,max:127,base:60,available:true}];assert.ok(sync.send(state));
  state.targets[0].base=70;assert.equal(sync.send(state),false,'Other native scalar sources reuse the compiled rack');
}
state.targets=[{target:{kind:'volume'},min:0,max:100,base:30}];assert.ok(sync.send(state));
state.targets[0].base=50;assert.ok(sync.send(state),'Gain macro bases retain their existing update path');

state.targets=[{target:{kind:'effect',slot:16,effectKind:14,parameter:0},min:0,max:100,base:30}];
state.effects.slots[16]={kind:14,enabled:true,parameters:[30,60]};assert.ok(sync.send(state));
assert.equal(sent.at(-1).state.effects.slots[0],null);assert.deepEqual(sent.at(-1).state.effects.slots[16],state.effects.slots[16]);
state.effects.slots[0]={kind:1,enabled:true,parameters:[20]};assert.equal(sync.send(state),false,'An unrelated FX stage does not rebuild the rack');
state.effects.slots[16].parameters[0]=60;assert.ok(sync.send(state),'A modulated FX program remains current');
state.effects.chains[2]=[{id:'mod-fx',kind:14,enabled:true,parameters:[20]},{id:'other-fx',kind:14,enabled:true,parameters:[40]}];
state.targets.push({target:{kind:'chain-effect',timbre:2,id:'mod-fx',effectKind:14,parameter:0},min:0,max:100,base:20});assert.ok(sync.send(state));
assert.deepEqual(sent.at(-1).state.effects.chains[2],[state.effects.chains[2][0]],'Native modulation retains the referenced chain IDs');

state.circuits[3].nodes.push({id:30,kind:'gain',params:{level:100}});
state.targets.push({target:{kind:'module',timbre:3,node:30,moduleKind:'gain',control:'level'},min:0,max:100,base:100});assert.ok(sync.send(state));
assert.deepEqual(sent.at(-1).state.circuits[3],state.circuits[3],'Module modulation retains its complete owning graph');
assert.equal(sent.at(-1).state.circuits[0],null);
state.circuits[3].nodes.at(-1).params.level=80;assert.ok(sync.send(state));
sync.reset();assert.ok(sync.send(state),'A whole-program load always resynchronizes its modulation rack');
