import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

// One revision for the module graph, workers, parameter tables and DSP binary.
// Relative imports need their own query: browsers do not inherit the parent's.
export function versionWebBuild(directory){
  const files=fs.readdirSync(directory).filter(name=>/\.(?:js|css|json|wasm|html)$/.test(name)).sort();
  const sources=new Map(files.map(name=>{
    const bytes=fs.readFileSync(path.join(directory,name));
    const source=/\.(?:js|html)$/.test(name)?bytes.toString().replace(/\?v=[a-f0-9]{16}(?=['"])/g,'').replace(/\n  <meta name="rustias-build" content="[a-f0-9]{16}">/g,''):bytes;
    return [name,source];
  }));
  const hash=createHash('sha256');
  for(const name of files){hash.update(name);hash.update(sources.get(name));}
  const revision=hash.digest('hex').slice(0,16);
  const stamp=(prefix,url,suffix)=>`${prefix}${url}?v=${revision}${suffix}`;
  for(const name of files.filter(name=>name.endsWith('.js'))){
    const file=path.join(directory,name),source=sources.get(name);
    const versioned=source
      .replace(/(\bfrom\s*['"])(\.\.?\/[^'"?]+\.js)(['"])/g,(_all,p,u,s)=>stamp(p,u,s))
      .replace(/(\bimport\s*['"])(\.\.?\/[^'"?]+\.js)(['"])/g,(_all,p,u,s)=>stamp(p,u,s))
      .replace(/(\b(?:import|importScripts)\(\s*['"])(\.\.?\/[^'"?]+\.js)(['"])/g,(_all,p,u,s)=>stamp(p,u,s))
      .replace(/(\bnew URL\(\s*['"])((?:\.\.?\/|samples\/|vendor\/)[^'"?]+\.(?:js|json|wasm))(['"])/g,(_all,p,u,s)=>stamp(p,u,s));
    fs.writeFileSync(file,versioned);
  }
  const file=path.join(directory,'index.html'),html=sources.get('index.html');
  fs.writeFileSync(file,html.replace(/((?:href|src)=['"])(\.\/[^'"?]+\.(?:js|css))(['"])/g,(_all,p,u,s)=>stamp(p,u,s)).replace('<head>',`<head>\n  <meta name="rustias-build" content="${revision}">`));
  return revision;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(`Web asset revision: ${versionWebBuild(process.argv[2]??'dist')}`);
