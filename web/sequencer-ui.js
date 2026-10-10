import {MAX_TIMBRES,INITIAL_TIMBRES,timbreArray} from './limits.js';
import {emptySequence,validateSequence,STEPS,VIEW_STEPS,RESOLUTIONS,MAX_EVENTS,stepEvents} from './sequence.js';
import {copySteps,pasteSteps} from './sequence-edit.js';
import {makePicker,isChoosing} from './panel.js';
import {noteName,sequenceLabels} from './sequence-labels.js';
const $=selector=>document.querySelector(selector);
export function createSequencer({onChange,onPlay,onStop,onReset,onSelectTimbre,onSelectDrum,onAudition,onSample,onEditSample,onCopySamples,onUploadSample,onError}){
  let sequence=emptySequence(),playing=false,editing,octave=4,drumKit=null,drumKey='null',sampleOptions=[],positions=timbreArray(-1),timbreCount=INITIAL_TIMBRES,selection,cursor,clipboard,selecting=false,drag,suppressClick=false,undo,notice='',epoch=0;
  const tracks=[],dialog=$('#step-editor'),notes=$('#step-notes'),velocity=$('#step-velocity'),gate=$('#step-gate');
  const kitFor=timbre=>drumKit?.timbre===timbre?drumKit:null;
  const sampleName=source=>sampleOptions.find(o=>o.value===source)?.label??source;
  const labels=(step,timbre)=>[...sequenceLabels(step.notes,kitFor(timbre)),...step.samples.map(sampleName)];
  const destination=()=>editing??cursor??(selection?{timbre:selection.timbre,step:Math.min(selection.start,selection.end)}:null);
  function commit(){onChange(validateSequence(sequence));render();}
  function label(step,timbre){const names=labels(step,timbre),limit=kitFor(timbre)||step.samples.length?1:2;return names.length?`${names.slice(0,limit).join(' ')}${names.length>limit?` +${names.length-limit}`:''}`:'—';}
  function render(){
    tracks.forEach((view,t)=>{
      const track=sequence.tracks[t];view.group.hidden=t>=timbreCount;view.enable.setAttribute('aria-pressed',track.enabled);view.row.classList.toggle('muted',!track.enabled);view.length.render(track.length);view.resolution.render(track.resolution);view.row.classList.toggle('drum-track',!!kitFor(t));view.block.render(view.bank);
      view.steps.forEach((button,local)=>{const index=view.bank*VIEW_STEPS+local,step=track.steps[index],active=editing?.timbre===t&&editing.step===index,inRange=selection?.timbre===t&&index>=Math.min(selection.start,selection.end)&&index<=Math.max(selection.start,selection.end);
        button.dataset.step=index;button.setAttribute('aria-label',`Timbre ${t+1} step ${index+1}`);button.setAttribute('aria-current',active?'step':'false');button.setAttribute('aria-pressed',!!inRange);button.querySelector('.step-index').textContent=String(index+1).padStart(2,'0');button.classList.toggle('filled',stepEvents(step).length>0);button.classList.toggle('editing',active);button.classList.toggle('selected',!!inRange);button.classList.toggle('outside-loop',index>=track.length);button.classList.toggle('sample-step',step.samples.length>0);button.classList.toggle('current',positions[t]===index);button.querySelector('.step-notes').textContent=label(step,t);button.title=`Timbre ${t+1} · Step ${index+1}: ${labels(step,t).join(', ')||'empty'}`;
      });
    });
    $('#seq-play').textContent=playing?'Stop':'Play';$('#seq-play').setAttribute('aria-pressed',playing);$('#seq-select').setAttribute('aria-pressed',selecting);$('#seq-copy').disabled=!selection&&!editing;$('#seq-paste').disabled=!clipboard||!destination();$('#step-paste').disabled=!clipboard;$('#seq-undo').disabled=!undo;
    $('#seq-selection').textContent=notice||(selection?`T${selection.timbre+1} · ${Math.min(selection.start,selection.end)+1}–${Math.max(selection.start,selection.end)+1}`:clipboard?`${clipboard.steps.length} copied`:'');
  }
  function audition(timbre,step){onAudition(timbre,structuredClone(step),sequence.tracks[timbre].resolution);}
  async function addSample(source,target=editing){
    if(!target)return;const position={...target},requestEpoch=epoch,step=sequence.tracks[position.timbre].steps[position.step];
    if(step.samples.includes(source)||stepEvents(step).length>=MAX_EVENTS)return;
    try{await onSample(position.timbre,source);if(requestEpoch!==epoch)return;const current=sequence.tracks[position.timbre].steps[position.step];if(current.samples.includes(source)||stepEvents(current).length>=MAX_EVENTS)return;
      current.samples.push(source);commit();renderEditor();audition(position.timbre,current);
    }catch(error){onError(error);}
  }
  const sourcePicker=makePicker({label:'Add sequence sample',options:[],value:null,searchable:true,searchLabel:'Search all samples',onChange:source=>addSample(source)});$('#step-sample-source').append(sourcePicker.button);
  $('#step-sample-upload').addEventListener('click',()=>{const target={...editing};onUploadSample(target.timbre,source=>addSample(source,target));});
  function renderEditor(){
    if(!editing)return;const step=sequence.tracks[editing.timbre].steps[editing.step],kit=kitFor(editing.timbre);notes.replaceChildren();
    $('#step-title').textContent=`Timbre ${editing.timbre+1} · Step ${String(editing.step+1).padStart(2,'0')}`;$('#step-prev').disabled=editing.step===0;$('#step-next').disabled=editing.step===STEPS-1;
    notes.classList.toggle('drum-keyboard',!!kit);$('.chord-octaves').hidden=!!kit;
    const choices=kit?kit.instruments:Array.from({length:24},(_,i)=>({note:(octave+1)*12+i})).filter(choice=>choice.note<=127);
    for(const [index,choice] of choices.entries()){
      const {note}=choice,button=document.createElement('button');button.type='button';button.className=`chord-note${kit?' drum-note':[1,3,6,8,10].includes(note%12)?' black':''}`;
      if(kit){const number=document.createElement('span'),name=document.createElement('span');number.className='drum-index';number.textContent=String(index+1).padStart(2,'0');name.className='drum-name';name.textContent=choice.name;button.append(number,name);button.setAttribute('aria-label',`Sequence drum ${index+1}: ${choice.name}`);button.title=choice.name+(note<0||note>127?' · Trigger outside MIDI range':'');}
      else{button.textContent=noteName(note);button.setAttribute('aria-label',`Sequence note ${noteName(note)}`);}
      button.setAttribute('aria-pressed',step.notes.includes(note));button.disabled=note<0||note>127||stepEvents(step).length>=MAX_EVENTS&&!step.notes.includes(note);
      button.addEventListener('click',()=>{if(kit)onSelectDrum?.(index,editing.timbre);if(step.notes.includes(note))step.notes=step.notes.filter(n=>n!==note);else step.notes.push(note);step.notes.sort((a,b)=>a-b);commit();renderEditor();audition(editing.timbre,step);});notes.append(button);
    }
    $('#step-selected').textContent=sequenceLabels(step.notes,kit).join(' · ')||'—';$('#step-octave').textContent=`${noteName((octave+1)*12)}–${noteName(Math.min(127,(octave+1)*12+23))}`;
    sourcePicker.options=sampleOptions;sourcePicker.render(null,'Add sample…');sourcePicker.button.disabled=!sampleOptions.length||stepEvents(step).length>=MAX_EVENTS;
    const samples=$('#step-samples');samples.replaceChildren();
    for(const source of step.samples){const chip=document.createElement('div'),edit=document.createElement('button'),remove=document.createElement('button');chip.className='sequence-sample';edit.type=remove.type='button';edit.className='sample-edit';edit.textContent=sampleName(source);edit.setAttribute('aria-label',`Edit sound: ${sampleName(source)}`);edit.title=`Edit sound: ${sampleName(source)}`;remove.className='sample-remove';remove.textContent='×';remove.setAttribute('aria-label',`Remove sample: ${sampleName(source)}`);
      edit.addEventListener('click',()=>{const timbre=editing.timbre;dialog.close();onEditSample(timbre,source);});remove.addEventListener('click',()=>{step.samples=step.samples.filter(id=>id!==source);commit();renderEditor();audition(editing.timbre,step);});chip.append(edit,remove);samples.append(chip);
    }
    velocity.value=step.velocity;gate.value=step.gate;$('#step-octave-down').disabled=octave===-1;$('#step-octave-up').disabled=octave===8;$('#step-paste').disabled=!clipboard;
  }
  function selectRange(timbre,start,end=start){selection={timbre,start,end};cursor={timbre,step:Math.min(start,end)};notice='';render();}
  function copy(){const range=selection??(editing?{timbre:editing.timbre,start:editing.step,end:editing.step}:null);if(!range)return;clipboard=copySteps(sequence,range.timbre,range.start,range.end,kitFor(range.timbre));notice=`${clipboard.steps.length} copied`;selection=null;cursor=null;drag=null;render();}
  function paste(){const target=destination();if(!target||!clipboard)return;undo=structuredClone(sequence);const result=pasteSteps(sequence,clipboard,target.timbre,target.step);onCopySamples(clipboard.timbre,target.timbre,result.sequence.tracks[target.timbre].steps.slice(target.step,target.step+result.count).flatMap(s=>s.samples));sequence=result.sequence;selection={timbre:target.timbre,start:target.step,end:target.step+result.count-1};notice=`${result.count} pasted${result.truncated?' · end of pattern':''}`;selecting=false;commit();renderEditor();}
  for(let timbre=0;timbre<MAX_TIMBRES;timbre++){
    const group=document.createElement('div'),modPanel=document.createElement('div'),modToggle=document.createElement('button');group.className='sequence-lane';modPanel.className='modulation-panel';modPanel.id='modulation-timbre-'+timbre;modPanel.hidden=true;modPanel.setAttribute('role','region');modPanel.setAttribute('aria-label','Mod sequences timbre '+(timbre+1));
    modToggle.type='button';modToggle.className='modulation-toggle';modToggle.textContent='▸';modToggle.setAttribute('aria-label','Mod sequences timbre '+(timbre+1));modToggle.setAttribute('aria-expanded','false');modToggle.setAttribute('aria-controls',modPanel.id);modToggle.addEventListener('click',()=>{modPanel.hidden=!modPanel.hidden;modToggle.setAttribute('aria-expanded',!modPanel.hidden);modToggle.textContent=modPanel.hidden?'▸':'▾';if(!modPanel.hidden)onSelectTimbre(timbre);modPanel.dispatchEvent(new Event('modulation-open'));});
    const row=document.createElement('div'),head=document.createElement('div'),select=document.createElement('button'),enable=document.createElement('button'),grid=document.createElement('div');row.className='sequence-track';head.className='track-head';grid.className='track-steps';
    select.type='button';select.textContent=`T${timbre+1}`;select.className='track-select';select.setAttribute('aria-label',`Edit timbre ${timbre+1}`);select.addEventListener('click',()=>onSelectTimbre(timbre));
    enable.type='button';enable.className='track-enable';enable.setAttribute('aria-label',`Sequencer timbre ${timbre+1} enabled`);enable.innerHTML='<span class="switch-led"></span>';enable.addEventListener('click',()=>{sequence.tracks[timbre].enabled=!sequence.tracks[timbre].enabled;commit();});
    const length=makePicker({label:`Sequencer timbre ${timbre+1} length`,options:Array.from({length:STEPS},(_,i)=>({value:i+1,label:String(i+1)})),value:16,onChange:value=>{sequence.tracks[timbre].length=value;commit();}});length.button.classList.add('track-length');head.append(select,modToggle,enable,length.button);
    const resolution=makePicker({label:`Sequencer timbre ${timbre+1} resolution`,options:RESOLUTIONS.map(value=>({value,label:value})),value:'1/16',onChange:value=>{sequence.tracks[timbre].resolution=value;commit();}});resolution.button.classList.add('track-resolution');head.append(resolution.button);
    const view={bank:0};
    const block=makePicker({label:`Sequencer timbre ${timbre+1} block`,options:Array.from({length:STEPS/VIEW_STEPS},(_,i)=>({value:i,label:`${i*VIEW_STEPS+1}–${(i+1)*VIEW_STEPS}`})),value:0,onChange:value=>{view.bank=value;render();}});block.button.classList.add('track-block');head.append(block.button);
    const steps=Array.from({length:VIEW_STEPS},(_,local)=>{
      const button=document.createElement('button');button.type='button';button.className='sequence-step';button.innerHTML='<span class="step-index"></span><span class="step-notes">—</span>';
      button.addEventListener('pointerdown',event=>{if(event.button!==0||event.pointerType==='touch'&&!selecting)return;drag={timbre,start:view.bank*VIEW_STEPS+local,moved:false};});
      button.addEventListener('pointerenter',()=>{if(drag?.timbre===timbre){const index=view.bank*VIEW_STEPS+local;if(index!==drag.start){drag.moved=true;selecting=true;selectRange(timbre,drag.start,index);}}});
      button.addEventListener('click',event=>{
        const index=view.bank*VIEW_STEPS+local;if(suppressClick){suppressClick=false;return;}
        if(event.altKey){sequence.tracks[timbre].steps[index]={notes:[],samples:[],velocity:100,gate:75};commit();return;}
        if(event.shiftKey||selecting){const anchor=selection?.timbre===timbre?selection.start:cursor?.timbre===timbre?cursor.step:index;selecting=true;selectRange(timbre,anchor,index);return;}
        cursor={timbre,step:index};selection=null;notice='';editing={...cursor};renderEditor();dialog.showModal();render();
      });grid.append(button);return button;
    });row.append(head,grid);group.append(row,modPanel);$('#sequence-tracks').append(group);Object.assign(view,{group,row,modPanel,modToggle,enable,length,resolution,block,steps});tracks.push(view);
  }
  document.addEventListener('pointerup',()=>{suppressClick=!!drag?.moved;drag=null;setTimeout(()=>{suppressClick=false;},20);});document.addEventListener('pointercancel',()=>{drag=null;suppressClick=false;});
  $('#seq-select').addEventListener('click',()=>{selecting=!selecting;selection=null;cursor=null;notice='';render();});$('#seq-copy').addEventListener('click',copy);$('#seq-paste').addEventListener('click',paste);$('#step-paste').addEventListener('click',paste);$('#seq-undo').addEventListener('click',()=>{if(undo){sequence=undo;undo=null;notice='Undone';commit();renderEditor();}});
  $('#seq-play').addEventListener('click',()=>playing?onStop():onPlay());$('#seq-reset').addEventListener('click',onReset);
  $('#step-close').addEventListener('click',()=>dialog.close());dialog.addEventListener('close',()=>{editing=null;render();});
  function moveStep(offset){if(!editing)return;editing.step=Math.max(0,Math.min(STEPS-1,editing.step+offset));cursor={...editing};tracks[editing.timbre].bank=Math.floor(editing.step/VIEW_STEPS);render();renderEditor();}
  $('#step-prev').addEventListener('click',()=>moveStep(-1));$('#step-next').addEventListener('click',()=>moveStep(1));
  $('#step-octave-down').addEventListener('click',()=>{octave=Math.max(-1,octave-1);renderEditor();});$('#step-octave-up').addEventListener('click',()=>{octave=Math.min(8,octave+1);renderEditor();});
  velocity.addEventListener('input',()=>{if(editing&&velocity.validity.valid&&velocity.value!==''){sequence.tracks[editing.timbre].steps[editing.step].velocity=Number(velocity.value);commit();}});gate.addEventListener('input',()=>{if(editing&&gate.validity.valid&&gate.value!==''){sequence.tracks[editing.timbre].steps[editing.step].gate=Number(gate.value);commit();}});
  $('#step-clear').addEventListener('click',()=>{sequence.tracks[editing.timbre].steps[editing.step].notes=[];sequence.tracks[editing.timbre].steps[editing.step].samples=[];commit();renderEditor();});
  $('#step-copy').addEventListener('click',()=>{const step=sequence.tracks[editing.timbre].steps[editing.step];if(editing.step<STEPS-1){undo=structuredClone(sequence);sequence.tracks[editing.timbre].steps[editing.step+1]=structuredClone(step);commit();moveStep(1);}});
  const textInput=target=>target.closest('input,select,[contenteditable=true],[role=combobox],[role=option]');
  document.addEventListener('copy',event=>{
    if(textInput(event.target)||isChoosing()||!selection&&!editing)return;
    copy();event.clipboardData?.setData('text/plain',JSON.stringify({type:'rustias-steps',...clipboard}));event.preventDefault();
  });
  document.addEventListener('paste',event=>{
    if(textInput(event.target)||isChoosing()||!destination())return;
    const text=event.clipboardData?.getData('text/plain')??'';
    try{if(text.startsWith('{')&&text.length<1024*1024){const value=JSON.parse(text);if(value.type==='rustias-steps'){const target=destination();pasteSteps(sequence,value,target.timbre,target.step);clipboard=value;}}if(clipboard){paste();event.preventDefault();}}catch(error){onError(error);}
  });
  document.addEventListener('keydown',event=>{
    if(textInput(event.target)||isChoosing())return;
    if(event.ctrlKey||event.metaKey){const key=event.key.toLowerCase();if(key==='c'&&(selection||editing)){event.preventDefault();copy();}else if(key==='v'&&clipboard&&destination()){event.preventDefault();paste();}else if(key==='a'&&cursor){event.preventDefault();selecting=true;selectRange(cursor.timbre,0,sequence.tracks[cursor.timbre].length-1);}else if(key==='z'&&undo){event.preventDefault();$('#seq-undo').click();}}
    else if(event.key==='Escape'&&!dialog.open){selection=null;selecting=false;notice='';render();}
  });
  render();
  return {getConfig:()=>validateSequence(sequence),setConfig:value=>{epoch++;sequence=validateSequence(value);selection=null;cursor=null;undo=null;notice='';if(dialog.open)dialog.close();editing=null;tracks.forEach(view=>{view.bank=0;});render();},
    modulationPanels:tracks.map(view=>view.modPanel),setModulationCounts:counts=>{tracks.forEach((view,t)=>{view.modToggle.classList.toggle('has-modulation',counts[t]>0);view.modToggle.title='Mod sequences '+counts[t]+' / 6';});},
    setSamples:options=>{sampleOptions=options;sourcePicker.options=options;render();if(editing)renderEditor();},
    setDrumKit:kit=>{const key=JSON.stringify(kit);if(key===drumKey)return;drumKey=key;drumKit=kit;render();if(editing)renderEditor();},
    setStatus:status=>{playing=status.running;positions=[...status.positions];tracks.forEach((view,t)=>view.steps.forEach((button,local)=>button.classList.toggle('current',status.positions[t]===view.bank*VIEW_STEPS+local)));$('#seq-play').textContent=playing?'Stop':'Play';$('#seq-play').setAttribute('aria-pressed',playing);},
    setTimbreCount:count=>{timbreCount=count;render();},
    setReady:ready=>{$('#seq-play').disabled=!ready;}
  };
}
