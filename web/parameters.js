import {MAX_TIMBRES,PATCH_ROUTES,LFO3_BASE} from './limits.js';
export function webParameters(native){
  const parameters=structuredClone(native);
  if(parameters.length!==155)throw new Error('Invalid native parameter definitions.');
  parameters[141].max=MAX_TIMBRES-1;
  for(let i=0;i<6;i++){
    const source=parameters[90+4*i];source.max=16;source.values=[0,1,2,3,4,5,6,7,8,16];source.options.push('LFO 3');
    const p=parameters[91+4*i];p.max=42;p.options.push('Patch 7 feedback','Patch 8 feedback','LFO 3 Rate');p.values.push(40,41,42);
  }
  for(let i=6;i<PATCH_ROUTES;i++)for(let n=0;n<4;n++)parameters.push({...structuredClone(parameters[90+n]),id:155+4*(i-6)+n,group:`Patch ${i+1}`});
  for(let i=0;i<8;i++)parameters.push({...structuredClone(parameters[73+i]),id:LFO3_BASE+i,group:'LFO 3'});
  return parameters;
}
export function extendValues(input,parameters){
  const v=[...input];
  if(v.length===153)v.push(v[151]);
  if(v.length===154){const old=v[29];v.push(old===3?0:old===2?1:old>=4?old-2:1);if(old>=2)v[29]=2;}
  if(v.length===155)v.push(3,0,64,0,3,0,64,0);
  if(v.length===LFO3_BASE){if(parameters)for(const p of parameters.slice(LFO3_BASE))v.push(p.default);else v.push(0,64,45,2,0,0,8,0);}
  return v;
}
