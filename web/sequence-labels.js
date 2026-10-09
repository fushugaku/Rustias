const pitches=['C','C♯','D','D♯','E','F','F♯','G','G♯','A','A♯','B'];
export const noteName=note=>`${pitches[note%12]}${Math.floor(note/12)-1}`;

// The sequencer keeps MIDI notes. Match the same triggers as WebEngine::note,
// including kit transpose and multiple instruments assigned to one trigger.
export function drumSequenceKit(global,drums,names=[]){
  if(!global[140])return null;
  return {timbre:global[141],instruments:drums.map((values,index)=>({
    note:values[146]+global[145]-64,
    name:names[index]??`Drum ${String(index+1).padStart(2,'0')}`,
  }))};
}
export function sequenceLabels(notes,kit){
  if(!kit)return notes.map(noteName);
  return notes.flatMap(note=>{
    const instruments=kit.instruments.filter(instrument=>instrument.note===note);
    return instruments.length?instruments.map(instrument=>instrument.name):[`Unassigned ${note}`];
  });
}
