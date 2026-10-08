import {emptySequence,validateSequence,STEPS,VIEW_STEPS,RESOLUTIONS} from './sequence.js';
import {makePicker} from './panel.js';
const noteNames=['C','C♯','D','D♯','E','F','F♯','G','G♯','A','A♯','B'];
const noteName=n=>`${noteNames[n%12]}${Math.floor(n/12)-1}`;
export function createSequencer({onChange,onPlay,onStop,onReset,onSelectTimbre,onAudition}){
  let sequence=emptySequence(),playing=false,editing,octave=4;
  const tracks=[],dialog=document.querySelector('#step-editor'),notes=document.querySelector('#step-notes'),velocity=document.querySelector('#step-velocity'),gate=document.querySelector('#step-gate');
  function commit(){onChange(validateSequence(sequence));render();}
  function label(step){return step.notes.length?`${step.notes.slice(0,2).map(noteName).join(' ')}${step.notes.length>2?` +${step.notes.length-2}`:''}`:'—';}
  function render(){
    tracks.forEach((view,t)=>{
      const track=sequence.tracks[t];view.enable.setAttribute('aria-pressed',track.enabled);view.row.classList.toggle('muted',!track.enabled);view.length.render(track.length);view.resolution.render(track.resolution);
      view.block.render(view.bank);view.steps.forEach((button,local)=>{const index=view.bank*VIEW_STEPS+local,step=track.steps[index];button.dataset.step=index;button.setAttribute('aria-label',`Timbre ${t+1} step ${index+1}`);button.querySelector('.step-index').textContent=String(index+1).padStart(2,'0');button.classList.toggle('filled',step.notes.length>0);button.classList.toggle('outside-loop',index>=track.length);button.querySelector('.step-notes').textContent=label(step);button.title=`Timbre ${t+1} · Step ${index+1}: ${step.notes.map(noteName).join(', ')||'empty'}`;});
    });
    document.querySelector('#seq-play').textContent=playing?'Stop':'Play';document.querySelector('#seq-play').setAttribute('aria-pressed',playing);
  }
  function renderEditor(){
    if(!editing)return;const step=sequence.tracks[editing.timbre].steps[editing.step];notes.replaceChildren();
    for(let i=0;i<24;i++){const note=(octave+1)*12+i;if(note>127)break;const button=document.createElement('button');button.type='button';button.className=`chord-note${[1,3,6,8,10].includes(note%12)?' black':''}`;button.textContent=noteName(note);button.setAttribute('aria-label',`Sequence note ${noteName(note)}`);button.setAttribute('aria-pressed',step.notes.includes(note));button.disabled=step.notes.length>=24&&!step.notes.includes(note);button.addEventListener('click',()=>{if(step.notes.includes(note))step.notes=step.notes.filter(n=>n!==note);else step.notes.push(note);step.notes.sort((a,b)=>a-b);commit();renderEditor();onAudition(editing.timbre,structuredClone(step),sequence.tracks[editing.timbre].resolution);});notes.append(button);}
    document.querySelector('#step-selected').textContent=step.notes.map(noteName).join(' · ')||'—';document.querySelector('#step-octave').textContent=`${noteName((octave+1)*12)}–${noteName(Math.min(127,(octave+1)*12+23))}`;
    velocity.value=step.velocity;gate.value=step.gate;document.querySelector('#step-octave-down').disabled=octave===-1;document.querySelector('#step-octave-up').disabled=octave===8;
  }
  for(let timbre=0;timbre<4;timbre++){
    const row=document.createElement('div'),head=document.createElement('div'),select=document.createElement('button'),enable=document.createElement('button'),grid=document.createElement('div');row.className='sequence-track';head.className='track-head';grid.className='track-steps';
    select.type='button';select.textContent=`T${timbre+1}`;select.className='track-select';select.setAttribute('aria-label',`Edit timbre ${timbre+1}`);select.addEventListener('click',()=>onSelectTimbre(timbre));
    enable.type='button';enable.className='track-enable';enable.setAttribute('aria-label',`Sequencer timbre ${timbre+1} enabled`);enable.innerHTML='<span class="switch-led"></span>';enable.addEventListener('click',()=>{sequence.tracks[timbre].enabled=!sequence.tracks[timbre].enabled;commit();});
    const length=makePicker({label:`Sequencer timbre ${timbre+1} length`,options:Array.from({length:STEPS},(_,i)=>({value:i+1,label:String(i+1)})),value:STEPS,onChange:value=>{sequence.tracks[timbre].length=value;commit();}});length.button.classList.add('track-length');head.append(select,enable,length.button);
    const resolution=makePicker({label:`Sequencer timbre ${timbre+1} resolution`,options:RESOLUTIONS.map(value=>({value,label:value})),value:"1/16",onChange:value=>{sequence.tracks[timbre].resolution=value;commit();}});resolution.button.classList.add("track-resolution");head.append(resolution.button);
    const view={bank:0};
    const block=makePicker({label:`Sequencer timbre ${timbre+1} block`,options:Array.from({length:STEPS/VIEW_STEPS},(_,i)=>({value:i,label:`${i*VIEW_STEPS+1}–${(i+1)*VIEW_STEPS}`})),value:0,onChange:value=>{view.bank=value;render();}});block.button.classList.add('track-block');head.append(block.button);
    const steps=Array.from({length:VIEW_STEPS},(_,local)=>{const index=local;const button=document.createElement('button');button.type='button';button.className='sequence-step';button.setAttribute('aria-label',`Timbre ${timbre+1} step ${index+1}`);button.innerHTML=`<span class="step-index">${String(index+1).padStart(2,'0')}</span><span class="step-notes">—</span>`;button.addEventListener('click',event=>{
      const index=view.bank*VIEW_STEPS+local;
      if(event.shiftKey){sequence.tracks[timbre].steps[index].notes=[];commit();return;}
      editing={timbre,step:index};document.querySelector('#step-title').textContent=`Timbre ${timbre+1} · Step ${String(index+1).padStart(2,'0')}`;renderEditor();dialog.showModal();
    });grid.append(button);return button;});row.append(head,grid);document.querySelector('#sequence-tracks').append(row);Object.assign(view,{row,enable,length,resolution,block,steps});tracks.push(view);
  }
  document.querySelector('#seq-play').addEventListener('click',()=>playing?onStop():onPlay());document.querySelector('#seq-reset').addEventListener('click',onReset);
  document.querySelector('#step-close').addEventListener('click',()=>dialog.close());document.querySelector('#step-clear').addEventListener('click',()=>{sequence.tracks[editing.timbre].steps[editing.step].notes=[];commit();renderEditor();});
  document.querySelector('#step-copy').addEventListener('click',()=>{const track=sequence.tracks[editing.timbre],next=(editing.step+1)%STEPS;tracks[editing.timbre].bank=Math.floor(next/VIEW_STEPS);track.steps[next]=structuredClone(track.steps[editing.step]);commit();editing.step=next;document.querySelector('#step-title').textContent=`Timbre ${editing.timbre+1} · Step ${String(next+1).padStart(2,'0')}`;renderEditor();});
  for(const [input,key] of [[velocity,'velocity'],[gate,'gate']])input.addEventListener('input',()=>{if(editing&&input.value!==''&&input.validity.valid){sequence.tracks[editing.timbre].steps[editing.step][key]=Number(input.value);commit();}});
  document.querySelector('#step-octave-down').addEventListener('click',()=>{octave--;renderEditor();});document.querySelector('#step-octave-up').addEventListener('click',()=>{octave++;renderEditor();});dialog.addEventListener('close',()=>{editing=null;});
  render();return {getConfig:()=>validateSequence(sequence),setConfig:config=>{sequence=validateSequence(config);tracks.forEach(view=>view.bank=0);render();},setStatus:status=>{playing=status.running;render();tracks.forEach((view,t)=>view.steps.forEach((button,index)=>button.classList.toggle('current',playing&&Number(button.dataset.step)===status.positions[t])));},setReady:ready=>{document.querySelector('#seq-play').disabled=!ready;}};
}
