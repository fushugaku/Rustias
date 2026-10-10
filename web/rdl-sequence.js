import {emptySequence} from './sequence.js';
import {emptyModulation,newModulationLane} from './modulation.js';

// SYS 2.00 edit-buffer offsets (1790 bytes), not the librarian record padding.
// Korg MIDI Implementation tables 6, 9 and 10 describe the packed fields.
// The published 2005 destination list predates the 42-entry RADIAS knob list.
const NOTE_RATES=['1/32','1/24','1/16','1/12','1/8','1/6','1/4','1/2','1/1'];
const MOD_RATES=['1/48','1/32','1/24','1/16','1/12','3/32','1/8','1/6','3/16','1/4','1/3','3/8','1/2','2/3','3/4','1/1'];
// None, Pitch, Porta, OSC1 c1/c2, OSC2 Semi/Tune, mixer, filters,
// Amp/Pan/Depth, EG1/2/3 ADSR, LFO1/2 frequency, Patch1...6 intensity.
const MOD_PARAMETERS=[null,53,59,11,12,15,16,17,18,19,1,2,9,25,26,22,23,27,28,7,8,31,32,33,34,35,3,4,5,6,36,37,38,39,75,83,92,96,100,104,108,112];
const signed=byte=>byte<128?byte:byte-256;
export function decodeRdlProgram(encoded){return Uint8Array.from(atob(encoded),c=>c.charCodeAt(0));}
export function sequencesFromRdl(encoded){
  const raw=decodeRdlProgram(encoded);if(raw.length<1790)throw new Error('Truncated RDL program sequences.');
  const sequencer=emptySequence(),modulation=emptyModulation(),notices=[];
  const flags=raw[1062],linked=!!(flags&16);
  function pattern(index){
    const p=raw.subarray(1134+index*328,1462+index*328),track=structuredClone(sequencer.tracks[0]),rate=p[0]>>4;
    if(rate>=NOTE_RATES.length)notices.push(`Step Seq ${index+1}: unknown resolution ${rate}; using 1/16.`);
    Object.assign(track,{enabled:!!(flags&128),length:(p[1]&31)+1,resolution:NOTE_RATES[rate]??'1/16',runMode:['OneShot','Loop','Step'][p[0]&3]??'Loop',latch:!!(p[1]&128),gateOffset:Math.max(-100,Math.min(100,signed(p[2]))),swing:Math.max(-100,Math.min(100,signed(p[3]))),transpose:!!(p[4]&128),baseNote:p[4]&127,scanBottom:p[6]&127,scanTop:p[7]&127,keySync:!!(flags&64),keyTriggered:true});
    if(track.scanBottom>track.scanTop)[track.scanBottom,track.scanTop]=[track.scanTop,track.scanBottom];
    for(let i=0;i<32;i++){
      const gate=p[8+i]&127,velocity=p[40+i];
      track.steps[i]={notes:[...new Set([...p.subarray(72+i*8,80+i*8)].filter(n=>n<128))].sort((a,b)=>a-b),samples:[],trigger:!!(p[8+i]&128)&&gate!==0,gate:Math.max(1,Math.min(100,gate)),tie:gate===100,velocity:velocity>0&&velocity<128?velocity:100,velocityMode:velocity>0&&velocity<128?'Fixed':'Key'};
    }
    return {name:`RDL Seq ${index+1}`,track};
  }
  sequencer.patterns=[pattern(0),pattern(1)];
  if(linked){
    const track=structuredClone(sequencer.patterns[0].track),second=sequencer.patterns[1].track;
    // Link LastStep is a six-bit 1...64 count. Sequence 1 supplies all common
    // settings except each half's BaseNote, including disabled step data.
    track.length=(raw[1063]&63)+1;
    for(let i=0;i<32;i++)track.steps[32+i]={...structuredClone(second.steps[i]),baseNote:second.baseNote};
    sequencer.patterns.push({name:'RDL Seq 1 + 2',track});
  }
  for(let t=0;t<4;t++){
    const start=48+t*228,assign=(raw[start]>>2)&3,patternIndex=linked?2:assign-2;
    sequencer.tracks[t].enabled=false;
    if(assign>=2)sequencer.tracks[t]={...structuredClone(sequencer.patterns[patternIndex].track),enabled:!!(flags&128)&&!!(raw[start]&128),pattern:patternIndex};
    const m=raw.subarray(start+172,start+228),sync=['Off','Timbre','Voice'][(m[1]>>4)&3]??'Off';
    for(let i=0;i<3;i++){
      const p=m.subarray(2+i*18,20+i*18),knob=p[0],parameter=MOD_PARAMETERS[knob],lane=newModulationLane(`rdl-${t+1}-${i+1}`),range=[1,5].includes(knob)?24:63;
      Object.assign(lane,{enabled:!!(m[0]&128)&&!!(raw[start]&128),length:(m[0]&15)+1,resolution:MOD_RATES[m[1]&15],direction:['Forward','Reverse','Alt1','Alt2'][(m[0]>>4)&3],runMode:(m[1]&128)||sync==='Off'?'Loop':'OneShot',keySync:sync,motion:p[1]&128?'Step':'Slide',target:parameter==null?null:{kind:'synth',timbre:t,parameter}});
      for(let s=0;s<16;s++)lane.values[s]=Math.max(-range,Math.min(range,p[2+s]-64));
      if(parameter===undefined&&knob!==0)notices.push(`Timbre ${t+1}, Mod ${i+1}: unknown knob ${knob}; its values are retained with no destination.`);
      modulation.tracks[t].push(lane);
    }
    if(sync==='Voice'&&modulation.tracks[t].some(lane=>lane.enabled&&lane.target))notices.push(`Timbre ${t+1}: Mod Sequence Voice sync resets the shared browser timbre clock on each note; independent per-voice phases are not reproduced.`);
  }
  for(const track of sequencer.tracks.slice(4))track.enabled=false;
  return {sequencer,modulation,notices};
}
export const emptyNotePatterns=sequence=>sequence.tracks.every((track,t)=>track.enabled===(t<4)&&track.length===16&&track.resolution==='1/16'&&track.runMode==null&&track.keyTriggered==null&&track.steps.every(step=>!step.notes.length&&!step.samples?.length&&step.velocity===100&&step.gate===75&&step.trigger==null&&step.tie==null&&step.velocityMode==null));
