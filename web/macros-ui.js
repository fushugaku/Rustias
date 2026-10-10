import {MacroBank,MACRO_COUNT,MACRO_BINDINGS,targetKey} from './macros.js';
import {makeDial} from './panel.js';
import {setMacroAssignmentHandler} from './macro-gesture.js';
export function createMacrosPanel({resolve,write,onChange,onError}){
  const root=document.querySelector('#macros'),views=[];
  const dialog=document.createElement('dialog');dialog.className='editor-dialog macro-dialog';dialog.id='macro-editor';
  dialog.innerHTML='<div class="dialog-heading"><h2 id="macro-title">Assign to macro</h2><button type="button" id="macro-close" aria-label="Close macro editor">×</button></div><p id="macro-parameter"></p><div id="macro-choices" class="macro-choices"></div><div id="macro-settings" hidden><label class="macro-name-label">Name<input id="macro-name" type="text" maxlength="32" aria-label="Macro name"></label><div id="macro-bindings"></div><p id="macro-empty">Right-click a parameter to assign it. On touch, press and hold.</p></div>';
  document.body.append(dialog);
  const $=s=>dialog.querySelector(s);let editing=0,pending;
  const bank=new MacroBank({resolve,write:changes=>{write(changes);onChange();refresh();}});
  function attempt(fn){try{fn();}catch(error){onError(error);}}
  function refresh(){for(let i=0;i<views.length;i++){const knob=bank.config.knobs[i],view=views[i];view.label.textContent=knob.name;view.label.title=knob.name;view.dial.render(knob.value);view.count.textContent=`${knob.bindings.length} / ${MACRO_BINDINGS}`;view.edit.title=`Edit ${knob.name} assignments`;view.edit.setAttribute('aria-label',`Edit Macro ${i+1} assignments`);}}
  function edit(index){editing=index;pending=null;const knob=bank.config.knobs[index];$('#macro-title').textContent=`Macro ${index+1}`;$('#macro-parameter').hidden=true;$('#macro-choices').hidden=true;$('#macro-settings').hidden=false;$('#macro-name').value=knob.name;renderBindings();if(!dialog.open)dialog.showModal();}
  function renderBindings(){const rows=$('#macro-bindings');rows.replaceChildren();const knob=bank.config.knobs[editing];$('#macro-empty').hidden=!!knob.bindings.length;
    knob.bindings.forEach(binding=>{
      const spec=resolve(binding.target),base=bank.config.bases.find(b=>targetKey(b.target)===targetKey(binding.target)),row=document.createElement('div'),label=document.createElement('label'),slider=document.createElement('input'),value=document.createElement('output'),remove=document.createElement('button');
      row.className='macro-binding';label.textContent=spec?.label??base?.label??'Unavailable parameter';label.title=label.textContent;row.classList.toggle('unavailable',!spec||spec.available===false);
      slider.type='range';slider.min=-100;slider.max=100;slider.step=1;slider.value=binding.amount;slider.setAttribute('aria-label',`${label.textContent} influence`);const paint=()=>{value.value=`${binding.amount>0?'+':''}${binding.amount}%`;slider.setAttribute('aria-valuetext',value.value);};paint();
      slider.addEventListener('input',()=>attempt(()=>{bank.setAmount(editing,binding.target,Number(slider.value));paint();}));remove.type='button';remove.textContent='×';remove.setAttribute('aria-label',`Remove macro assignment: ${label.textContent}`);remove.addEventListener('click',()=>attempt(()=>{bank.remove(editing,binding.target);renderBindings();}));row.append(label,slider,value,remove);rows.append(row);
    });
  }
  function assign(target){const spec=resolve(target);if(!spec||spec.available===false)return;pending=target;$('#macro-title').textContent='Assign to macro';$('#macro-parameter').textContent=spec.label;$('#macro-parameter').hidden=false;$('#macro-settings').hidden=true;const choices=$('#macro-choices');choices.hidden=false;choices.replaceChildren();
    bank.config.knobs.forEach((knob,index)=>{const button=document.createElement('button'),name=document.createElement('span'),count=document.createElement('span');button.type='button';button.className='macro-choice';name.textContent=knob.name;count.textContent=`${knob.bindings.length} / ${MACRO_BINDINGS}`;button.append(name,count);button.setAttribute('aria-label',`Assign to Macro ${index+1}`);const assigned=knob.bindings.some(b=>targetKey(b.target)===targetKey(target));button.setAttribute('aria-pressed',assigned);button.disabled=!assigned&&knob.bindings.length===MACRO_BINDINGS;button.addEventListener('click',()=>attempt(()=>{bank.assign(index,pending);edit(index);}));choices.append(button);});
    if(!dialog.open)dialog.showModal();
  }
  for(let i=0;i<MACRO_COUNT;i++){const field=document.createElement('div'),label=document.createElement('label'),editButton=document.createElement('button'),count=document.createElement('span');field.className='macro-control';const dial=makeDial({label:`Macro ${i+1}`,min:0,max:100,defaultValue:0,read:()=>bank.config.knobs[i].value,onChange:value=>attempt(()=>bank.setValue(i,value)),format:v=>`${v}%`});editButton.type='button';editButton.className='macro-edit';editButton.innerHTML='<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m16 3 5 5M4 15 15 4a2 2 0 0 1 3 0l2 2a2 2 0 0 1 0 3L9 20l-6 1 1-6Z"/></svg>';editButton.append(count);editButton.addEventListener('click',()=>edit(i));field.append(label,dial.button,dial.number,editButton);root.append(field);views.push({dial,label,edit:editButton,count});}
  $('#macro-close').addEventListener('click',()=>dialog.close());dialog.addEventListener('close',()=>{pending=null;});
  $('#macro-name').addEventListener('input',()=>{const name=$('#macro-name').value.trim();if(name){bank.config.knobs[editing].name=name;refresh();onChange();}});$('#macro-name').addEventListener('blur',()=>{$('#macro-name').value=bank.config.knobs[editing].name;});
  setMacroAssignmentHandler(assign);refresh();
  return {getConfig:()=>bank.getConfig(),setConfig(raw){bank.setConfig(raw);if(dialog.open)dialog.close();refresh();},setLiveValues:values=>views.forEach((view,i)=>view.dial.setModulation(values?.[i])),rebase:target=>bank.rebase(target),rebaseTimbre(t){for(const base of bank.config.bases)if(base.target.timbre===t||base.target.kind==='effect'&&Math.floor(base.target.slot/2)===t)bank.rebase(base.target);},refresh};
}
