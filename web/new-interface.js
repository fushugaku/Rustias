import {createSequenceWorkspace} from './sequence-workspace.js';
import {MASTER_EFFECT_SLOT,timbreEffectSlot} from './limits.js';
import {makePicker} from './panel.js';

const $=selector=>document.querySelector(selector);
function element(tag,className,text){const el=document.createElement(tag);if(className)el.className=className;if(text!=null)el.textContent=text;return el;}
function button(text,label=text){const el=element('button','nw-button',text);el.type='button';el.setAttribute('aria-label',label);return el;}

export function setupInterfaceNavigation(){
  const fresh=new URLSearchParams(location.search).get('interface')==='new';
  document.documentElement.classList.toggle('new-interface',fresh);
  if(fresh)document.title='Rustias · New interface';
  const brand=$('.brand'),identity=element('div','interface-identity'),nav=element('nav','interface-navigation');nav.setAttribute('aria-label','Interface');
  for(const [name,value]of [['Classic',null],['New interface','new']]){
    const link=element('a','',name),url=new URL(location.href);value?url.searchParams.set('interface',value):url.searchParams.delete('interface');link.href=url.href;
    if(fresh===(value==='new'))link.setAttribute('aria-current','page');else{link.target='_blank';link.rel='noopener';}nav.append(link);
  }
  brand.replaceWith(identity);identity.append(brand,nav);return fresh;
}

function popover(label,content){
  const trigger=button(label),popup=element('div','nw-popover');popup.setAttribute('popover','auto');popup.id='new-'+label.toLowerCase().replaceAll(' ','-');popup.setAttribute('role','dialog');popup.setAttribute('aria-label',label);trigger.setAttribute('aria-haspopup','dialog');trigger.setAttribute('aria-controls',popup.id);trigger.setAttribute('aria-expanded','false');
  const head=element('div','nw-popover-heading'),close=button('×','Close '+label);head.append(element('h2','',label),close);popup.append(head,...content);document.body.append(popup);
  const hide=()=>{popup.hidePopover();trigger.setAttribute('aria-expanded','false');};close.addEventListener('click',hide);
  trigger.addEventListener('click',()=>{if(trigger.getAttribute('aria-expanded')==='true'){hide();return;}const r=trigger.getBoundingClientRect(),width=Math.min(360,innerWidth-24);popup.style.width=width+'px';popup.style.left=Math.max(12,Math.min(innerWidth-width-12,r.right-width))+'px';popup.style.top=Math.min(r.bottom+8,innerHeight-100)+'px';popup.style.maxHeight=Math.max(100,innerHeight-r.bottom-20)+'px';popup.showPopover();trigger.setAttribute('aria-expanded','true');});
  popup.addEventListener('toggle',event=>{if(event.newState==='closed')trigger.setAttribute('aria-expanded','false');});
  popup.addEventListener('keydown',event=>{if(event.key==='Escape'){event.stopPropagation();hide();trigger.focus();}});return {trigger,popup,hide};
}

export function prepareNewInterface(){
  let context={selected:0,timbreCount:4,values:[]},api,sequenceWorkspace;
  const layoutKey='rustias.new-interface.layout.v1';let saved={};try{saved=JSON.parse(localStorage.getItem(layoutKey))??{};}catch{}
  const small=matchMedia('(max-width:800px)').matches;
  let narrow=small;
  const prefs={height:Number.isFinite(saved.height)?saved.height:220,macrosDesktop:typeof saved.macrosDesktop==='boolean'?saved.macrosDesktop:typeof saved.macros==='boolean'?saved.macros:true,macrosMobile:!!saved.macrosMobile,keyboard:!!saved.keyboard,mode:['steps','piano','samples'].includes(saved.mode)?saved.mode:'steps'};
  prefs.macros=small?prefs.macrosMobile:prefs.macrosDesktop;
  const persist=()=>{try{localStorage.setItem(layoutKey,JSON.stringify(prefs));}catch{}};
  const instrument=$('.instrument'),toolbar=$('.toolbar'),top=element('div','nw-topbar'),transport=element('div','nw-transport');
  const files=popover('Files',[$('#load-program'),$('#save-program'),$('#rdl-details'),$('#program-file')]);
  files.popup.addEventListener('click',event=>{if(event.target.closest('#load-program,#save-program,#rdl-details'))files.hide();});
  const performance=$('#performance'),tempo=performance.querySelector('[data-parameter="89"]');
  const settings=popover('Performance',[performance,$('#midi'),$('#power'),$('#panic')]);
  settings.trigger.textContent='Controls';
  const tools=element('div','nw-header-tools');tools.append($('#build'),files.trigger,settings.trigger,$('#fullscreen'));
  top.append($('.interface-identity'),$('.program-select'),$('.patch-save'),$('.sound-select'),tools);
  const macroToggle=button('Macros'),keyboardToggle=button('Keyboard'),toggles=element('div','nw-view-tools');toggles.append(macroToggle,keyboardToggle);
  transport.append($('.timbre-bank'),tempo,$('#seq-play'),$('#seq-reset'),$('#record-toggle'),$('#record-time'),toggles);toolbar.replaceChildren(top,transport);
  const mobileTimbre=element('div','nw-mobile-timbre'),timbrePicker=makePicker({label:'Editing timbre',options:Array.from({length:4},(_,i)=>({value:i,label:'T'+(i+1)})),value:0,onChange:value=>$(`.timbres [data-timbre="${value}"]`).click()});mobileTimbre.append(timbrePicker.button);$('.timbre-bank').prepend(mobileTimbre);

  const macros=$('.macros-rack'),stage=element('div','nw-stage'),sound=element('section','nw-sound'),stageHead=element('div','nw-stage-heading'),title=element('h2','nw-context','Timbre 01'),jumps=element('nav','nw-section-jumps');jumps.setAttribute('aria-label','Sound sections');
  for(const [label,key]of [['OSC','osc1'],['Filters','filter1'],['Mod','eg1'],['Patch','patch1'],['Voice','voice'],['Drums','drums']]){
    const jump=button(label);jump.addEventListener('click',()=>{if($('#rack').hidden)$('#build').click();const rack=$('#rack'),module=rack.querySelector('.module-'+key);rack.scrollTo({top:module.getBoundingClientRect().top-rack.getBoundingClientRect().top+rack.scrollTop-8,behavior:'smooth'});});jumps.append(jump);
  }
  stageHead.append(title,jumps);sound.append(stageHead,element('div','nw-signal-flow'),$('#rack'),$('#circuit'),$('#recordings-library'));
  const fx=element('aside','nw-fx'),fxHead=element('div','nw-stage-heading'),fxTitle=element('h2','','Effects'),fxFold=button('−','Collapse effects');fxHead.append(fxTitle,fxFold);fx.append(fxHead,$('#effects'));stage.append(sound,fx);instrument.insertBefore(stage,macros.nextSibling);
  fxFold.addEventListener('click',()=>{fx.classList.toggle('folded');fxFold.textContent=fx.classList.contains('folded')?'+':'−';fxFold.setAttribute('aria-label',fx.classList.contains('folded')?'Expand effects':'Collapse effects');fxFold.setAttribute('aria-expanded',!fx.classList.contains('folded'));});
  if(small)fx.classList.add('folded');fxFold.textContent=small?'+':'−';fxFold.setAttribute('aria-expanded',!small);
  const rack=$('#rack'),groups=[['Sound',['osc1','osc2','mixer','filter1','filter2','drive','amp']],['Modulation',['eg1','eg2','eg3','lfo1','lfo2']],['Virtual patch',Array.from({length:8},(_,i)=>'patch'+(i+1))],['Voice & tuning',['voice','midi','scale']],['Drum kit',['drums']]];
  groups.forEach(([label,keys],group)=>{const divider=element('h2','nw-rack-divider',label);divider.style.order=group*100;rack.append(divider);keys.forEach((key,index)=>rack.querySelector('.module-'+key).style.order=group*100+index+1);});
  for(const id of [1,2,3,4,5,6,7,8,11,12,17,18,19,31])rack.querySelector(`[data-parameter="${id}"]`)?.classList.add('nw-primary');

  const dock=element('section','nw-dock');dock.id='new-sequence-dock';dock.setAttribute('aria-label','Sequencer workspace');
  const grip=element('div','nw-dock-grip');grip.setAttribute('role','separator');grip.setAttribute('aria-label','Resize sequencer');grip.setAttribute('aria-orientation','horizontal');grip.tabIndex=0;
  dock.append(grip,$('.sequencer'));const inspector=element('aside','nw-inspector');inspector.id='new-step-inspector';inspector.hidden=true;inspector.append($('#step-editor'));dock.append(inspector);
  const footer=$('.performance-footer'),keybed=$('.keybed'),footerTools=element('div','nw-footer-tools');footerTools.append($('#record-folder'),$('#recordings-toggle'),$('#record-message'));$('#recorder').hidden=true;footer.append(footerTools);
  keybed.hidden=!prefs.keyboard;macros.hidden=!prefs.macros;macroToggle.setAttribute('aria-pressed',prefs.macros);keyboardToggle.setAttribute('aria-pressed',prefs.keyboard);
  macroToggle.addEventListener('click',()=>{prefs.macros=!prefs.macros;prefs[narrow?'macrosMobile':'macrosDesktop']=prefs.macros;macros.hidden=!prefs.macros;macroToggle.setAttribute('aria-pressed',prefs.macros);setHeight(prefs.height);persist();});
  keyboardToggle.addEventListener('click',()=>{prefs.keyboard=!prefs.keyboard;keybed.hidden=!prefs.keyboard;keyboardToggle.setAttribute('aria-pressed',prefs.keyboard);if(!prefs.keyboard)api?.releaseAll();setHeight(prefs.height);persist();});
  instrument.replaceChildren(toolbar,$('#error'),macros,stage,dock,footer);
  function placeResponsiveTools(){const mobile=matchMedia('(max-width:800px)').matches;if(mobile!==narrow){narrow=mobile;prefs.macros=mobile?prefs.macrosMobile:prefs.macrosDesktop;macros.hidden=!prefs.macros;macroToggle.setAttribute('aria-pressed',prefs.macros);}if(mobile){stageHead.append(toggles);settings.popup.append($('#seq-reset'),$('#record-folder'));footerTools.prepend($('#record-time'));}else{transport.append(toggles);transport.insertBefore($('#seq-reset'),$('#record-toggle'));transport.insertBefore($('#record-time'),toggles);footerTools.prepend($('#record-folder'));}}
  placeResponsiveTools();
  function maxHeight(){return Math.max(160,innerHeight-toolbar.offsetHeight-(macros.hidden?0:macros.offsetHeight)-footer.offsetHeight-160);}
  function setHeight(value){prefs.height=Math.round(Math.max(160,value));const height=Math.min(prefs.height,maxHeight());instrument.style.setProperty('--sequence-height',height+'px');instrument.style.setProperty('--toolbar-height',toolbar.offsetHeight+'px');grip.setAttribute('aria-valuemin','160');grip.setAttribute('aria-valuemax',maxHeight());grip.setAttribute('aria-valuenow',height);}
  let resizing;grip.addEventListener('pointerdown',event=>{if(event.button!==0)return;resizing={y:event.clientY,height:dock.offsetHeight};grip.setPointerCapture(event.pointerId);event.preventDefault();});grip.addEventListener('pointermove',event=>{if(resizing)setHeight(resizing.height+resizing.y-event.clientY);});
  for(const event of ['pointerup','pointercancel','lostpointercapture'])grip.addEventListener(event,()=>{if(resizing){resizing=null;persist();}});
  grip.addEventListener('keydown',event=>{if(['ArrowUp','ArrowDown','Home','End'].includes(event.key)){event.preventDefault();setHeight(event.key==='Home'?160:event.key==='End'?maxHeight():Number(grip.getAttribute('aria-valuenow'))+(event.key==='ArrowUp'?32:-32));persist();}});
  window.addEventListener('resize',()=>{placeResponsiveTools();setHeight(prefs.height);});setHeight(prefs.height);
  const editorObserver=new MutationObserver(()=>{const open=$('#step-editor').open;inspector.hidden=!open;dock.classList.toggle('has-inspector',open);if(narrow)(open?inspector:dock).scrollIntoView({block:'start',behavior:'smooth'});});editorObserver.observe($('#step-editor'),{attributes:true,attributeFilter:['open']});

  const expanded=new Map();
  function refreshEffects(){if(!api)return;const modules=[...$('#effects').children];modules.forEach((module,role)=>{
    const slot=role===4?MASTER_EFFECT_SLOT:timbreEffectSlot(context.selected,role),effect=api.fxUI.getSlot(slot),key=String(slot),empty=effect.kind===0;module.classList.toggle('fx-empty',empty);module.classList.toggle('fx-shared',role===4);module.dataset.effectSlot=slot;
    let toggle=module.querySelector('.nw-fx-expand');if(!toggle){toggle=button('−');toggle.className='nw-fx-expand';module.querySelector('.module-heading').append(toggle);toggle.addEventListener('click',()=>{const index=module.dataset.effectSlot;expanded.set(index,toggle.getAttribute('aria-expanded')!=='true');refreshEffects();});}
    const open=expanded.get(key)??effect.enabled;toggle.hidden=empty;const text=open?'−':'+';if(toggle.textContent!==text)toggle.textContent=text;toggle.setAttribute('aria-expanded',open);toggle.setAttribute('aria-label',`${open?'Collapse':'Expand'} ${module.getAttribute('aria-label')} parameters`);module.classList.toggle('fx-collapsed',!open);
  });}
  function refreshMacros(){if(!api)return;const knobs=api.macroUI.getConfig().knobs;$('#macros').querySelectorAll('.macro-control').forEach((field,index)=>{let summary=field.querySelector('.nw-macro-targets');if(!summary){summary=element('div','nw-macro-targets');field.append(summary);}const labels=knobs[index].bindings.map(binding=>api.resolveMacro(binding.target)?.label??'Unavailable parameter');summary.textContent=labels.length?labels.slice(0,2).map(label=>label.replaceAll(' · ',' ').replace('Amplifier','Amp').replace('Filter ','F')).join(' · ')+(labels.length>2?` +${labels.length-2}`:''):'';summary.title=labels.join('\n');});}
  return {
    refreshContext(next){context=next;title.textContent=$('#edit-context').textContent;fxTitle.textContent=`T${next.selected+1} · Effects`;
      timbrePicker.options=Array.from({length:next.timbreCount},(_,i)=>({value:i,label:'T'+(i+1)}));timbrePicker.render(next.selected);
      const v=next.values,custom=api?.circuitUI.isRouting(next.selected),source=$('#parameter-0').disabled?'Sample':'OSC 1 + OSC 2 + Noise';
      const filter=['Filter 1','Filters · Serial','Filters · Parallel','Filters · Individual'][v[20]??0],drive=v[29]?'Drive / WS':null;
      const chain=v[30]===0?[source,'Mixer',drive,filter,'Amp']:[source,'Mixer',filter,drive,'Amp'];$('.nw-signal-flow').textContent=custom?'Custom routing':chain.filter(Boolean).join(' → ');
      sequenceWorkspace?.setContext(next.selected,next.timbreCount);refreshEffects();refreshMacros();
    },
    connect(next){api=next;sequenceWorkspace=createSequenceWorkspace({sequenceUI:api.sequenceUI,dock,prefs,onPreference:()=>{if(prefs.mode!=='steps')setHeight(Math.max(prefs.height,320));persist();}});
      const fxObserver=new MutationObserver(refreshEffects);fxObserver.observe($('#effects'),{childList:true,subtree:true,characterData:true,attributes:true,attributeFilter:['aria-pressed']});
      refreshEffects();refreshMacros();setHeight(prefs.height);
    }
  };
}
