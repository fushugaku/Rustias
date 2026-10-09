import assert from 'node:assert/strict';
import {defaultCircuit,connect,validateCircuit,placeDrive} from '../web/circuit.js';

export function verifyCircuitSwitch({api,setCircuit,graphNote,render,rms,control}){
  let c=defaultCircuit();c.enabled=true;
  c.nodes.push({id:16,kind:'switch',x:24,y:1000,params:{route:0}},{id:17,kind:'sum',x:374,y:1000,params:{a:127,b:127,c:0}});
  c.wires=[{from:0,to:16,port:'in'},{from:16,output:'a',to:4,port:'in'},{from:4,to:17,port:'a'},{from:16,output:'b',to:17,port:'b'},{from:17,to:7,port:'in'},{from:7,to:15,port:'in'}];
  const a=graphNote(c),directA=structuredClone(c);directA.wires=directA.wires.filter(w=>w.from!==16);directA.wires.push({from:0,to:4,port:'in'});
  const referenceA=graphNote(directA);assert.ok(rms(a.map((v,i)=>v-referenceA[i]))<.000001,'Switch A sends the complete signal through Filter 1');
  c.nodes.find(n=>n.id===16).params.route=1;
  const b=graphNote(c),directB=structuredClone(c);directB.wires=directB.wires.filter(w=>w.from!==16);directB.wires.push({from:0,to:17,port:'b'});
  const referenceB=graphNote(directB);assert.ok(rms(b.map((v,i)=>v-referenceB[i]))<.000001,'Switch B sends the complete signal directly to Amplifier');
  assert.ok(rms(a.map((v,i)=>v-b[i]))>.00001,'Filter and direct routes produce different sound');
  const cvB=connect(c,13,16,'select');cvB.nodes.find(n=>n.id===16).params.route=0;
  assert.deepEqual(graphNote(cvB),b,'A connected high CV chooses B regardless of the manual route');
  const cvA=connect(c,11,16,'select');cvA.nodes.find(n=>n.id===11).kind='lfo';cvA.nodes.find(n=>n.id===11).params={rate:1,shape:2,depth:0};
  assert.deepEqual(graphNote(cvA),a,'A connected zero CV chooses A');
  const onlyA=structuredClone(c);onlyA.wires=onlyA.wires.filter(w=>w.output!=='b');assert.ok(rms(graphNote(onlyA))<.000001,'The inactive output does not leak to another branch');
  const badOutput=structuredClone(c);badOutput.wires.find(w=>w.from===16).output='out';assert.throws(()=>validateCircuit(badOutput));setCircuit(badOutput,0,0);
  const badSource=structuredClone(c);badSource.wires.find(w=>w.from===0).output='b';assert.throws(()=>validateCircuit(badSource));setCircuit(badSource,0,0);
  assert.throws(()=>connect(c,16,4,'cutoff','a'),'Audio switch outputs cannot connect to CV inputs');
  const loop=structuredClone(c);loop.wires[0]={from:17,to:16,port:'in'};assert.throws(()=>validateCircuit(loop));setCircuit(loop,0,0);
  const badRoute=structuredClone(c);badRoute.nodes.find(n=>n.id===16).params.route=.5;assert.throws(()=>validateCircuit(badRoute));setCircuit(badRoute,0,0);

  // Two identical paths must remain sample-identical through live crossfading.
  const live=structuredClone(c);live.wires=live.wires.filter(w=>w.to!==4&&w.from!==4);live.wires.push({from:16,output:'a',to:17,port:'a'});live.nodes.find(n=>n.id===16).params.route=0;
  function held(change){api.rustias_init();control(3,0);control(4,127);control(5,127);setCircuit(live);api.rustias_note(0,69,100);render(8000);if(change){const next=structuredClone(live);next.nodes.find(n=>n.id===16).params.route=1;setCircuit(next);}return render(4000);}
  const continuous=held(false),switched=held(true);assert.ok(rms(switched.map((v,i)=>v-continuous[i]))<.000001,'Switching a held note does not reset or duplicate the source');
  const stored=JSON.parse(JSON.stringify(c));assert.deepEqual(validateCircuit(stored),c,'Output A/B identities survive saved patch JSON');
  const old=defaultCircuit([30]);assert.deepEqual(validateCircuit(old),old,'Existing cables without an output name retain their routing');
  const before=defaultCircuit();before.enabled=true;
  const preAmp=placeDrive(before,1);assert.deepEqual(preAmp.nodes,before.nodes,'Position changes do not move any module');assert.deepEqual(preAmp.panels,before.panels);
  assert.ok(preAmp.wires.some(w=>w.from===6&&w.to===7));assert.ok(preAmp.wires.some(w=>w.from===3&&w.to===4));assert.ok(preAmp.wires.some(w=>w.from===5&&w.to===6));
  const preFilter=placeDrive(preAmp,0);assert.ok(preFilter.wires.some(w=>w.from===3&&w.to===6));assert.ok(preFilter.wires.some(w=>w.from===6&&w.to===4));
  setCircuit(preAmp);setCircuit(preFilter);
  const branched=connect(before,6,15,'in');assert.equal(placeDrive(branched,1),null,'Position does not discard a custom fanout');assert.deepEqual(before,placeDrive(before,0));
  return 'Browser Switch: separate A/B audio routes, CV selection, live state preservation, port validation, JSON compatibility and stationary Drive Position rewiring';
}
