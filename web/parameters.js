import {MAX_TIMBRES,PATCH_ROUTES,LFO3_BASE,OSC3_BASE,OSC4_BASE} from './limits.js';
export function webParameters(native){
  const parameters=structuredClone(native);
  if(parameters.length!==155)throw new Error('Invalid native parameter definitions.');
  parameters[141].max=MAX_TIMBRES-1;
  parameters[19].label='OSC 3 level';
  for(let i=0;i<6;i++){
    const source=parameters[90+4*i];source.max=16;source.values=[0,1,2,3,4,5,6,7,8,16];source.options.push('LFO 3');
    const p=parameters[91+4*i];p.max=68;p.options=p.options.map(label=>label==='Noise Level'?'OSC 3 Level':label);p.options.push('Patch 7 feedback','Patch 8 feedback','LFO 3 Rate','OSC 3 Pitch','OSC 3 Control 1','OSC 3 Control 2','OSC 3 Waveform','OSC 3 Mode','OSC 3 Semitone','OSC 3 Fine','OSC 3 Ring / Sync','OSC 3 Source');p.values.push(40,41,42,43,44,45,46,47,48,49,50,51);p.options.push('OSC 4 Pitch','OSC 4 Control 1','OSC 4 Control 2','OSC 4 Waveform','OSC 4 Mode','OSC 4 Semitone','OSC 4 Fine','OSC 4 Ring / Sync','OSC 4 Source','OSC 4 Level','OSC 4 Shaper','Filter 1 Model','Filter 2 Model','Filter 1 Drive','Filter 2 Drive','Filter 1 Morph','Filter 2 Morph');p.values.push(...Array.from({length:17},(_,i)=>52+i));
  }
  for(let i=6;i<PATCH_ROUTES;i++)for(let n=0;n<4;n++)parameters.push({...structuredClone(parameters[90+n]),id:155+4*(i-6)+n,group:`Patch ${i+1}`});
  for(let i=0;i<8;i++)parameters.push({...structuredClone(parameters[73+i]),id:LFO3_BASE+i,group:'LFO 3'});
  [0,10,11,12,15,16].forEach((id,i)=>parameters.push({...structuredClone(parameters[id]),id:OSC3_BASE+i,group:'OSC 3',...(i===0?{default:4}:{})}));
  parameters.push({...structuredClone(parameters[14]),id:OSC3_BASE+6,group:'OSC 3',label:'Ring / Sync'});
  parameters.push({...structuredClone(parameters[13]),id:OSC3_BASE+7,group:'OSC 3',label:'Source',min:0,max:2,default:0,options:['OSC 1','OSC 2','OSC 1 → 2'],values:[0,1,2]});
  [0,10,11,12,15,16,14].forEach((id,i)=>parameters.push({...structuredClone(parameters[id]),id:OSC4_BASE+i,group:'OSC 4',...(i===0?{default:3}:i===1?{max:4,default:4,options:[...parameters[id].options,'Wavefolder']}:{}),...(i===2?{default:32}:i===3?{default:64}:i===6?{label:'Ring / Sync'}:{})}));
  parameters.push({...structuredClone(parameters[178]),id:186,group:'OSC 4',max:3,options:['OSC 1','OSC 2','OSC 1 → 2','OSC 3'],values:[0,1,2,3]});
  parameters.push({...structuredClone(parameters[17]),id:187,group:'Mixer',label:'OSC 4 level',default:0});
  parameters.push({id:188,group:'OSC 4',label:'Shaper',min:0,max:4,default:0,options:['Triangle fold','Sine fold','Soft fold','Asymmetric fold','Rectify']});
  const models=['RADIAS','Ladder LP24','MS-20 LP12','MS-20 HP12','Pro LP24','SEM','Multi Filter','Ladder HP24','Ladder BP24','M/Poly LP24','Linear LP12','Linear LP24','Linear HP12','Linear HP24','Linear BP12','Linear BP24','Linear Notch12','Linear Notch24'];
  for(const id of [189,190])parameters.push({id,group:id===189?'Filter 1':'Filter 2',label:'Model',min:0,max:17,default:0,options:models});
  for(const [id,group,label]of [[191,'Filter 1','Drive'],[192,'Filter 2','Drive'],[193,'Filter 1','Model morph'],[194,'Filter 2','Model morph']])parameters.push({id,group,label,min:0,max:127,default:0});
  return parameters;
}
export function extendValues(input,parameters){
  const v=[...input];
  if(v.length===153)v.push(v[151]);
  if(v.length===154){const old=v[29];v.push(old===3?0:old===2?1:old>=4?old-2:1);if(old>=2)v[29]=2;}
  if(v.length===155)v.push(3,0,64,0,3,0,64,0);
  if(v.length===LFO3_BASE){if(parameters)for(const p of parameters.slice(LFO3_BASE,OSC3_BASE))v.push(p.default);else v.push(0,64,45,2,0,0,8,0);}
  if(v.length===OSC3_BASE){if(parameters)for(const p of parameters.slice(OSC3_BASE,179))v.push(p.default);else v.push(4,0,0,0,64,64,0,0);}
  if(v.length===OSC3_BASE+6)v.push(0,0);
  if(v.length===179&&parameters)for(const p of parameters.slice(179))v.push(p.default);
  else if(v.length===179)v.push(3,4,32,64,64,64,0,0,0,0,0,0,0,0,0,0);
  return v;
}
