import {STEPS,VIEW_STEPS,RESOLUTIONS} from './sequence.js';
import {noteName} from './sequence-labels.js';

// Display geometry only: playback still uses SequenceClock in the AudioWorklet.
export function sequenceTimeline(track,bank=0){
  if(!RESOLUTIONS.includes(track.resolution)||!Number.isInteger(bank)||bank<0||bank>=STEPS/VIEW_STEPS)throw new Error('Invalid sequence view.');
  const [n,d]=track.resolution.split('/').map(Number);
  return Array.from({length:VIEW_STEPS},(_,local)=>{
    const index=bank*VIEW_STEPS+local,quarters=index*n*4;
    const bar=Math.floor(index*n/d)+1,beat=Math.floor(quarters/d)%4+1;
    return {index,bar,beat,label:quarters%d===0?`${bar}.${beat}`:'',barStart:index===0||Math.floor(index*n/d)!==Math.floor((index-1)*n/d),inLoop:index<track.length};
  });
}

export function patternSpan(track){
  const [n,d]=track.resolution.split('/').map(Number),whole=Math.floor(track.length*n/d),rest=track.length*n%d;
  let a=rest,b=d;while(b){[a,b]=[b,a%b];}
  const fraction=rest?`${rest/a}/${d/a}`:'';
  return `${whole||!fraction?whole:''}${whole&&fraction?' + ':''}${fraction} ${track.length*n===d?'bar':'bars'}`;
}

export function pianoRows(base=48){
  base=Math.max(0,Math.min(104,Math.round(base)));
  return Array.from({length:24},(_,i)=>{const note=base+23-i;return {key:`note:${note}`,kind:'note',note,label:noteName(note),black:[1,3,6,8,10].includes(note%12)};});
}

export function sampleRows(track,kit,options,extra=[]){
  const rows=(kit?.instruments??[]).map((instrument,i)=>({key:`kit:${i}`,kind:'note',note:instrument.note,label:instrument.name,invalid:instrument.note<0||instrument.note>127}));
  const kitNotes=new Set(rows.map(row=>row.note)),sources=new Set([...track.steps.flatMap(step=>step.samples??[]),...extra]);
  const names=new Map(options.map(option=>[option.value,option.label]));
  for(const note of [...new Set(track.steps.flatMap(step=>step.notes))].sort((a,b)=>b-a))if(!kitNotes.has(note))rows.push({key:`note:${note}`,kind:'note',note,label:noteName(note)+(kit?' · Unassigned':' · Synth')});
  for(const source of sources)rows.push({key:`sample:${source}`,kind:'sample',source,label:names.get(source)??source});
  return rows;
}

export const rowHasEvent=(row,step)=>row.kind==='sample'?(step.samples??[]).includes(row.source):step.notes.includes(row.note);
