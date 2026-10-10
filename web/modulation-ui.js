import {makeDial,makePicker} from './panel.js';
import {emptyModulation,validateModulation,newModulationLane,modulationKey,modulationRange,modulationBarChoices,MOD_LANES,MOD_STEPS,MOD_RESOLUTIONS,MOD_DIRECTIONS,MOD_RUN_MODES} from './modulation.js';

export function createModulationEditor({panels,getTargets,onChange,onError,onCounts}){
  let config=emptyModulation(),status={positions:[]};
  const views=panels.map(()=>({records:[]}));
  const notify=t=>{onChange(validateModulation(config),t);onCounts(config.tracks.map(lanes=>lanes.length));};
  const attempt=fn=>{try{fn();}catch(error){onError(error);}};
  function field(label,control,className=''){
    const wrap=document.createElement('label'),text=document.createElement('span');wrap.className='mod-field '+className;text.textContent=label;wrap.append(text,control);return wrap;
  }
  function targetOptions(t,lane,shared=getTargets(t)){
    const options=[{value:'none',label:'None',target:null},...shared];
    if(lane.target&&!options.some(o=>o.value===modulationKey(lane.target)))options.push({value:modulationKey(lane.target),label:'Unavailable parameter',target:lane.target});
    return options;
  }
  function refreshTargets(){
    for(let t=0;t<panels.length;t++)if(!panels[t].hidden){const shared=getTargets(t);for(const view of views[t].records){view.target.options=targetOptions(t,view.lane,shared);view.target.render(view.lane.target?modulationKey(view.lane.target):'none');}}
  }
  function paint(view){
    const lane=view.lane;view.power.textContent=lane.enabled?'On':'Off';view.power.setAttribute('aria-pressed',lane.enabled);view.root.classList.toggle('muted',!lane.enabled);
    view.steps.render(lane.length);view.bars.options=modulationBarChoices(lane);view.bars.render(lane.length);view.resolution.render(lane.resolution);view.amount.render();
    view.motionButtons.forEach(b=>b.setAttribute('aria-pressed',b.textContent===lane.motion));view.direction.render(lane.direction);view.runMode.render(lane.runMode);
    view.bank=Math.min(view.bank,Math.floor((lane.length-1)/16));view.block.options=Array.from({length:Math.ceil(lane.length/16)},(_,i)=>({value:i,label:(i*16+1)+'–'+Math.min(lane.length,(i+1)*16)}));view.block.render(view.bank);view.blockWrap.hidden=lane.length<=16;
    const range=modulationRange(lane.target),position=status.positions[view.t]?.[view.index];
    view.grid.style.setProperty('--mod-columns',Math.min(4,lane.length-view.bank*16));
    for(const [i,step]of view.knobs.entries()){
      const index=view.bank*16+i,active=index<lane.length;step.label.textContent=String(index+1).padStart(2,'0');step.dial.button.setAttribute('aria-label',view.aria+' step '+(index+1)+' dial');step.dial.number.setAttribute('aria-label',view.aria+' step '+(index+1));
      step.dial.render();step.dial.button.disabled=step.dial.number.disabled=!active;step.root.classList.toggle('outside-loop',!active);step.root.classList.toggle('current',position===index);
      step.dial.number.min=-range;step.dial.number.max=range;
    }
  }
  function render(t){
    if(panels[t].hidden)return;
    const panel=panels[t],shared=getTargets(t),heading=document.createElement('div'),title=document.createElement('h3'),add=document.createElement('button');panel.replaceChildren();views[t].records=[];
    heading.className='modulation-heading';title.textContent='MOD SEQUENCER';add.type='button';add.textContent='+ Mod';add.className='mod-add';add.setAttribute('aria-label','Add mod sequence timbre '+(t+1));add.disabled=config.tracks[t].length===MOD_LANES;add.title=config.tracks[t].length+' / '+MOD_LANES;
    add.addEventListener('click',()=>attempt(()=>{config.tracks[t].push(newModulationLane(crypto.randomUUID()));notify(t);render(t);}));heading.append(title,add);panel.append(heading);
    config.tracks[t].forEach((lane,index)=>{
      const root=document.createElement('section'),tools=document.createElement('div'),grid=document.createElement('div'),name=document.createElement('h4'),power=document.createElement('button'),remove=document.createElement('button');
      const aria='Timbre '+(t+1)+' Mod '+(index+1);root.className='modulation-sequence';root.setAttribute('aria-label',aria);tools.className='mod-tools';grid.className='mod-step-grid';name.textContent='MOD '+(index+1);power.type=remove.type='button';power.className='mod-power';power.setAttribute('aria-label',aria+' enabled');remove.className='mod-remove';remove.textContent='×';remove.setAttribute('aria-label','Remove '+aria);
      power.addEventListener('click',()=>{lane.enabled=!lane.enabled;notify(t);paint(view);});remove.addEventListener('click',()=>{config.tracks[t].splice(index,1);notify(t);render(t);});tools.append(name,power);
      const target=makePicker({label:aria+' parameter',options:targetOptions(t,lane,shared),value:lane.target?modulationKey(lane.target):'none',searchable:true,searchLabel:'Search '+aria+' parameters',onChange:value=>attempt(()=>{lane.target=target.options.find(o=>o.value===value).target;const range=modulationRange(lane.target);lane.values=lane.values.map(v=>Math.max(-range,Math.min(range,v)));notify(t);render(t);})});
      tools.append(field('Parameter',target.button,'mod-target'));
      const amount=makeDial({label:aria+' intensity',min:-100,max:100,defaultValue:100,read:()=>lane.amount,onChange:value=>{lane.amount=value;amount.render();notify(t);},format:v=>(v>0?'+':'')+v+'%'}),amountWrap=document.createElement('div');amountWrap.className='mod-amount';const amountLabel=document.createElement('span');amountLabel.textContent='Intensity';amountWrap.append(amountLabel,amount.button,amount.number);tools.append(amountWrap);
      const changeLength=value=>{lane.length=value;notify(t);paint(view);};
      const steps=makePicker({label:aria+' steps',options:Array.from({length:MOD_STEPS},(_,i)=>({value:i+1,label:String(i+1)})),value:lane.length,onChange:changeLength});tools.append(field('Steps',steps.button));
      const bars=makePicker({label:aria+' bars',options:modulationBarChoices(lane),value:lane.length,onChange:changeLength});tools.append(field('Bars',bars.button));
      const resolution=makePicker({label:aria+' resolution',options:MOD_RESOLUTIONS.map(value=>({value,label:value})),value:lane.resolution,onChange:value=>{lane.resolution=value;notify(t);paint(view);}});tools.append(field('Resolution',resolution.button));
      const motion=document.createElement('div');motion.className='mod-motion segmented';motion.setAttribute('role','group');motion.setAttribute('aria-label',aria+' motion');const motionButtons=['Step','Slide'].map(value=>{const b=document.createElement('button');b.type='button';b.textContent=value;b.setAttribute('aria-label',aria+' '+value);b.addEventListener('click',()=>{lane.motion=value;notify(t);paint(view);});motion.append(b);return b;});tools.append(field('Motion',motion));
      const direction=makePicker({label:aria+' direction',options:MOD_DIRECTIONS.map(value=>({value,label:value})),value:lane.direction,onChange:value=>{lane.direction=value;notify(t);paint(view);}});tools.append(field('SeqType',direction.button));
      const runMode=makePicker({label:aria+' run mode',options:MOD_RUN_MODES.map(value=>({value,label:value})),value:lane.runMode,onChange:value=>{lane.runMode=value;notify(t);paint(view);}});tools.append(field('RunMode',runMode.button));
      const view={root,power,grid,lane,t,index,aria,bank:0,target,amount,steps,bars,resolution,motionButtons,direction,runMode,knobs:[]};
      const block=makePicker({label:aria+' block',options:[],value:0,onChange:value=>{view.bank=value;paint(view);}}),blockWrap=field('Steps',block.button,'mod-block');tools.append(blockWrap,remove);Object.assign(view,{block,blockWrap});
      for(let i=0;i<16;i++){
        const step=document.createElement('div'),label=document.createElement('label'),range=modulationRange(lane.target);step.className='mod-step';
        const dial=makeDial({label:aria+' step '+(i+1),min:-range,max:range,defaultValue:0,read:()=>lane.values[view.bank*16+i],onChange:value=>{lane.values[view.bank*16+i]=value;dial.render();notify(t);},format:v=>(v>0?'+':'')+v+(lane.target?.kind==='macro'?'%':range===24?' st':'')});
        step.append(label,dial.button,dial.number);grid.append(step);view.knobs.push({root:step,label,dial});
      }
      root.append(tools,grid);panel.append(root);views[t].records.push(view);paint(view);
    });
  }
  panels.forEach((panel,t)=>panel.addEventListener('modulation-open',()=>render(t)));
  return {
    getConfig:()=>validateModulation(config),
    setConfig(raw){config=validateModulation(raw);views.forEach(view=>view.records=[]);for(let t=0;t<panels.length;t++)render(t);onCounts(config.tracks.map(lanes=>lanes.length));},
    refreshTargets,
    setStatus(next){status=next;for(const view of views)for(const record of view.records)for(const [i,step]of record.knobs.entries())step.root.classList.toggle('current',next.positions[record.t]?.[record.index]===record.bank*16+i);},
  };
}
