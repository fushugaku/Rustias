import {MAX_TIMBRES,PATCH_ROUTES} from './limits.js';
export function webParameters(native){
  const parameters=structuredClone(native);
  if(parameters.length!==155)throw new Error('Invalid native parameter definitions.');
  parameters[141].max=MAX_TIMBRES-1;
  for(let i=0;i<6;i++){
    const p=parameters[91+4*i];p.max=41;p.options.push('Patch 7 feedback','Patch 8 feedback');p.values.push(40,41);
  }
  for(let i=6;i<PATCH_ROUTES;i++)for(let n=0;n<4;n++)parameters.push({...structuredClone(parameters[90+n]),id:155+4*(i-6)+n,group:`Patch ${i+1}`});
  return parameters;
}
export function extendValues(input,parameters){
  const v=[...input];
  if(v.length===153)v.push(v[151]);
  if(v.length===154){const old=v[29];v.push(old===3?0:old===2?1:old>=4?old-2:1);if(old>=2)v[29]=2;}
  if(v.length===155){if(parameters)for(const p of parameters.slice(155))v.push(p.default);else v.push(3,0,64,0,3,0,64,0);}
  return v;
}
