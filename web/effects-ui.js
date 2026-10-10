import {makePicker,makeDial} from './panel.js';
import {emptyEffects,validateEffects,defaultEffect,effectDefinitions} from './effects.js';
export function createEffectsPanel({getSelected,onChange,onError}){
  let config=emptyEffects(),views=[];
  const root=document.querySelector('#effects');
  const slot=role=>role===2?8:getSelected()*2+role;
  function update(i,fn,rebuild=false){try{fn(config.slots[i]);onChange(i,structuredClone(config.slots[i]));rebuild?render():refresh();}catch(error){onError(error);}}
  function render(){root.replaceChildren();views=[];
    for(let role=0;role<3;role++){
      const i=slot(role),fx=config.slots[i],label=role===2?'MASTER FX':`INSERT FX ${role+1}`,aria=role===2?'Master FX':`Timbre ${getSelected()+1} Insert FX ${role+1}`;
      const module=document.createElement('section'),heading=document.createElement('div'),title=document.createElement('h2'),power=document.createElement('button'),controls=document.createElement('div'),type=document.createElement('div'),body=document.createElement('div');
      module.className='module fx-module';module.setAttribute('aria-label',aria);heading.className='module-heading';title.textContent=label;power.type='button';power.className='fx-power';power.setAttribute('aria-label',`${aria} enabled`);power.addEventListener('click',()=>update(i,p=>p.enabled=!p.enabled));heading.append(title,power);
      type.className='fx-type';const definitions=effectDefinitions(role===2),picker=makePicker({label:`${aria} type`,value:fx.kind,options:definitions.map(d=>({value:d.kind,label:d.name})),searchable:true,searchLabel:`Search ${aria} effects`,onChange:kind=>update(i,()=>config.slots[i]=defaultEffect(kind,role===2),true)});type.append(picker.button);body.className='module-controls fx-properties';body.style.setProperty('--columns',4);
      const fields=[];
      if(fx.kind!==0)for(const [index,p]of definitions[fx.kind].properties.entries()){
        const properties=definitions[fx.kind].properties,duplicate=properties.filter(q=>q.name===p.name).length>1;
        const name=duplicate?(p.name==='TempoSync'?(properties.findIndex(q=>q.name===p.name)===index?'Time sync':'LFO sync'):`${p.name} ${p.max<=16?'sync':'free'}`):p.name;
        const wrap=document.createElement('div'),text=document.createElement('label');wrap.className='parameter';text.textContent=name;text.title=name;wrap.append(text);
        const read=()=>config.slots[i].parameters[index]-p.zero;
        let sync=-1,invert=false;
        if(duplicate&&p.name!=='TempoSync'){sync=properties.findIndex(q=>q.name==='TempoSync');invert=p.max>16;}
        if(p.name==='LFO Freq'&&properties[index-1]?.name==='TempoSync'){sync=index-1;invert=true;}
        if(p.name==='Sync Note')sync=properties.findLastIndex((q,j)=>j<index&&q.name==='TempoSync');
        const disabled=()=>sync>=0&&(invert?config.slots[i].parameters[sync]!==0:config.slots[i].parameters[sync]===0);
        let field;
        if(p.min===0&&p.max===1&&['TempoSync','Key Sync'].includes(p.name)){const button=document.createElement('button');button.type='button';button.className='switch-control';button.setAttribute('aria-label',`${aria} ${name}`);button.addEventListener('click',()=>update(i,f=>f.parameters[index]=(read()?0:1)+p.zero));wrap.append(button);field={render(){button.textContent=read()?'On':'Off';button.setAttribute('aria-pressed',read()!==0);},inputs:[button]};}
        else {const dial=makeDial({label:`${aria} ${name}`,min:p.min,max:p.max,defaultValue:p.default-p.zero,read,onChange:value=>update(i,f=>f.parameters[index]=value+p.zero)});wrap.append(dial.button,dial.number);field={render:()=>dial.render(),inputs:[dial.button,dial.number]};}
        field.disabled=disabled;field.wrap=wrap;
        fields.push(field);body.append(wrap);
      }
      controls.append(type,body);module.append(heading,controls);root.append(module);views.push({role,slot:i,power,picker,fields,module});
    }refresh();
  }
  function refresh(){for(const v of views){const fx=config.slots[v.slot],paired=v.role===1&&[29,30].includes(config.slots[v.slot-1].kind);v.power.textContent=fx.enabled?'On':'Off';v.power.setAttribute('aria-pressed',fx.enabled);v.power.disabled=fx.kind===0||paired;v.picker.render(fx.kind);v.picker.button.disabled=paired;v.module.classList.toggle('fx-paired',paired);v.module.title=paired?'Insert FX 1 uses both insert slots':'';for(const f of v.fields){f.render();const inactive=paired||f.disabled();f.wrap.classList.toggle('inactive',inactive);for(const input of f.inputs)input.disabled=inactive;}}}
  render();return {getConfig:()=>structuredClone(config),setConfig(raw){const next=validateEffects(raw),changed=views.some(v=>config.slots[v.slot].kind!==next.slots[v.slot].kind);config=next;changed?render():refresh();},select:render,refresh};
}
