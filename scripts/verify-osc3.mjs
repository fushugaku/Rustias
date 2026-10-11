import assert from 'node:assert/strict';
import {OSC3_BASE,PARAMETER_COUNT,MAX_TIMBRES} from '../web/limits.js';
import {defaultCircuit,connect,validateCircuit,removeModule,availableModules} from '../web/circuit.js';
import {normalizeProgram,captureTimbre,applyTimbre} from '../web/programs.js';
import {MacroBank,emptyMacros} from '../web/macros.js';
import {newModulationLane,emptyModulation,ModulationAutomation} from '../web/modulation.js';

export function verifyOsc3({api,parameters,control,render,rms,save,load,setCircuit}){
  const b=OSC3_BASE;
  assert.equal(api.rustias_parameter_count(),PARAMETER_COUNT);
  assert.deepEqual(parameters.slice(b,b+6).map(p=>[p.min,p.max]),[0,10,11,12,15,16].map(i=>[parameters[i].min,parameters[i].max]));
  assert.deepEqual(parameters.slice(b+6).map(p=>[p.min,p.max,p.default]),[[0,3,0],[0,2,0]]);
  const difference=(a,c)=>rms(a.map((v,i)=>v-c[i]));
  function setup({t=0,wave=0,mode=0,ctrl1=0,ctrl2=0,semi=64,fine=64,level=127,first=0,second=0,transpose=64,secondaryWave=0,secondaryMode=0,secondarySemi=64,interaction=0,reference=0,firstWave=0}={}){
    api.rustias_init();control(71,1,t);control(17,first,t);control(18,0,t);control(19,level,t);control(9,127,t);control(3,0,t);control(4,127,t);control(5,127,t);control(53,transpose,t);
    for(const [id,v]of [[b,wave],[b+1,mode],[b+2,ctrl1],[b+3,ctrl2],[b+4,semi],[b+5,fine],[b+6,interaction],[b+7,reference],[0,firstWave],[13,secondaryWave],[14,secondaryMode],[15,secondarySemi],[18,second]])control(id,v,t);
  }
  const play=(t=0)=>{assert.equal(api.rustias_note(t,69,100),1);return render(24000);};
  const sound=options=>{setup(options);return play(options?.t??0);};
  function frequency(audio){let n=0;for(let i=4001;i<audio.length;i++)if(audio[i]>=0&&audio[i-1]<0)n++;return n*48000/(audio.length-4000);}
  for(let mode=0;mode<4;mode++)for(let wave=0;wave<(mode===0?6:4);wave++)assert.ok(rms(sound({wave,mode,ctrl1:60,ctrl2:60}))>.0001,`OSC 3 ${wave}/${mode} renders through the real fixed graph`);
  assert.ok(Math.abs(frequency(sound({wave:3}))-440)<4,'OSC 3 follows A4');
  assert.ok(Math.abs(frequency(sound({wave:3,semi:76}))-880)<5,'Semitone tuning matches OSC 2');
  assert.ok(Math.abs(frequency(sound({wave:3,fine:127}))-466.16)<5,'Fine 127 follows the native OSC 2 tuning table');
  assert.ok(Math.abs(frequency(sound({wave:3,transpose:76}))-880)<5,'Common transpose reaches OSC 3');
  assert.ok(difference(sound({ctrl1:0}),sound({ctrl1:100}))>.001,'CTRL 1 changes the third generator');
  assert.ok(difference(sound({wave:3,mode:3,ctrl1:80,ctrl2:0}),sound({wave:3,mode:3,ctrl1:80,ctrl2:100}))>.001,'CTRL 2 changes its VPM ratio');
  const first=sound({level:0,first:127});for(const options of [{wave:3,mode:3,ctrl1:100},{wave:5,ctrl1:100}])assert.ok(difference(first,sound({...options,level:0,first:127}))<1e-8,'Unmixed OSC 3 leaves OSC 1 samples unchanged');
  for(let t=0;t<MAX_TIMBRES;t++)assert.ok(rms(sound({t,wave:3}))>.001,`OSC 3 belongs to timbre ${t+1}`);

  const coupled={wave:3,semi:59,firstWave:3,secondaryWave:1,secondaryMode:3,secondarySemi:71};
  const uncoupled=sound(coupled),byMode=[];
  for(let interaction=1;interaction<=3;interaction++){
    const refs=[];
    for(let reference=0;reference<3;reference++){
      const audio=sound({...coupled,interaction,reference});refs.push(audio);
      assert.ok(rms(audio)>.0001,`OSC 3 interaction ${interaction}/${reference} is audible`);
      assert.ok(difference(uncoupled,audio)>.001,`Interaction ${interaction}/${reference} changes real OSC 3 audio`);
    }
    assert.ok(difference(refs[0],refs[1])>.001,'OSC 1 and OSC 2 are independent references');
    if(interaction&1)assert.ok(difference(refs[1],refs[2])>.001,'OSC 2 before Ring and the current OSC 1→2 result are separate audio taps');
    else assert.ok(difference(refs[1],refs[2])<1e-8,'The pair result uses its OSC 2 carrier phase for hard sync');
    byMode.push(refs[0]);
  }
  assert.ok(difference(byMode[0],byMode[1])>.001);assert.ok(difference(byMode[1],byMode[2])>.001,'Ring+Sync applies both interactions');
  assert.ok(Math.abs(frequency(uncoupled)-329.63)<5);assert.ok(Math.abs(frequency(byMode[1])-440)<5,'Hard sync resets a slower OSC 3 at the OSC 1 cycle boundary');
  const nativePair=sound({...coupled,level:0,first:80,second:80});
  for(let interaction=1;interaction<4;interaction++)for(let reference=0;reference<3;reference++)assert.ok(difference(nativePair,sound({...coupled,level:0,first:80,second:80,interaction,reference}))<1e-8,'OSC 3 taps leave the original OSC 1/2 samples unchanged');
  for(let mode=0;mode<4;mode++)for(let wave=0;wave<(mode===0?6:4);wave++)assert.ok(rms(sound({...coupled,wave,mode,ctrl1:60,ctrl2:60,interaction:3}))>.00001,`Ring+Sync also processes the shared ${wave}/${mode} kernel`);
  setup({...coupled,interaction:1});play();control(b+7,2);assert.equal(api.rustias_voices(),1);assert.ok(rms(render())>.001,'The reference can change on a held voice');

  for(const [destination,options]of [[43,{wave:3}],[44,{wave:0}],[45,{wave:3,mode:3,ctrl1:80}],[46,{wave:0}],[47,{wave:0,ctrl1:80,ctrl2:60}],[48,{wave:3}],[49,{wave:3}],[50,coupled],[51,{...coupled,interaction:1}]]){
    const dry=sound(options);setup(options);control(90,5);control(91,destination);control(92,75);assert.ok(difference(dry,play())>.0001,`Virtual Patch destination ${destination} modulates OSC 3`);
  }
  const dry=sound({wave:3,level:100});setup({wave:3,level:100});control(90,16);control(91,5);control(92,110);control(165,86);assert.ok(difference(dry,play())>.001,'LFO 3 modulates OSC 3 Level through the existing third mixer channel');
  setup({wave:3});play();control(b+4,76);assert.equal(api.rustias_voices(),1);assert.ok(Math.abs(frequency(render(24000))-880)<5,'Live tuning retains the held voice');control(9,0);control(1,15);assert.ok(rms(render(18000).slice(4000))<.01,'The native filter processes OSC 3');
  setup({wave:0,mode:3});control(b,4);assert.equal(api.rustias_value(0,b+1),0);assert.equal(api.rustias_control(0,b+1,3),0,'Noise/Formant keep Waveform mode');

  const graph=defaultCircuit();graph.enabled=true;const node=graph.nodes.find(n=>n.kind==='osc3');assert.equal(node.id,2,'OSC 3 retains the old Noise node and cable IDs');
  function graphSound(c,options={wave:0,ctrl1:60}){setup(options);setCircuit(c);return play();}
  const plain=graphSound(graph),cv=connect(graph,14,node.id,'ctrl1');assert.ok(difference(plain,graphSound(cv))>.001,'OSC 3 CTRL 1 accepts CV');
  const pitchCv=connect(graph,14,node.id,'pitch');assert.ok(difference(plain,graphSound(pitchCv))>.001,'Its pitch accepts independent CV');
  const modA=connect(graph,0,node.id,'mod'),modB=connect(graph,1,node.id,'mod');assert.ok(difference(graphSound(modA,{wave:0,mode:1,ctrl1:90,secondaryWave:1}),graphSound(modB,{wave:0,mode:1,ctrl1:90,secondaryWave:1}))>.001,'Cross mode uses the connected audio modulator');
  const graphDry=graphSound(graph,coupled);for(let interaction=1;interaction<4;interaction++)for(let reference=0;reference<3;reference++)assert.ok(difference(graphDry,graphSound(graph,{...coupled,interaction,reference}))>.001,'Build routing retains all Ring/Sync references');
  assert.ok(availableModules(removeModule(graph,node.id)).includes('osc3'));const oldGraph=structuredClone(graph);oldGraph.nodes.find(n=>n.id===2).kind='noise';assert.equal(validateCircuit(oldGraph).nodes.find(n=>n.id===2).kind,'osc3','Old Noise wiring migrates in place');
  setup({wave:3});setCircuit(graph);play();const off=structuredClone(graph);off.enabled=false;setCircuit(off);assert.equal(api.rustias_voices(),1);assert.ok(rms(render())>.001,'Routing Off returns to the fixed native filters with OSC 3');setCircuit(graph);assert.equal(api.rustias_voices(),1);assert.ok(rms(render())>.001,'Returning to Build preserves the third oscillator state');
  setup({wave:3,level:10});for(let note=0;note<128;note++)api.rustias_note(0,note,100);assert.equal(api.rustias_voices(),128);render(256);api.rustias_stop();

  // The synth source in a kit has its own complete third oscillator program.
  api.rustias_init();control(140,1);for(let d=0;d<2;d++)for(const [id,v]of [[17,0],[18,0],[19,127],[9,127],[3,0],[4,127],[5,127],[b,d?0:3],[b+1,0],[b+4,d?76:64]])assert.equal(api.rustias_drum_control(d,id,v),1);
  api.rustias_drum_pad(0,100);const drumA=render(12000);api.rustias_stop();api.rustias_drum_pad(1,100);assert.ok(difference(drumA,render(12000))>.001,'Native drums retain independent OSC 3 settings');

  setup({t:7,wave:3,mode:3,ctrl1:75,ctrl2:90,semi:59,fine:77,interaction:3,reference:2});const engine=save();assert.equal(load(engine),1);assert.deepEqual(save(),engine);const legacy=structuredClone(engine);for(const v of [...legacy.timbres,...legacy.drums])v.length=b;assert.equal(load(legacy),1);const migrated=save();assert.deepEqual(migrated.timbres.map(v=>v.slice(0,b)),legacy.timbres);assert.deepEqual(migrated.timbres[7].slice(b),[4,0,0,0,64,64,0,0],'Older programs gain an independent Noise source without changing existing values');
  const beforeInteraction=structuredClone(engine);for(const v of [...beforeInteraction.timbres,...beforeInteraction.drums])v.length=b+6;assert.equal(load(beforeInteraction),1);assert.deepEqual(save().timbres[7].slice(b+6),[0,0],'Six-control OSC 3 programs migrate with Ring/Sync off');assert.deepEqual(normalizeProgram(beforeInteraction,parameters).engine.timbres[7].slice(b+6),[0,0]);
  const program=normalizeProgram(engine,parameters),moved=applyTimbre(program,6,captureTimbre(program,7,parameters),parameters);assert.deepEqual(moved.engine.timbres[6].slice(b),engine.timbres[7].slice(b));
  const invalid=structuredClone(engine);invalid.timbres[7][b]=4;assert.equal(load(invalid),0);assert.throws(()=>normalizeProgram(invalid,parameters),'Invalid mode is rejected before overwriting a program');
  setup({wave:0,ctrl1:64});const target={kind:'synth',timbre:0,parameter:b+2},bank=new MacroBank({resolve:()=>({label:'OSC 3 CTRL 1',min:0,max:127,read:()=>api.rustias_value(0,b+2)}),write:changes=>changes.forEach(c=>control(c.target.parameter,c.value,c.target.timbre))});bank.assign(0,target);bank.setAmount(0,target,-50);bank.setValue(0,100);assert.equal(api.rustias_value(0,b+2),1);bank.setValue(0,0);assert.equal(api.rustias_value(0,b+2),64,'Macros restore the third oscillator base');
  const config=emptyModulation(),lane=newModulationLane('osc3-motion');lane.target=target;lane.values[0]=12;config.tracks[0]=[lane];const automation=new ModulationAutomation(changes=>changes.forEach(c=>control(c.target.parameter,c.value,c.target.timbre)));automation.setState({config,macros:emptyMacros(),targets:[{target,min:0,max:127,base:64,available:true}]});automation.clock.play();automation.beforeRender();assert.equal(api.rustias_value(0,b+2),76);automation.stop();assert.equal(api.rustias_value(0,b+2),64,'Mod sequencing is temporary for OSC 3 too');
  api.rustias_stop();return 'Browser OSC 3 replaces Noise using shared OSC 1 kernels: all 18 wave/mode combinations, OSC 2 Semi/Fine, Ring/Sync/Ring+Sync with OSC 1, OSC 2 and current pair-result taps, untouched native pair, all timbres/drums, nine Virtual Patch targets plus level, audio/CV wiring, live routing, macros/mod lanes, 128 voices and program/timbre migration';
}
