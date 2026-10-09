export const emptySamples=()=>({version:2,slots:Array.from({length:16},()=>({source:'synth',mode:0}))});
export function validateSamples(value){
  if(value==null)return emptySamples();
  if(![1,2].includes(value.version)||!Array.isArray(value.slots)||value.slots.length!==16)throw new Error('Invalid drum sample assignments.');
  return {version:2,slots:value.slots.map(slot=>{
    if(typeof slot?.source!=='string'||!(/^(synth|(?:808|909):[a-z0-9-]+|custom:[a-zA-Z0-9-]+)$/.test(slot.source))||![0,1,2].includes(slot.mode))throw new Error('Invalid drum sample assignment.');
    return {source:slot.source,mode:slot.mode,...(slot.name?{name:String(slot.name).slice(0,100)}:{})};
  })};
}
// Old PCM patches used the owning timbre's manual gain. Preserve that sound
// when moving to per-instrument gains; version 2 keeps independent edits.
export function migrateSampleAmplifiers(engine,previous,samples){
  if(previous?.version!==1)return engine;
  const result=structuredClone(engine),gain=result.timbres[result.timbres[0][141]][115];
  samples.slots.forEach((slot,index)=>{if(slot.source!=='synth')result.drums[index][115]=gain;});
  return result;
}
