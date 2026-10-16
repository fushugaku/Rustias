import {build} from 'esbuild';
import fs from 'node:fs/promises';
import path from 'node:path';
import {installPluginCrypto} from '../web/plugin-platform.js';
const root=path.resolve(import.meta.dirname,'..'),out=path.join(root,'output/plugin/resources');
await fs.rm(out,{recursive:true,force:true});
await fs.mkdir(out,{recursive:true});
await fs.cp(path.join(root,'dist'),path.join(out,'web'),{recursive:true});
const workers={};
for(const file of ['recordings-worker.js','rdl-worker.js']){
  const result=await build({entryPoints:[path.join(root,'web',file)],bundle:true,format:'iife',platform:'browser',target:'safari15',write:false,legalComments:'eof'});
  // WKWebView custom schemes do not support module workers. These contain the
  // same source bundled into classic blob workers, including the MP3 encoder.
  workers[file]=`(${installPluginCrypto.toString()})();crypto.randomUUID();\n`+(file==='recordings-worker.js'?(await fs.readFile(path.join(root,'web/vendor/lamejs/lame.all.js'),'utf8'))+'\nconst importScripts=()=>{};\n':'')+result.outputFiles[0].text;
}
const ui=await build({entryPoints:[path.join(root,'web/app.js')],bundle:true,format:'esm',platform:'browser',target:'safari15',write:false,define:{'import.meta.url':'window.location.href'},legalComments:'eof'});
await fs.writeFile(path.join(out,'web/host-app.js'),'window.__RUSTIAS_PLUGIN_WORKERS__='+JSON.stringify(workers)+';\n(async()=>{\n'+ui.outputFiles[0].text+'\n})().catch(error=>window.dispatchEvent(new ErrorEvent("error",{message:error.message,error})));\n');
const html=await fs.readFile(path.join(out,'web/index.html'),'utf8');
await fs.writeFile(path.join(out,'web/index.html'),html.replace(/<script type="module" src="[^"]+"><\/script>/,'<script defer src="./host-app.js"></script>'));
await build({entryPoints:[path.join(root,'plugin/runtime.js')],outfile:path.join(out,'runtime.js'),bundle:true,format:'iife',platform:'neutral',target:'safari15',legalComments:'eof'});
await fs.rm(path.join(out,'rustias.wasm'),{force:true});
await fs.copyFile(path.join(root,'plugin/THIRD_PARTY_NOTICES.txt'),path.join(out,'THIRD_PARTY_NOTICES.txt'));
console.log('Plugin resources use the shared control layer and interface; DSP is a native Rust static library.');
