import fs from 'node:fs';
const api=new WebAssembly.Instance(await WebAssembly.compile(fs.readFileSync(process.argv[2])),{}).exports;
const length=api.rustias_effect_catalog();
const data=Buffer.from(new Uint8Array(api.memory.buffer,api.rustias_effect_catalog_buffer(),length));
fs.writeFileSync(process.argv[3],data);
