// Shared hardware controls for the complete firmware-free instrument.
const shortLabels = {
  9:"Morph",10:"Mode",11:"CTRL 1",12:"CTRL 2",15:"Semi",16:"Fine",17:"OSC 1",18:"OSC 2",19:"Noise",7:"Amp level",8:"Amp pan",
  24:"Link",25:"EG1 int",26:"Key track",27:"EG1 int",28:"Key track",31:"Depth",40:"Curve",41:"Vel level",42:"Vel time",43:"Key track",44:"Curve",45:"Vel level",46:"Vel time",47:"Key track",48:"Curve",49:"Vel level",50:"Vel time",51:"Key track",
  52:"Key track",53:"Transpose",54:"Fine",55:"Vibrato",56:"Bend range",57:"Bend RX",58:"Wheel RX",59:"Port time",60:"Port curve",61:"CC65 mode",62:"Voice mode",63:"Retrigger",64:"Priority",65:"Damper",66:"CC64",67:"Unison",68:"Voices",69:"Detune",70:"Spread",71:"Enabled",72:"Channel",
  76:"Key sync",77:"Phase",78:"Tempo sync",79:"Division",80:"Rate offset",84:"Key sync",85:"Phase",86:"Tempo sync",87:"Division",88:"Rate offset",89:"BPM",114:"Level offset",115:"Source gain",116:"MIDI vol RX",117:"MIDI vol",118:"Gain bank",119:"Key low",120:"Key high",121:"Tune ¢",122:"Scale",123:"Root",124:"Scale shift",137:"Bend",138:"Wheel",139:"CC65",140:"Drum mode",141:"Timbre",142:"Instrument",143:"Kit level",144:"Kit pan",145:"Transpose",146:"Trigger",147:"Excl group",148:"Global ch",149:"Amp RX",150:"Expression",151:"Expr RX 0",152:"Gain source",153:"Expr RX 1",154:"WS Type",
};
const rows = [
  [
    {key:"osc1",name:"OSC 1",span:4,columns:2,ids:[0,10,11,12],wave:true},
    {key:"osc2",name:"OSC 2",span:3,columns:2,ids:[13,14,15,16]},
    {key:"mixer",name:"MIXER / AMP",span:3,columns:3,ids:[17,18,19,7,8]},
    {key:"filter1",name:"FILTER 1",span:4,columns:3,ids:[9,1,2,25,26]},
    {key:"filter2",name:"FILTER 2 / ROUTING",span:6,columns:4,ids:[20,21,24,22,23,27,28]},
    {key:"drive",name:"DRIVE / WS",span:4,columns:2,ids:[29,154,31,30],drive:true},
  ],
  [
    {key:"eg1",name:"EG 1 · FILTER",ids:[32,33,34,35,40,41,42,43]},
    {key:"eg2",name:"EG 2 · AMP",ids:[3,4,5,6,44,45,46,47]},
    {key:"eg3",name:"EG 3",ids:[36,37,38,39,48,49,50,51]},
    {key:"lfo1",name:"LFO 1",ids:[73,74,75,76,77,78,79,80]},
    {key:"lfo2",name:"LFO 2",ids:[81,82,83,84,85,86,87,88]},
  ].map(p=>({...p,columns:4})),
  Array.from({length:6},(_,i)=>({key:`patch${i+1}`,name:`PATCH ${i+1}`,ids:[90+i*4,91+i*4,92+i*4,93+i*4],patch:true})),
  [
    {key:"voice",name:"VOICE / PITCH / PORTAMENTO",span:6,columns:6,ids:[62,63,64,65,66,67,68,69,70,53,54,55,56,57,58,59,60,61]},
    {key:"amp",name:"AMPLIFIER",span:3,columns:3,ids:[52,114,115,116,117,118,152]},
    {key:"midi",name:"TIMBRE / MIDI",span:4,columns:3,ids:[71,72,119,120,151,153,148,149]},
    {key:"scale",name:"TUNING / CUSTOM SCALE",span:7,columns:6,ids:[121,122,123,124,...Array.from({length:12},(_,i)=>125+i)]},
    {key:"drums",name:"DRUM KIT",span:4,columns:4,ids:[140,141,142,143,144,145,146,147]},
  ],
];
let openPicker,typedChoice="",typedChoiceTimer;
const popup=document.createElement("div");popup.id="choice-popup";popup.className="choice-popup";popup.setAttribute("popover","auto");popup.setAttribute("role","listbox");document.body.append(popup);
export function isChoosing(){return !!openPicker;}
function closePicker(returnFocus=false){
  const previous=openPicker;openPicker=null;
  if(popup.hidePopover)popup.hidePopover();else popup.hidden=true;
  if(previous){previous.button.setAttribute("aria-expanded","false");if(returnFocus)previous.button.focus();}
}
popup.addEventListener("toggle",event=>{if(event.newState==="closed"&&openPicker){openPicker.button.setAttribute("aria-expanded","false");openPicker=null;}});
document.addEventListener("pointerdown",event=>{if(openPicker&&!popup.contains(event.target)&&!openPicker.button.contains(event.target))closePicker();});
export function makePicker({label,options,value,onChange,short,searchable=false,searchLabel='Search programs'}){
  const button=document.createElement("button");button.type="button";button.className="lcd-choice";button.setAttribute("role","combobox");button.setAttribute("aria-label",label);button.setAttribute("aria-haspopup","listbox");button.setAttribute("aria-expanded","false");button.setAttribute("aria-controls",popup.id);
  const text=document.createElement("span"),arrow=document.createElement("span");arrow.className="choice-arrow";arrow.textContent="⌄";arrow.setAttribute("aria-hidden","true");button.append(text,arrow);
  const picker={button,options,value,onChange,render(next,override){picker.value=next;const option=picker.options.find(o=>o.value===next),name=override??option?.label;text.textContent=short?.(name)??name??String(next);button.title=`${label}: ${name??next}`;}};
  const choose=next=>{picker.render(next);onChange(next);};
  function open(){
    if(openPicker===picker){closePicker();return;}closePicker();typedChoice="";openPicker=picker;popup.replaceChildren();popup.setAttribute("aria-label",label);
    const rect=button.getBoundingClientRect(),width=Math.min(Math.max(rect.width,searchable?260:180),innerWidth-16);
    popup.style.width=`${width}px`;popup.style.left=`${Math.min(Math.max(8,rect.left),innerWidth-width-8)}px`;
    const spaceBelow=innerHeight-rect.bottom-12,spaceAbove=rect.top-12;
    popup.style.top=spaceBelow>=Math.min(220,spaceAbove)?`${rect.bottom+4}px`:"auto";popup.style.bottom=spaceBelow>=Math.min(220,spaceAbove)?"auto":`${innerHeight-rect.top+4}px`;
    popup.style.maxHeight=`${Math.min(420,Math.max(spaceBelow,spaceAbove))}px`;
    let search;
    if(searchable){search=document.createElement('input');search.type='search';search.className='choice-search';search.placeholder=searchLabel;search.setAttribute('aria-label',searchLabel);search.addEventListener('input',()=>{const query=search.value.trim().toLowerCase();for(const item of popup.querySelectorAll('[role=option]'))item.hidden=!item.textContent.toLowerCase().includes(query);});popup.append(search);}
    for(const option of picker.options){const item=document.createElement("button");item.type="button";item.textContent=option.label;item.setAttribute("role","option");item.setAttribute("aria-selected",option.value===picker.value);item.addEventListener("click",()=>{choose(option.value);closePicker(true);});popup.append(item);}
    if(popup.showPopover)popup.showPopover();else popup.hidden=false;button.setAttribute("aria-expanded","true");
    const active=popup.querySelector('[aria-selected="true"]');if(search){popup.scrollTop=0;search.focus({preventScroll:true});}else{active?.focus({preventScroll:true});active?.scrollIntoView({block:"nearest"});}
  }
  button.addEventListener("click",open);
  button.addEventListener("keydown",event=>{
    if(["ArrowUp","ArrowDown","Home","End"].includes(event.key)){event.preventDefault();const i=picker.options.findIndex(o=>o.value===picker.value),next=event.key==="Home"?0:event.key==="End"?picker.options.length-1:Math.max(0,Math.min(picker.options.length-1,i+(event.key==="ArrowDown"?1:-1)));choose(picker.options[next].value);}
  });
  picker.render(value);return picker;
}
popup.addEventListener("keydown",event=>{
  if(!openPicker)return;const items=[...popup.querySelectorAll('[role=option]')].filter(item=>!item.hidden),i=items.indexOf(document.activeElement),searching=event.target.matches('input');
  if(event.key==="Escape"){event.preventDefault();event.stopPropagation();closePicker(true);}
  else if(["ArrowUp","ArrowDown","Home","End"].includes(event.key)&&(!searching||event.key.startsWith('Arrow'))){event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?items.length-1:i<0?(event.key==='ArrowUp'?items.length-1:0):(i+(event.key==="ArrowDown"?1:-1)+items.length)%items.length;items[next]?.focus();}
  else if(searching&&event.key==='Enter'){event.preventDefault();items[0]?.click();}
  else if(event.key==="Tab")closePicker();
  else if(!searching&&event.key.length===1&&!event.metaKey&&!event.ctrlKey&&!event.altKey){event.preventDefault();clearTimeout(typedChoiceTimer);typedChoice+=event.key.toLowerCase();typedChoiceTimer=setTimeout(()=>{typedChoice="";},650);const match=items.find(item=>item.textContent.toLowerCase().startsWith(typedChoice)||item.textContent.toLowerCase().replace(/^(808 |\d{3} · )/,"").startsWith(typedChoice));match?.focus();}

});
export function createPanel({parameters,readValues,setControl,format,disabled,displayValue,nativeValue}){
  const fields=new Map();
  function field(id){
    const p=parameters[id],wrap=document.createElement("div"),label=document.createElement("label");wrap.className="parameter";wrap.dataset.parameter=id;
    const aria=`${p.group} ${id===89?"BPM":p.label}`;label.textContent=p.group.startsWith("Patch")?({Source:"SRC",Destination:"DEST",Intensity:"Intensity","Manual offset":"Offset"}[p.label]??p.label):shortLabels[id]??(id>=125&&id<=136?p.label.replace(" cents"," ¢"):p.label);label.htmlFor=`parameter-${id}`;label.title=aria;wrap.append(label);
    if(p.readonly){const output=document.createElement("output");output.className="lcd-readout";output.id=label.htmlFor;output.setAttribute("aria-label",aria);wrap.append(output);fields.set(id,{wrap,output,inputs:[]});}
    else if(id===29||id===30){
      wrap.classList.add("segment-parameter");const group=document.createElement("div");group.className="segmented";group.setAttribute("role","group");group.setAttribute("aria-label",aria);
      const buttons=p.options.map((name,i)=>{const button=document.createElement("button");button.type="button";button.textContent=id===29&&i===2?"WS":name;button.setAttribute("aria-label",`${id===29?"Drive/WS":"Drive/WS Position"} ${name}`);button.addEventListener("click",()=>setControl(id,i));group.append(button);return button;});
      wrap.append(group);fields.set(id,{wrap,buttons,inputs:buttons});
    }else if(p.options?.join("|")==="Off|On"){
      const button=document.createElement("button"),led=document.createElement("span"),text=document.createElement("span");button.type="button";button.className="switch-control";button.id=label.htmlFor;button.setAttribute("aria-label",aria);led.className="switch-led";led.setAttribute("aria-hidden","true");button.append(led,text);button.addEventListener("click",()=>setControl(id,readValues()[id]?0:1));wrap.append(button);fields.set(id,{wrap,switchButton:button,switchText:text,inputs:[button]});
    }else if(p.options){
      const options=p.options.map((name,i)=>({label:name,value:p.values?.[i]??p.min+i}));
      const picker=makePicker({label:aria,options,value:readValues()[id],onChange:v=>setControl(id,v),short:s=>s?.replace("Waveform","Wave").replace("Ring + Sync","Ring/Sync").replace("WavShape","WS").replace("Timbre","Tmbre").replace("Highest","High").replace("Lowest","Low").replace("SubOSC ","Sub ")});
      picker.button.id=label.htmlFor;wrap.append(picker.button);fields.set(id,{wrap,picker,inputs:[picker.button]});
    }else{
      const button=document.createElement("button"),number=document.createElement("input");button.type="button";button.className="knob";button.setAttribute("role","slider");button.setAttribute("aria-label",`${aria} dial`);button.setAttribute("aria-valuemin",p.min);button.setAttribute("aria-valuemax",p.max);button.innerHTML='<span class="knob-face"></span>';
      number.id=label.htmlFor;number.type="number";number.className="dial-value";number.min=displayValue(p,p.min);number.max=displayValue(p,p.max);number.step=id===89?0.1:1;number.setAttribute("aria-label",aria);
      number.addEventListener("input",()=>{if(number.value!==""&&number.validity.valid)setControl(id,nativeValue(p,Number(number.value)));});
      number.addEventListener("change",()=>{if(number.value!==""&&number.validity.valid)setControl(id,nativeValue(p,Number(number.value)));number.value=displayValue(p,readValues()[id]);});number.addEventListener("blur",()=>{number.value=displayValue(p,readValues()[id]);});
      let drag;button.addEventListener("pointerdown",event=>{if(event.button!==0)return;button.setPointerCapture(event.pointerId);drag={y:event.clientY,value:readValues()[id]};event.preventDefault();});
      button.addEventListener("pointermove",event=>{if(drag)setControl(id,drag.value+(drag.y-event.clientY)*(p.max-p.min)/127*(event.shiftKey?.12:.8));});
      for(const event of ["pointerup","pointercancel","lostpointercapture"])button.addEventListener(event,()=>{drag=null;});
      button.addEventListener("wheel",event=>{event.preventDefault();setControl(id,readValues()[id]-Math.sign(event.deltaY)*(id===89?10:1));},{passive:false});
      button.addEventListener("keydown",event=>{const steps={ArrowUp:1,ArrowRight:1,ArrowDown:-1,ArrowLeft:-1,PageUp:10,PageDown:-10};if(event.key in steps){event.preventDefault();setControl(id,readValues()[id]+steps[event.key]);}else if(["Home","End"].includes(event.key)){event.preventDefault();setControl(id,event.key==="Home"?p.min:p.max);}});button.addEventListener("dblclick",()=>setControl(id,p.default));
      wrap.classList.add("dial-parameter");wrap.append(button,number);fields.set(id,{wrap,button,number,inputs:[button,number]});
    }
    return wrap;
  }
  for(const id of [89,137,138,139,150])document.querySelector("#performance").append(field(id));
  rows.forEach((plans,index)=>{
    const row=document.createElement("div");row.className=`rack-row rack-row-${index}`;document.querySelector("#rack").append(row);
    for(const plan of plans){
      const section=document.createElement("section"),head=document.createElement("div"),title=document.createElement("h2"),body=document.createElement("div");section.className=`module module-${plan.key}`;section.dataset.module=plan.key;section.setAttribute("aria-label",plan.name);section.style.setProperty("--span",plan.span??1);body.style.setProperty("--columns",plan.columns??2);body.className="module-controls";head.className="module-heading";title.textContent=plan.name;head.append(title);
      if(plan.wave){const ns="http://www.w3.org/2000/svg",svg=document.createElementNS(ns,"svg"),path=document.createElementNS(ns,"path");svg.id="wave-display";svg.setAttribute("viewBox","0 0 240 72");svg.setAttribute("role","img");path.id="wave-path";svg.append(path);head.append(svg);}
      if(plan.patch){section.classList.add("patch-module");body.classList.add("patch-body");const route=document.createElement("div");route.className="patch-route";route.append(field(plan.ids[0]),field(plan.ids[1]));body.append(route,field(plan.ids[2]),field(plan.ids[3]));}
      else for(const id of plan.ids)body.append(field(id));
      section.append(head,body);row.append(section);
    }
  });
  return {render(v,overrides={}){
    for(const [id,f] of fields){const p=parameters[id],value=v[id],inactive=disabled(id,v);f.wrap.classList.toggle("inactive",inactive);for(const input of f.inputs)input.disabled=inactive;
      if(f.output)f.output.value=format(id,value);
      if(f.picker)f.picker.render(value,overrides[id]);
      if(f.switchButton){f.switchButton.setAttribute("aria-pressed",!!value);f.switchText.textContent=value?"On":"Off";}
      if(f.buttons)f.buttons.forEach((b,i)=>b.setAttribute("aria-pressed",i===value));
      if(f.button){f.button.style.setProperty("--angle",`${-135+(value-p.min)/(p.max-p.min)*270}deg`);f.button.setAttribute("aria-valuenow",value);f.button.setAttribute("aria-valuetext",format(id,value));if(document.activeElement!==f.number)f.number.value=displayValue(p,value);f.number.title=format(id,value);}
    }
  }};
}
