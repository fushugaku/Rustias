import {MASTER_EFFECT_SLOT,TIMBRE_EFFECTS,timbreEffectSlot,effectUsesMaster} from './limits.js';
import {bindMacroTarget} from './macro-gesture.js';
import {makePicker,makeDial} from './panel.js';
import {emptyEffects,validateEffects,defaultEffect,effectDefinitions} from './effects.js';
import {effectChoices,effectValueLabel,effectPropertyLabel,effectFieldState} from './effect-display.js';
export function createEffectsPanel({getSelected,onChange,onError}){
  let config=emptyEffects(),views=[];
  const root=document.querySelector('#effects');
  const slot=role=>role===TIMBRE_EFFECTS?MASTER_EFFECT_SLOT:timbreEffectSlot(getSelected(),role);
  function update(i,fn,rebuild=false,parameter){try{fn(config.slots[i]);onChange(i,structuredClone(config.slots[i]),parameter);rebuild?render():refresh();}catch(error){onError(error);}}
  function render(){root.replaceChildren();views=[];
    for(let role=0;role<=TIMBRE_EFFECTS;role++){
      const i=slot(role),shared=role===TIMBRE_EFFECTS,master=effectUsesMaster(i),fx=config.slots[i],label=shared?'MASTER FX':role<2?`INSERT FX ${role+1}`:`TIMBRE FX ${role+1}`,aria=shared?'Master FX':`Timbre ${getSelected()+1} ${role<2?'Insert ':''}FX ${role+1}`;
      const module=document.createElement('section'),heading=document.createElement('div'),title=document.createElement('h2'),power=document.createElement('button'),controls=document.createElement('div'),type=document.createElement('div'),body=document.createElement('div');
      module.className='module fx-module';module.setAttribute('aria-label',aria);heading.className='module-heading';title.textContent=label;power.type='button';power.className='fx-power';power.setAttribute('aria-label',`${aria} enabled`);power.addEventListener('click',()=>update(i,p=>p.enabled=!p.enabled));heading.append(title,power);
      type.className='fx-type';const definitions=effectDefinitions(master),definition=definitions[fx.kind],picker=makePicker({label:`${aria} type`,value:fx.kind,options:definitions.map(d=>({value:d.kind,label:d.name})),searchable:true,searchLabel:`Search ${aria} effects`,onChange:kind=>update(i,()=>config.slots[i]=defaultEffect(kind,master),true)});type.append(picker.button);body.className='module-controls fx-properties';
      const fields=[];
      if(fx.kind!==0)for(const [index,p]of definition.properties.entries()){
        const duplicate=definition.properties.filter(q=>q.name===p.name).length>1;
        const name=effectPropertyLabel(definition,master,index),controlName=duplicate&&/^(?:[LCR] Delay|Tap[12]Delay|Delay|Duration)$/.test(p.name)?`${name} ${p.max<=16?'sync':'free'}`:name;
        const wrap=document.createElement('div'),text=document.createElement('label');wrap.className='parameter';wrap.dataset.effectSlot=i;wrap.dataset.effectParameter=index;text.textContent=name;text.title=controlName;wrap.append(text);
        const target=()=>({kind:'effect',slot:i,effectKind:fx.kind,parameter:index}),read=()=>config.slots[i].parameters[index]-p.zero,format=value=>effectValueLabel(definition,master,index,value,config.slots[i].parameters);
        let field;
        if(p.min===0&&p.max===1&&['TempoSync','Key Sync'].includes(p.name)){
          const button=document.createElement('button');button.type='button';button.className='switch-control';button.setAttribute('aria-label',`${aria} ${controlName}`);button.addEventListener('click',()=>update(i,f=>f.parameters[index]=(read()?0:1)+p.zero,false,index));bindMacroTarget(button,target);wrap.append(button);
          field={render(){button.textContent=format(read());button.setAttribute('aria-pressed',read()!==0);},inputs:[button]};
        }else{
          const dial=makeDial({label:`${aria} ${controlName}`,min:p.min,max:p.max,defaultValue:p.default-p.zero,read,onChange:value=>update(i,f=>f.parameters[index]=value+p.zero,false,index),macroTarget:target,format,choices:effectChoices(definition,master,index,fx.parameters)});wrap.append(dial.button,dial.number);
          field={render(){dial.setChoices(effectChoices(definition,master,index,config.slots[i].parameters));dial.render();},inputs:[dial.button,dial.number]};
        }
        field.state=()=>effectFieldState(definition,master,index,config.slots[i].parameters);field.wrap=wrap;fields.push(field);body.append(wrap);
      }
      controls.append(type,body);module.append(heading,controls);root.append(module);views.push({role,slot:i,power,picker,fields,module});
    }refresh();
  }
  function refresh(){for(const v of views){const fx=config.slots[v.slot],paired=v.role===1&&[29,30].includes(config.slots[v.slot-1].kind);v.power.textContent=fx.enabled?'On':'Off';v.power.setAttribute('aria-pressed',fx.enabled);v.power.disabled=fx.kind===0||paired;v.picker.render(fx.kind);v.picker.button.disabled=paired;v.module.classList.toggle('fx-paired',paired);v.module.title=paired?'Insert FX 1 uses both insert slots':'';for(const f of v.fields){f.render();const state=f.state(),inactive=paired||state.disabled;f.wrap.hidden=state.hidden;f.wrap.classList.toggle('inactive',inactive);for(const input of f.inputs)input.disabled=inactive;}}}
  render();return {getSlot:i=>config.slots[i],setParameter(i,p,value){config.slots[i].parameters[p]=value;},getConfig:()=>structuredClone(config),setConfig(raw){const next=validateEffects(raw),changed=views.some(v=>config.slots[v.slot].kind!==next.slots[v.slot].kind);config=next;changed?render():refresh();},select:render,refresh};
}
