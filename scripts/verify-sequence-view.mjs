import assert from 'node:assert/strict';
import {emptySequence,RESOLUTIONS} from '../web/sequence.js';
import {sequenceTimeline,patternSpan,pianoRows,sampleRows,rowHasEvent} from '../web/sequence-view.js';

export function verifySequenceView(){
  const track=emptySequence().tracks[0];track.length=128;
  assert.equal(patternSpan(track),'8 bars');
  assert.deepEqual(sequenceTimeline(track).filter(t=>t.label).map(t=>[t.index,t.label]),[[0,'1.1'],[4,'1.2'],[8,'1.3'],[12,'1.4']]);
  assert.deepEqual(sequenceTimeline(track,7).filter(t=>t.label).map(t=>[t.index,t.label]),[[112,'8.1'],[116,'8.2'],[120,'8.3'],[124,'8.4']]);
  track.resolution='1/3';assert.deepEqual(sequenceTimeline(track).slice(0,4).map(t=>[t.bar,t.beat,t.label]),[[1,1,'1.1'],[1,2,''],[1,3,''],[2,1,'2.1']]);
  track.length=4;assert.equal(patternSpan(track),'1 + 1/3 bars');
  for(const resolution of RESOLUTIONS){track.resolution=resolution;const ticks=sequenceTimeline(track,7);assert.equal(ticks.at(-1).index,127);assert.ok(ticks.every(t=>Number.isFinite(t.bar)&&t.beat>=1&&t.beat<=4));}
  assert.throws(()=>sequenceTimeline(track,8));
  assert.equal(pianoRows(0).at(-1).note,0);assert.equal(pianoRows(104)[0].note,127);
  track.steps[127]={notes:[60,127],samples:['factory:0','user:sample'],velocity:71,gate:28,trigger:false};
  const kit={instruments:[{note:60,name:'909 Kick'},{note:-1,name:'Outside range'}]},options=[{value:'factory:0',label:'808 Snare'},{value:'user:sample',label:'My percussion'}];
  const before=structuredClone(track),rows=sampleRows(track,kit,options,['factory:0','user:extra']);
  assert.equal(rows.find(row=>row.note===127).label,'G9 · Unassigned');
  assert.equal(rows.find(row=>row.source==='factory:0').label,'808 Snare');
  assert.equal(rows.filter(row=>row.source==='factory:0').length,1);
  assert.ok(rows.find(row=>row.note===-1).invalid);
  assert.ok(rowHasEvent(rows[0],track.steps[127]),'Muted kit triggers remain visible outside the shortened loop');
  assert.ok(rowHasEvent(rows.find(row=>row.source==='user:sample'),track.steps[127]));
  assert.deepEqual(track,before,'Changing the presentation never rewrites sequencer data');
  return 'New interface: all 128 steps, musical straight/triplet/dotted rulers, full MIDI range and lossless kit/direct-sample rows';
}

if(process.argv[1]===new URL(import.meta.url).pathname)console.log(verifySequenceView());
