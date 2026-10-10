import {makePicker} from './panel.js';
import {VIEW_STEPS,STEPS} from './sequence.js';
import {sequenceTimeline,patternSpan,pianoRows,sampleRows,rowHasEvent} from './sequence-view.js';

function el(tag,className,text){const node=document.createElement(tag);if(className)node.className=className;if(text!=null)node.textContent=text;return node;}
function button(text,label){const node=el('button','',text);node.type='button';if(label)node.setAttribute('aria-label',label);return node;}

export function createSequenceWorkspace({sequenceUI,dock,prefs,onPreference}){
  let selected=0,count=4,base=48,queued=false,follow=false,focusNotes=false,previousMod=false;
  const extra=Array.from({length:8},()=>new Set()),sequencer=dock.querySelector('.sequencer'),head=sequencer.querySelector('.sequence-heading'),inspector=dock.querySelector('.nw-inspector');
  head.querySelector('h2').textContent='Sequencer';head.classList.add('nw-sequence-heading');
  const modes=el('div','nw-sequence-modes');modes.setAttribute('role','group');modes.setAttribute('aria-label','Sequencer view');
  const modeButtons=new Map();for(const [value,label]of [['steps','Steps'],['piano','Piano roll'],['samples','Samples']]){const control=button(label);control.addEventListener('click',()=>{prefs.mode=value;focusNotes=true;onPreference();render();});modeButtons.set(value,control);modes.append(control);}
  const position=el('output','nw-sequence-position','—');position.setAttribute('aria-label','Playing step');
  const followButton=button('Follow');followButton.setAttribute('aria-pressed','false');followButton.addEventListener('click',()=>{follow=!follow;followButton.setAttribute('aria-pressed',follow);});
  head.prepend(modes);head.append(followButton,position);head.querySelector('.sequence-transport').remove();
  const content=el('div','nw-sequence-content'),main=el('div','nw-sequence-main');main.append(sequencer);content.append(main,inspector);dock.append(head,content);
  const overview=el('div','nw-pattern-overview'),overviewCaption=el('div','nw-pattern-caption'),banks=el('div','nw-pattern-banks');banks.setAttribute('role','group');banks.setAttribute('aria-label','Pattern overview');overview.append(overviewCaption,banks);sequencer.prepend(overview);
  const controls=el('div','nw-roll-tools'),down=button('−','Lower piano roll octave'),up=button('+','Higher piano roll octave'),range=el('span','nw-roll-range'),sampleControl=el('div','nw-add-sample-row');controls.append(down,range,up,sampleControl);
  const sourcePicker=makePicker({label:'Add sample row',value:null,options:[],searchable:true,searchLabel:'Search all samples',onChange:source=>{extra[selected].add(source);render();}});sampleControl.append(sourcePicker.button);
  down.addEventListener('click',()=>{base=Math.max(0,base-12);render();});up.addEventListener('click',()=>{base=Math.min(104,base+12);render();});
  const roll=el('div','nw-roll');roll.setAttribute('aria-label','Pattern grid');const grid=el('div','nw-roll-grid');roll.append(grid);sequencer.append(controls,roll);
  const trackRoot=document.querySelector('#sequence-tracks');
  function queue(){if(queued)return;queued=true;requestAnimationFrame(()=>{queued=false;render();});}
  function paintStatus(status){
    const index=status.positions[selected]??-1;position.textContent=index<0?'—':`T${selected+1} · ${String(index+1).padStart(2,'0')}`;
    for(const node of sequencer.querySelectorAll('[data-grid-step]'))node.classList.toggle('current',Number(node.dataset.gridStep)===index);
    for(const node of banks.querySelectorAll('[data-mini-step]'))node.classList.toggle('current',Number(node.dataset.miniStep)===index);
    if(follow&&status.playing&&index>=0&&!sequenceUI.getView().editing&&sequenceUI.getView().banks[selected]!==Math.floor(index/VIEW_STEPS))sequenceUI.setBank(selected,Math.floor(index/VIEW_STEPS));
  }
  sequenceUI.subscribe(status=>status.type==='status'?paintStatus(status):queue());
  function selectStep(index,event){const view=sequenceUI.getView();if(event.shiftKey||view.selecting){const anchor=view.selection?.timbre===selected?view.selection.start:view.cursor?.timbre===selected?view.cursor.step:index;sequenceUI.selectRange(selected,anchor,index);}else sequenceUI.openStep(selected,index);}
  function render(){
    const sequence=sequenceUI.getConfig(),view=sequenceUI.getView(),track=sequence.tracks[selected],bank=view.banks[selected],kit=sequenceUI.getDrumKit(),ownKit=kit?.timbre===selected?kit:null;
    const selectedStep=index=>view.selection?.timbre===selected&&index>=Math.min(view.selection.start,view.selection.end)&&index<=Math.max(view.selection.start,view.selection.end);
    const modEditor=!sequenceUI.modulationPanels[selected].hidden;dock.classList.toggle('has-mod-editor',modEditor);if(modEditor&&!previousMod&&matchMedia('(max-width:800px)').matches)dock.scrollIntoView({block:'start',behavior:'smooth'});previousMod=modEditor;
    dock.dataset.sequenceMode=prefs.mode;modeButtons.forEach((control,mode)=>control.setAttribute('aria-pressed',prefs.mode===mode));
    for(const group of trackRoot.children)group.dataset.active=Number(group.dataset.sequenceTimbre)===selected;
    overviewCaption.textContent=`T${selected+1} · ${track.length} steps · ${track.resolution} · ${patternSpan(track)}`;banks.replaceChildren();
    for(let block=0;block<STEPS/VIEW_STEPS;block++){
      const first=block*VIEW_STEPS,control=button('',`Show timbre ${selected+1} steps ${first+1}–${first+VIEW_STEPS}`),caption=el('span','nw-bank-caption',`${first+1}–${first+VIEW_STEPS}`),mini=el('span','nw-bank-mini');control.setAttribute('aria-pressed',bank===block);control.className='nw-bank';control.classList.toggle('outside-loop',first>=track.length);
      for(let i=first;i<first+VIEW_STEPS;i++){const tick=el('span','nw-mini-step');tick.dataset.miniStep=i;tick.classList.toggle('filled',!!(track.steps[i].notes.length||track.steps[i].samples.length));tick.classList.toggle('step-muted',track.steps[i].trigger===false);tick.classList.toggle('outside-loop',i>=track.length);mini.append(tick);}
      control.append(caption,mini);control.addEventListener('click',()=>sequenceUI.setBank(selected,block));banks.append(control);
    }
    if(prefs.mode==='steps'||modEditor){controls.hidden=roll.hidden=true;paintStatus({positions:view.positions});return;}
    controls.hidden=roll.hidden=false;const piano=prefs.mode==='piano';down.hidden=up.hidden=range.hidden=!piano;sampleControl.hidden=piano;down.disabled=base===0;up.disabled=base===104;
    const options=sequenceUI.getSamples();sourcePicker.options=options;sourcePicker.render(null,'Add sample row…');sourcePicker.button.disabled=!options.length;
    const rows=piano?pianoRows(base):sampleRows(track,ownKit,options,[...extra[selected]]),timeline=sequenceTimeline(track,bank);range.textContent=piano?`${rows.at(-1).label}–${rows[0].label}`:'';
    const scroll={top:roll.scrollTop,left:roll.scrollLeft},active=document.activeElement?.dataset.gridKey,cells=new Map();grid.replaceChildren();
    const header=el('div','nw-roll-row nw-roll-head'),corner=el('div','nw-row-label',piano?'Notes':'Samples');header.append(corner);
    timeline.forEach(tick=>{const control=button('',`Open timbre ${selected+1} step ${tick.index+1}`);control.className='nw-grid-time';control.dataset.gridStep=tick.index;control.append(el('span','',String(tick.index+1).padStart(2,'0')),el('small','',tick.label));control.classList.toggle('bar-start',tick.barStart);control.classList.toggle('outside-loop',!tick.inLoop);control.classList.toggle('selected',!!selectedStep(tick.index));control.addEventListener('click',event=>selectStep(tick.index,event));header.append(control);});grid.append(header);
    rows.forEach(row=>{
      const line=el('div','nw-roll-row'),label=el('div','nw-row-label'),name=button(row.label,row.kind==='sample'?`Edit sound: ${row.label}`:`Audition ${row.label}`);name.title=row.label;name.className='nw-row-name';label.classList.toggle('black',!!row.black);line.append(label);label.append(name);
      name.disabled=!!row.invalid;name.addEventListener('click',()=>row.kind==='sample'?sequenceUI.editSample(selected,row.source):sequenceUI.previewRow(selected,row));
      if(row.kind==='sample'){const preview=button('▶',`Preview ${row.label}`);preview.className='nw-row-preview';preview.addEventListener('click',()=>sequenceUI.previewRow(selected,row));label.append(preview);}
      timeline.forEach(tick=>{
        const step=track.steps[tick.index],chosen=rowHasEvent(row,step),control=button('',`${row.label} · Step ${tick.index+1}`),key=`${row.key}:${tick.index}`;control.className='nw-grid-event';control.dataset.gridKey=key;control.dataset.gridStep=tick.index;control.setAttribute('aria-pressed',chosen);control.disabled=!!row.invalid;control.classList.toggle('bar-start',tick.barStart);control.classList.toggle('outside-loop',!tick.inLoop);control.classList.toggle('step-muted',step.trigger===false);control.classList.toggle('editing',view.editing?.timbre===selected&&view.editing.step===tick.index);control.classList.toggle('selected',!!selectedStep(tick.index));control.title=`${row.label} · Step ${tick.index+1} · Velocity ${step.velocity} · ${step.tie?'Tie':`Gate ${step.gate}%`}`;
        const marker=el('span','nw-event-marker');marker.style.setProperty('--gate',step.tie?'100%':`${step.gate}%`);marker.style.setProperty('--velocity',.4+step.velocity/127*.6);control.append(marker);
        control.addEventListener('click',event=>{if(event.shiftKey||sequenceUI.getView().selecting){selectStep(tick.index,event);return;}row.kind==='sample'?sequenceUI.toggleSample(selected,tick.index,row.source):sequenceUI.toggleNote(selected,tick.index,row.note);});
        control.addEventListener('keydown',event=>{const movement={ArrowRight:1,ArrowLeft:-1,ArrowDown:VIEW_STEPS,ArrowUp:-VIEW_STEPS};if(!(event.key in movement))return;event.preventDefault();const all=[...grid.querySelectorAll('.nw-grid-event')],index=all.indexOf(control),target=all[index+movement[event.key]];target?.focus();});
        cells.set(key,control);line.append(control);
      });grid.append(line);
    });
    if(!rows.length)grid.append(el('p','nw-grid-empty','Add a sample row to start a pattern.'));
    roll.scrollLeft=scroll.left;roll.scrollTop=scroll.top;
    if(active&&cells.has(active))cells.get(active).focus({preventScroll:true});
    if(focusNotes&&piano){const note=track.steps.flatMap(step=>step.notes).find(note=>note>=base&&note<base+24)??60;roll.scrollTop=Math.max(0,(base+23-note)*28-roll.clientHeight/2+56);focusNotes=false;}
    paintStatus({positions:view.positions});
  }
  render();
  return {setContext(timbre,timbreCount){if(selected===timbre&&count===timbreCount)return;selected=timbre;count=timbreCount;focusNotes=true;queue();}};
}
