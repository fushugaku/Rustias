import {parseRdl} from './rdl.js';
self.onmessage=({data})=>{
  try{
    // A separate instance parses the bank without constructing the synth or
    // blocking the live AudioWorklet or the browser interface.
    const api=new WebAssembly.Instance(data.module,{}).exports;
    self.postMessage({library:parseRdl(api,data.buffer)});
  }catch(error){self.postMessage({error:error.message??String(error)});}
};
