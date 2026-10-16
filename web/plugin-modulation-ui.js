import {MAX_TIMBRES,INITIAL_TIMBRES} from './limits.js';
import {emptySequence,validateSequence} from './sequence.js';

// Native plugins use DAW notes. Keep imported browser patterns as inert program
// data so sharing a program back to the browser does not discard its music.
export function createPluginModulationRack({inline,onSelectTimbre}){
  let sequence=emptySequence(),count=INITIAL_TIMBRES,selected=0;
  const root=document.querySelector('.sequencer'),heading=document.createElement('div'),title=document.createElement('h2'),tabs=document.createElement('div'),body=document.createElement('div');
  root.classList.add('plugin-modulation-rack');root.setAttribute('aria-label','Modulation sequencers');heading.className='sequence-heading';title.textContent='Mod sequencer';tabs.className='plugin-modulation-tabs';tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','Modulation timbre');body.className='plugin-modulation-panels';heading.append(title,tabs);root.replaceChildren(heading,body);
  const panels=[],buttons=[];
  function render(){
    buttons.forEach((button,t)=>{button.hidden=t>=count;button.setAttribute('aria-selected',t===selected);button.tabIndex=t===selected?0:-1;panels[t].hidden=t!==selected;});
    panels[selected].dispatchEvent(new Event('modulation-open'));
  }
  for(let t=0;t<MAX_TIMBRES;t++){
    const button=document.createElement('button'),panel=document.createElement('div');button.type='button';button.textContent='T'+(t+1);button.id='plugin-mod-tab-'+t;button.setAttribute('role','tab');button.setAttribute('aria-label','Mod sequences timbre '+(t+1));button.setAttribute('aria-controls','modulation-timbre-'+t);panel.id='modulation-timbre-'+t;panel.className='modulation-panel';panel.setAttribute('role','tabpanel');panel.setAttribute('aria-labelledby',button.id);panel.hidden=t!==0;
    button.addEventListener('click',()=>{selected=t;render();onSelectTimbre(t);});
    button.addEventListener('keydown',event=>{const next=event.key==='ArrowRight'?(t+1)%count:event.key==='ArrowLeft'?(t+count-1)%count:event.key==='Home'?0:event.key==='End'?count-1:null;if(next!=null){event.preventDefault();buttons[next].click();buttons[next].focus();}});
    buttons.push(button);panels.push(panel);tabs.append(button);body.append(panel);
  }
  render();
  return {
    nativeModulation:true,modulationPanels:panels,
    getConfig:()=>validateSequence(sequence),setConfig:value=>{sequence=validateSequence(value);},
    setTimbreCount(value){count=value;selected=Math.min(selected,count-1);render();},
    setContext(timbre,timbreCount){if(selected===timbre&&count===timbreCount)return;selected=timbre;count=timbreCount;render();},
    setModulationCounts(counts){buttons.forEach((button,t)=>{button.classList.toggle('has-modulation',counts[t]>0);button.title='Mod sequences '+counts[t]+' / 6';});},
    setReady(){},setStatus(){},setSamples(){},setDrumKit(){},
  };
}
