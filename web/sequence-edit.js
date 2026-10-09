import {validSampleSource} from './sample-state.js';
import {validateSequence,STEPS} from './sequence.js';
export function copySteps(sequence,timbre,start,end=start,kit){
  const checked=validateSequence(sequence);
  if(!Number.isInteger(timbre)||timbre<0||timbre>3||![start,end].every(n=>Number.isInteger(n)&&n>=0&&n<STEPS))throw new Error('Invalid step selection.');
  const first=Math.min(start,end),last=Math.max(start,end);
  return {version:1,timbre,start:first,steps:structuredClone(checked.tracks[timbre].steps.slice(first,last+1)),...(kit?{drums:structuredClone(kit.instruments)}:{})};
}
export function pasteSteps(sequence,clipboard,timbre,start){
  const next=validateSequence(sequence);
  if(clipboard?.version!==1||!Number.isInteger(clipboard.timbre)||clipboard.timbre<0||clipboard.timbre>3||!Array.isArray(clipboard.steps)||!clipboard.steps.length||clipboard.steps.length>STEPS||!Number.isInteger(timbre)||timbre<0||timbre>3||!Number.isInteger(start)||start<0||start>=STEPS)throw new Error('Invalid step clipboard.');
  if(clipboard.drums&&(!Array.isArray(clipboard.drums)||clipboard.drums.length>16||clipboard.drums.some(d=>!Number.isInteger(d?.note)||d.note< -64||d.note>190||d.source!=null&&d.source!=='synth'&&!validSampleSource(d.source))))throw new Error('Invalid kit clipboard.');
  const count=Math.min(clipboard.steps.length,STEPS-start),track=next.tracks[timbre];
  clipboard.steps.slice(0,count).forEach((step,i)=>{
    const pasted=structuredClone(step);
    if(timbre!==clipboard.timbre&&clipboard.drums){
      pasted.notes=pasted.notes.filter(note=>{const drums=clipboard.drums.filter(d=>d.note===note);for(const drum of drums)if(drum.source&&drum.source!=='synth')pasted.samples.push(drum.source);return !drums.length||drums.some(d=>!d.source||d.source==='synth');});
      pasted.samples=[...new Set(pasted.samples)];
    }
    track.steps[start+i]=pasted;
  });
  track.length=Math.max(track.length,start+count);
  return {sequence:validateSequence(next),count,truncated:count<clipboard.steps.length};
}
