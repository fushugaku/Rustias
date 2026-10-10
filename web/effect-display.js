import {effectDisplayTables} from './effects.js';
// RADIAS Owner's Manual, Effect guide pp.123–147. Encoded indices stay intact.
export const DELAY_NOTES=['1/64','1/32','1/24','1/16','1/12','1/8','1/6','3/16','1/4','1/3','3/8','1/2','3/4','1/1'];
export const LFO_NOTES=['8/1','4/1','2/1','1/1','3/4','1/2','3/8','1/3','1/4','3/16','1/6','1/8','1/12','1/16','1/24','1/32','1/64'];
const controlSources=['Off','Velocity','P.Bend','M.Wheel','F.Pedal','F.SW','Damper','E.F','MIDI1','MIDI2','MIDI3','MIDI4','MIDI5'];
const cabinet=['TWD 1x8','TWD 1x12','TWD 4x10','BLK 2x10','BLK 2x12','AC15','AC30','AD412','UK H30','UK T75','US V30'];
const signed=(v,precision=0)=>`${v>0?'+':''}${v.toFixed(precision)}`;
const hz=v=>v>=1000?`${(v/1000).toFixed(2).replace(/0$/,'')} kHz`:`${Math.round(v)} Hz`;
const lfoHz=v=>`${v.toFixed(v<10?2:1)} Hz`;
const pad=v=>String(v).padStart(3,'0');
export function effectPropertyLabel(def,master,index){
  const p=def.properties[index],matches=def.properties.map((p,i)=>p.name===def.properties[index].name?i:-1).filter(i=>i>=0);
  if(p.name==='B2 Type'&&master&&def.kind===6)return 'B4 Type';
  if(p.name==='CtrlMode'&&def.kind===29)return index===3?'Mode Ctrl Mode':'Switch Ctrl Mode';
  if(p.name==='CtrlSrc'&&def.kind===29)return ({2:'Mode Ctrl Src',6:'Switch Ctrl Src',9:'Speed Ctrl Src'})[index]??p.name;
  if(p.name==='TempoSync'&&matches.length>1)return index===matches[0]?'Time sync':'LFO sync';
  return p.name;
}
export function effectValueLabel(def,master,index,value,parameters){
  const p=def.properties[index],name=p.name,v=value,t=effectDisplayTables(),kind=def.kind;
  if(name==='Dry/Wet')return v===0?'Dry':v===100?'Wet':`${100-v}:${v}`;
  if(name==='TempoSync'||name==='PreLPF')return ['Off','On'][v];
  if(name==='Key Sync')return ['Off','Timbre'][v];
  if(name==='Sync Note')return LFO_NOTES[v];
  if(/^(?:[LCR] Delay|Tap[12]Delay|Delay|Duration)$/.test(name)&&p.max===13)return DELAY_NOTES[v];
  if(name==='Env Sel')return ['LR Mix','LR Indv.'][v];
  if(name==='Filter')return ['LPF24','LPF18','LPF12','HPF12','BPF12'][v];
  if(name==='Wah Type')return ['Y-CRY','RM-A','RM-B','J-CRY','VOX','M-VOX'][v];
  if(name==='LFO Wave')return ['Saw','Squ','Tri','Sin','S&H'][v];
  if(name==='CabiType')return cabinet[v];
  if(name==='Mod Src')return (kind===4?['LFO','Ctrl']:['Auto','LFO','Ctrl'])[v];
  if(name==='CtrlSrc'||name==='Ctrl Src')return controlSources[v]??`MIDI${v-7}`;
  if(name==='B1 Type')return ['Peaking','Shelv Lo'][v];
  if(name==='B2 Type')return ['Peaking','Shelv Hi'][v];
  if(name==='Phase')return (kind===9?['Normal','Inverted']:['+','−'])[v];
  if(name==='Type')return (kind===11?(master?['Hall','SmoothHall','WetPlate','DryPlate','Room','BritRoom']:['Hall','WetPlate','Room']):kind===12?['Sharp','Loose','Modulation','Reverse']:kind===14?['Stereo','Cross']:kind===22?['Flanger','Comb']:kind===23?['Blue','U_VB']:[])[v]??String(v);
  if(name==='OSC Mode')return ['Fixed','Note'][v];
  if(name==='OSC Wave')return ['Saw','Tri','Sin'][v];
  if(name==='FB Pos')return ['Pre','Post'][v];
  if(name==='Mode')return ['Slow','Medium','Fast'][v];
  if(name==='Mode Sw')return ['Rotate','Stop'][v];
  if(name==='CtrlMode')return ['Toggle','Moment'][v];
  if(name==='Sp Ctrl')return ['Switch','Manual'][v];
  if(name==='Speed Sw')return ['Slow','Fast'][v];
  if(name.startsWith('Vo '))return ['A','I','U','E','O'][v];
  if(name==='VoiceCtrl')return v===-63?'Bottom':v===0?'Center':v===63?'Top':signed(v);
  if(name==='InitPhase'||name==='LFOSpread')return `${signed(v*10)}°`;
  if(/^(?:Pre|B[1-4]) Freq$/.test(name))return hz(t.eqHz[v]);
  if(/^(?:Pre|B[1-4]) Q$/.test(name))return (0.5+v/10).toFixed(1);
  if(/^(?:Pre|B[1-4]) Gain$/.test(name)||['LoEQGain','HiEQGain','Low EQ','High EQ'].includes(name))return `${signed(v/2,1)} dB`;
  if(['GainAdjst','Tu1 Gain','Tu2 Gain'].includes(name))return v===-41?'-Inf dB':`${signed(v)} dB`;
  if(name==='Threshold'&&kind===2)return `${signed(v)} dB`;
  if(name==='Bit')return `${v+4} bit`;
  if(name==='Fs')return `${(t.decimatorHz[v]/1000).toFixed(1)} kHz`;
  if(name==='Ratio')return v===69?'Inf:1':`${(8388607/t.inverseRatio[v]).toFixed(1)}:1`;
  if(name==='Attack'&&kind<=3)return `${(t.attackTenthsMs[v]/10).toFixed(1)} ms`;
  if(name==='Release'&&kind===3)return `${(t.releaseTenthsMs[v]/10).toFixed(1)} ms`;
  if(name==='LFO Freq'||name==='Wah Freq'&&kind===19)return lfoHz(t.lfoHz[v]);
  if(name==='TimeRatio'){const sync=def.properties.findIndex(p=>p.name==='TempoSync');return `${((parameters[sync]?t.syncRatio:t.freeRatio)[v]/10).toFixed(1)}%`;}
  if(/^(?:[LCR] Delay|Tap[12]Delay|Delay)$/.test(name)){
    if(kind===22)return `${(t.chorusTenthsMs[v]/10).toFixed(1)} ms`;
    const table=kind===26?t.stereoMs:[14,16].includes(kind)?(master?t.masterStereoMs:t.stereoMs):[17,19].includes(kind)?(master?t.masterModMonoMs:t.modMonoMs):kind===18?(master?t.masterModStereoMs:t.modStereoMs):(master?t.masterLcrMs:t.lcrMs);return `${table[v]} ms`;
  }
  if(name==='PreDelayL'||name==='PreDelayR')return `${(t.chorusTenthsMs[v]/10).toFixed(1)} ms`;
  if(name==='Pre Delay')return `${t.preDelayMs[v]} ms`;
  if(name==='ER Time')return `${t.earlyMs[v]} ms`;
  if(name==='Rev Time'){const small=master?parameters[1]>=4:parameters[1]===2;return `${(((small?t.smallReverb:t.largeReverb)[v]+1)/10).toFixed(1)} s`;}
  if(name==='Duration')return `${(master?t.masterGrainMs:t.stereoMs)[v]} ms`;
  if(name==='NoteFine'||name==='Fine')return `${signed(v*2)} ¢`;
  if(name==='FixedFreq')return hz(t.ringHz[v]);
  if(name==='H/R Bal')return v===0?'Rotor':v===100?'Horn':`${v}:${100-v}`;
  if(name==='HornRatio'||name==='RotrRatio')return v===0?'Stop':(0.5+(v-1)*.02).toFixed(2);
  if(p.max===100)return `${pad(v)}%`;
  if(p.min<0)return signed(v);
  return pad(v);
}
const choiceCache=new WeakMap();
export function effectChoices(def,master,index,parameters){
  let cache=choiceCache.get(def);if(!cache){cache=new Map();choiceCache.set(def,cache);}
  const sync=def.properties.findIndex(p=>p.name==='TempoSync'),key=`${master}:${index}:${def.properties[index].name==='TimeRatio'?parameters[sync]:def.properties[index].name==='Rev Time'?parameters[1]:0}`;
  if(!cache.has(key)){const p=def.properties[index];cache.set(key,Array.from({length:p.max-p.min+1},(_,i)=>({value:i+p.min,label:effectValueLabel(def,master,index,i+p.min,parameters)})));}return cache.get(key);
}
export function effectFieldState(def,master,index,parameters){
  const p=def.properties[index],name=p.name,kind=def.kind;
  const duplicates=def.properties.filter(q=>q.name===name).length>1;
  if(duplicates&&/^(?:[LCR] Delay|Tap[12]Delay|Delay|Duration)$/.test(name)){const sync=def.properties.findIndex(q=>q.name==='TempoSync'),visible=p.max<=16?!!parameters[sync]:!parameters[sync];return {hidden:!visible,disabled:!visible};}
  if(name==='LFO Freq'&&def.properties[index-1]?.name==='TempoSync')return {hidden:!!parameters[index-1],disabled:!!parameters[index-1]};
  if(name==='Sync Note'){const sync=def.properties.findLastIndex((q,j)=>j<index&&q.name==='TempoSync');return {hidden:!parameters[sync],disabled:!parameters[sync]};}
  if(!master&&(kind===11&&index>=4||kind===26&&index>=3&&index<=8||kind===27&&index===10||kind===12&&(index===5||index===6))||master&&kind===12&&index>=7)return {hidden:true,disabled:true};
  if(!master&&kind<=3&&name==='Env Sel')return {hidden:false,disabled:true};
  if(name==='InitPhase'){const key=def.properties.findLastIndex((q,j)=>j<index&&q.name==='Key Sync');return {hidden:false,disabled:parameters[key]===0};}
  if(kind===25&&(index===2&&parameters[1]!==0||[3,4].includes(index)&&parameters[1]===0))return {hidden:false,disabled:true};
  if(kind===6&&name.endsWith(' Q')){const band=Number(name[1]),type=band===1?2:band===(master?4:2)?3:-1;if(type>=0&&parameters[type]!==0)return {hidden:false,disabled:true};}
  return {hidden:false,disabled:false};
}
