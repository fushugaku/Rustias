// Audio engine buffers use WASM views in browsers and bounded native tokens in
// plugins. Synthesis and parameter entry points are the same Rust implementation.
export function engineWrite(api,pointer,view){
  if(api.writeBuffer)api.writeBuffer(pointer,view);
  else new Uint8Array(api.memory.buffer,pointer,view.byteLength).set(new Uint8Array(view.buffer,view.byteOffset,view.byteLength));
}
export function engineBytes(api,pointer,length){
  return api.readBuffer?api.readBuffer(pointer,length,false):new Uint8Array(api.memory.buffer,pointer,length);
}
export function engineFloats(api,pointer,length){
  return api.readBuffer?api.readBuffer(pointer,length,true):new Float32Array(api.memory.buffer,pointer,length);
}
