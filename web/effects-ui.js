import {MASTER_EFFECT_SLOT,TIMBRE_EFFECTS,timbreEffectSlot,effectUsesMaster} from './limits.js';
import {bindMacroTarget} from './macro-gesture.js';
import {makePicker,makeDial} from './panel.js';
import {emptyEffects,validateEffects,defaultEffect,effectDefinitions} from './effects.js';
import {effectChoices,effectValueLabel,effectPropertyLabel,effectFieldState} from './effect-display.js';
export function createEffectsPanel({getSelected,onChange,onChainChange,onChainEffectChange,onError}){
  let config=emptyEffects(),views=[];
  const root=document.querySelector('#effects');
  const attempt=fn=>{try{fn();}catch(error){onError(error);}};
  const chainChanged=t=>{onChainChange(t,structuredClone(config.chains[t]));render();};
  function update(view,fn,rebuild=false,parameter){attempt(()=>{
    const fx=view.read();fn(fx);
    if(view.id)onChainEffectChange(view.timbre,structuredClone(fx),parameter);
    else onChange(view.slot,structuredClone(fx),parameter);
    rebuild?render():refresh();
  });}
  function makeModule(view){
    const {aria,label,master}=view,fx=view.read();
    const module=document.createElement('section'),heading=document.createElement('div'),title=document.createElement('h2'),power=document.createElement('button'),controls=document.createElement('div'),type=document.createElement('div'),body=document.createElement('div');
    module.className='module fx-module';module.setAttribute('aria-label',aria);module.dataset.effectKey=view.key;module.dataset.effectShared=String(view.shared);
    if(view.id){module.dataset.effectId=view.id;module.dataset.effectTimbre=view.timbre;}else module.dataset.effectSlot=view.slot;
    heading.className='module-heading';title.textContent=label;power.type='button';power.className='fx-power';power.setAttribute('aria-label',`${aria} enabled`);power.addEventListener('click',()=>update(view,p=>p.enabled=!p.enabled));heading.append(title,power);
    if(view.id){
      const order=document.createElement('div');order.className='fx-chain-tools';
      for(const [text,verb,direction]of [['↑','Move earlier',-1],['↓','Move later',1],['×','Remove',0]]){
        const button=document.createElement('button');button.type='button';button.textContent=text;button.setAttribute('aria-label',`${verb} ${aria}`);
        const index=config.chains[view.timbre].findIndex(fx=>fx.id===view.id);
        button.disabled=direction<0&&index===0||direction>0&&index===config.chains[view.timbre].length-1;
        button.addEventListener('click',()=>attempt(()=>{const chain=config.chains[view.timbre],index=chain.findIndex(fx=>fx.id===view.id);if(direction){const [fx]=chain.splice(index,1);chain.splice(index+direction,0,fx);}else chain.splice(index,1);chainChanged(view.timbre);}));order.append(button);
      }
      controls.append(order);
    }
    type.className='fx-type';const definitions=effectDefinitions(master),definition=definitions[fx.kind];
    const picker=makePicker({label:`${aria} type`,value:fx.kind,options:definitions.map(d=>({value:d.kind,label:d.name})),searchable:true,searchLabel:`Search ${aria} effects`,onChange:kind=>update(view,p=>Object.assign(p,defaultEffect(kind,master)),true)});type.append(picker.button);body.className='module-controls fx-properties';
    const fields=[];
    if(fx.kind!==0)for(const [index,p]of definition.properties.entries()){
      const duplicate=definition.properties.filter(q=>q.name===p.name).length>1;
      const name=effectPropertyLabel(definition,master,index),controlName=duplicate&&/^(?:[LCR] Delay|Tap[12]Delay|Delay|Duration)$/.test(p.name)?`${name} ${p.max<=16?'sync':'free'}`:name;
      const wrap=document.createElement('div'),text=document.createElement('label');wrap.className='parameter';if(view.id)wrap.dataset.effectId=view.id;else wrap.dataset.effectSlot=view.slot;wrap.dataset.effectParameter=index;text.textContent=name;text.title=controlName;wrap.append(text);
      const target=()=>view.id?{kind:'chain-effect',timbre:view.timbre,id:view.id,effectKind:view.read().kind,parameter:index}:{kind:'effect',slot:view.slot,effectKind:view.read().kind,parameter:index};
      const read=()=>view.read().parameters[index]-p.zero,format=value=>effectValueLabel(definition,master,index,value,view.read().parameters);
      let field;
      if(p.min===0&&p.max===1&&['TempoSync','Key Sync'].includes(p.name)){
        const button=document.createElement('button');button.type='button';button.className='switch-control';button.setAttribute('aria-label',`${aria} ${controlName}`);button.addEventListener('click',()=>update(view,f=>f.parameters[index]=(read()?0:1)+p.zero,false,index));bindMacroTarget(button,target);wrap.append(button);
        field={render(){button.textContent=format(read());button.setAttribute('aria-pressed',read()!==0);},inputs:[button]};
      }else{
        const dial=makeDial({label:`${aria} ${controlName}`,min:p.min,max:p.max,defaultValue:p.default-p.zero,read,onChange:value=>update(view,f=>f.parameters[index]=value+p.zero,false,index),macroTarget:target,format,choices:effectChoices(definition,master,index,fx.parameters)});wrap.append(dial.button,dial.number);
        field={render(){dial.setChoices(effectChoices(definition,master,index,view.read().parameters));dial.render();},inputs:[dial.button,dial.number]};
      }
      field.state=()=>effectFieldState(definition,master,index,view.read().parameters);field.wrap=wrap;fields.push(field);body.append(wrap);
    }
    controls.append(type,body);module.append(heading,controls);root.append(module);views.push({...view,power,picker,fields,module});
  }
  function render(){root.replaceChildren();views=[];const t=getSelected();
    const add=document.createElement('div');add.className='fx-add-row';
    const picker=makePicker({label:`Add effect to timbre ${t+1}`,value:0,options:[{value:0,label:'+ Effect'},...effectDefinitions(true).filter(d=>d.kind).map(d=>({value:d.kind,label:d.name}))],searchable:true,searchLabel:'Search new timbre effects',onChange:kind=>attempt(()=>{if(!kind)return;config.chains[t].push({...defaultEffect(kind,true),id:crypto.randomUUID()});chainChanged(t);})});add.append(picker.button);root.append(add);
    for(let role=0;role<TIMBRE_EFFECTS;role++){
      const slot=timbreEffectSlot(t,role);makeModule({slot,role,shared:false,master:effectUsesMaster(slot),key:String(slot),label:role<2?`INSERT FX ${role+1}`:`TIMBRE FX ${role+1}`,aria:`Timbre ${t+1} ${role<2?'Insert ':''}FX ${role+1}`,read:()=>config.slots[slot]});
    }
    config.chains[t].forEach((fx,index)=>makeModule({id:fx.id,timbre:t,role:TIMBRE_EFFECTS+index,shared:false,master:true,key:`chain:${t}:${fx.id}`,label:`TIMBRE FX ${TIMBRE_EFFECTS+index+1}`,aria:`Timbre ${t+1} FX ${TIMBRE_EFFECTS+index+1}`,read:()=>config.chains[t].find(entry=>entry.id===fx.id)}));
    makeModule({slot:MASTER_EFFECT_SLOT,role:TIMBRE_EFFECTS,shared:true,master:true,key:String(MASTER_EFFECT_SLOT),label:'MASTER FX',aria:'Master FX',read:()=>config.slots[MASTER_EFFECT_SLOT]});

    refresh();
  }
  function refresh(){for(const v of views){const fx=v.read();if(!fx)continue;const paired=!v.id&&!v.shared&&v.role===1&&[29,30].includes(config.slots[v.slot-1].kind);v.power.textContent=fx.enabled?'On':'Off';v.power.setAttribute('aria-pressed',fx.enabled);v.power.disabled=fx.kind===0||paired;v.picker.render(fx.kind);v.picker.button.disabled=paired;v.module.classList.toggle('fx-paired',paired);v.module.title=paired?'Insert FX 1 uses both insert slots':'';for(const f of v.fields){f.render();const state=f.state(),inactive=paired||state.disabled;f.wrap.hidden=state.hidden;f.wrap.classList.toggle('inactive',inactive);for(const input of f.inputs)input.disabled=inactive;}}}
  const signature=state=>JSON.stringify([state.slots.map(fx=>fx.kind),state.chains.map(chain=>chain.map(fx=>[fx.id,fx.kind]))]);
  render();return {getSlot:i=>config.slots[i],getChain:t=>config.chains[t],getChainEffect:(t,id)=>config.chains[t]?.find(fx=>fx.id===id),setParameter(i,p,value){config.slots[i].parameters[p]=value;},setChainParameter(t,id,p,value){config.chains[t].find(fx=>fx.id===id).parameters[p]=value;},getConfig:()=>structuredClone(config),setConfig(raw){const next=validateEffects(raw),changed=signature(config)!==signature(next);config=next;changed?render():refresh();},select:render,refresh};
}
