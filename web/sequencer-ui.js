import {emptySequence,validateSequence,STEPS,VIEW_STEPS,RESOLUTIONS} from './sequence.js';
import {makePicker} from './panel.js';
import {noteName,sequenceLabels} from './sequence-labels.js';
export function createSequencer({onChange,onPlay,onStop,onReset,onSelectTimbre,onSelectDrum,onAudition}){
  let sequence=emptySequence(),playing=false,editing,octave=4,drumKit=null,drumKey='null';
  const tracks=[],dialog=document.querySelector('#step-editor'),notes=document.querySelector('#step-notes'),velocity=document.querySelector('#step-velocity'),gate=document.querySelector('#step-gate');
  const kitFor=timbre=>drumKit?.timbre===timbre?drumKit:null;
  function commit(){onChange(validateSequence(sequence));render();}
  function label(step,timbre){const kit=kitFor(timbre),names=sequenceLabels(step.notes,kit),limit=kit?1:2;return names.length?`${names.slice(0,limit).join(' ')}${names.length>limit?` +${names.length-limit}`:''}`:'—';}
  function render(){
    tracks.forEach((view,t)=>{
      const track=sequence.tracks[t];view.enable.setAttribute('aria-pressed',track.enabled);view.row.classList.toggle('muted',!track.enabled);view.length.render(track.length);view.resolution.render(track.resolution);
      view.row.classList.toggle('drum-track',!!kitFor(t));
      view.block.render(view.bank);view.steps.forEach((button,local)=>{const index=view.bank*VIEW_STEPS+local,step=track.steps[index];button.dataset.step=index;button.setAttribute('aria-label',`Timbre ${t+1} step ${index+1}`);button.querySelector('.step-index').textContent=String(index+1).padStart(2,'0');button.classList.toggle('filled',step.notes.length>0);button.classList.toggle('outside-loop',index>=track.length);button.querySelector('.step-notes').textContent=label(step,t);button.title=`Timbre ${t+1} · Step ${index+1}: ${sequenceLabels(step.notes,kitFor(t)).join(', ')||'empty'}`;});
    });
    document.querySelector('#seq-play').textContent=playing?'Stop':'Play';document.querySelector('#seq-play').setAttribute('aria-pressed',playing);
  }
  function renderEditor(){
    if(!editing)return;const step=sequence.tracks[editing.timbre].steps[editing.step],kit=kitFor(editing.timbre);notes.replaceChildren();
    document.querySelector('#step-title').textContent=`Timbre ${editing.timbre+1} · Step ${String(editing.step+1).padStart(2,'0')}`;
    document.querySelector('#step-prev').disabled=editing.step===0;document.querySelector('#step-next').disabled=editing.step===STEPS-1;
    notes.classList.toggle('drum-keyboard',!!kit);document.querySelector('.chord-octaves').hidden=!!kit;
    const choices=kit?kit.instruments:Array.from({length:24},(_,i)=>({note:(octave+1)*12+i})).filter(choice=>choice.note<=127);
    for(const [index,choice] of choices.entries()){
      const {note}=choice,button=document.createElement('button');button.type='button';button.className=`chord-note${kit?' drum-note':[1,3,6,8,10].includes(note%12)?' black':''}`;
      if(kit){const number=document.createElement('span'),name=document.createElement('span');number.className='drum-index';number.textContent=String(index+1).padStart(2,'0');name.className='drum-name';name.textContent=choice.name;button.append(number,name);button.setAttribute('aria-label',`Sequence drum ${index+1}: ${choice.name}`);button.title=choice.name+(note<0||note>127?' · Trigger outside MIDI range':'');}
      else{button.textContent=noteName(note);button.setAttribute('aria-label',`Sequence note ${noteName(note)}`);}
      button.setAttribute('aria-pressed',step.notes.includes(note));button.disabled=note<0||note>127||step.notes.length>=24&&!step.notes.includes(note);
      button.addEventListener('click',()=>{if(kit)onSelectDrum?.(index,editing.timbre);if(step.notes.includes(note))step.notes=step.notes.filter(n=>n!==note);else step.notes.push(note);step.notes.sort((a,b)=>a-b);commit();renderEditor();onAudition(editing.timbre,structuredClone(step),sequence.tracks[editing.timbre].resolution);});notes.append(button);
    }
    document.querySelector('#step-selected').textContent=sequenceLabels(step.notes,kit).join(' · ')||'—';document.querySelector('#step-octave').textContent=`${noteName((octave+1)*12)}–${noteName(Math.min(127,(octave+1)*12+23))}`;
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
  function moveStep(direction){if(!editing)return;const next=editing.step+direction;if(next<0||next>=STEPS)return;editing.step=next;tracks[editing.timbre].bank=Math.floor(next/VIEW_STEPS);render();renderEditor();}
  document.querySelector('#step-prev').addEventListener('click',()=>moveStep(-1));document.querySelector('#step-next').addEventListener('click',()=>moveStep(1));
  document.querySelector('#step-copy').addEventListener('click',()=>{const track=sequence.tracks[editing.timbre],next=(editing.step+1)%STEPS;tracks[editing.timbre].bank=Math.floor(next/VIEW_STEPS);track.steps[next]=structuredClone(track.steps[editing.step]);commit();editing.step=next;document.querySelector('#step-title').textContent=`Timbre ${editing.timbre+1} · Step ${String(next+1).padStart(2,'0')}`;renderEditor();});
  for(const [input,key] of [[velocity,'velocity'],[gate,'gate']])input.addEventListener('input',()=>{if(editing&&input.value!==''&&input.validity.valid){sequence.tracks[editing.timbre].steps[editing.step][key]=Number(input.value);commit();}});
  document.querySelector('#step-octave-down').addEventListener('click',()=>{octave--;renderEditor();});document.querySelector('#step-octave-up').addEventListener('click',()=>{octave++;renderEditor();});dialog.addEventListener('close',()=>{editing=null;});
  render();return {getConfig:()=>validateSequence(sequence),setConfig:config=>{sequence=validateSequence(config);tracks.forEach(view=>view.bank=0);render();if(dialog.open)renderEditor();},setDrumKit:kit=>{const key=JSON.stringify(kit);if(key===drumKey)return;drumKey=key;drumKit=kit;render();if(dialog.open)renderEditor();},setStatus:status=>{playing=status.running;render();tracks.forEach((view,t)=>view.steps.forEach((button,index)=>button.classList.toggle('current',playing&&Number(button.dataset.step)===status.positions[t])));},setReady:ready=>{document.querySelector('#seq-play').disabled=!ready;}};
}
