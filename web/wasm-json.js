import {engineWrite} from './engine-io.js';
// Engine payloads use ASCII identifiers and numeric fields. Grow the shared buffer
// before obtaining its pointer: memory growth may invalidate existing JS views.
export function writeWasmJson(api,value){
  const json=JSON.stringify(value),bytes=Uint8Array.from(json,c=>c.charCodeAt(0));
  if(bytes.length>api.rustias_preset_capacity()&&!api.rustias_preset_reserve(bytes.length))throw new Error('Not enough memory for this program.');
  engineWrite(api,api.rustias_preset_buffer(),bytes);
  return bytes.length;
}
