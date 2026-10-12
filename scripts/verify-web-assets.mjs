import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {versionWebBuild} from './version-web-build.mjs';

export function verifyWebAssets(directory='dist'){
  const html=fs.readFileSync(path.join(directory,'index.html'),'utf8'),revision=html.match(/name="rustias-build" content="([a-f0-9]{16})"/)?.[1];
  assert.ok(revision,'Built HTML identifies its asset revision');
  const check=url=>{
    const parsed=new URL(url,'https://test.invalid/');assert.equal(parsed.searchParams.get('v'),revision,`${url} belongs to this build`);
    const name=path.basename(parsed.pathname),relative=parsed.pathname.slice(1);
    if(!relative.includes('/'))assert.ok(name.includes(`.${revision}.`),`${url} cannot reuse an older unversioned asset`);
    assert.ok(fs.existsSync(path.join(directory,relative)),`${url} exists in the deployment`);
  };
  for(const m of html.matchAll(/(?:href|src)=['"]([^'"]+\.(?:js|css)(?:\?[^'"]*)?)['"]/g))check(m[1]);
  let imports=0;
  for(const file of fs.readdirSync(directory).filter(n=>n.endsWith('.js')&&!/\.[a-f0-9]{16}\.js$/.test(n))){
    const js=fs.readFileSync(path.join(directory,file),'utf8');
    for(const m of js.matchAll(/\bfrom\s*['"](\.\.?\/[^'"]+\.js(?:\?[^'"]*)?)['"]/g)){check(m[1]);imports++;}
    for(const m of js.matchAll(/\bnew URL\(\s*['"]((?:\.\.?\/|samples\/|vendor\/)[^'"]+\.(?:js|json|wasm)(?:\?[^'"]*)?)['"]/g))check(m[1]);
  }
  assert.ok(imports>50,'Nested module dependencies also carry the revision');
  const temporary=fs.mkdtempSync(path.join(os.tmpdir(),'rustias-assets-'));
  try{
    fs.writeFileSync(path.join(temporary,'index.html'),'<head><script type="module" src="./app.js"></script></head>');
    fs.writeFileSync(path.join(temporary,'app.js'),`import {x} from './child.js'; fetch(new URL('./rustias.wasm',import.meta.url)); new Worker(new URL('./worker.js',import.meta.url));`);
    fs.writeFileSync(path.join(temporary,'child.js'),'export const x=1;');
    fs.writeFileSync(path.join(temporary,'worker.js'),`import './child.js';`);
    fs.writeFileSync(path.join(temporary,'rustias.wasm'),Buffer.from([0,1,2]));
    const first=versionWebBuild(temporary);assert.equal(versionWebBuild(temporary),first,'Stamping the same build is idempotent');
    assert.equal(fs.readFileSync(path.join(temporary,`index.${first}.html`),'utf8'),fs.readFileSync(path.join(temporary,'index.html'),'utf8'),'The release entry loads the same instrument without a cached HTML document');
    fs.writeFileSync(path.join(temporary,'rustias.wasm'),Buffer.from([0,1,3]));
    const next=versionWebBuild(temporary);assert.notEqual(next,first,'A DSP change invalidates the complete browser asset graph');
    assert.ok(fs.readFileSync(path.join(temporary,'app.js'),'utf8').includes(`./child.${next}.js?v=${next}`));
    assert.ok(fs.readFileSync(path.join(temporary,'worker.js'),'utf8').includes(`./child.${next}.js?v=${next}`));
    assert.ok(fs.existsSync(path.join(temporary,`rustias.${next}.wasm`)));
    assert.ok(!fs.existsSync(path.join(temporary,`rustias.${first}.wasm`)),'A rebuild removes generated aliases from the preceding build');
  }finally{fs.rmSync(temporary,{recursive:true,force:true});}
  return `Coherent browser assets: ${imports} module imports, workers, tables and WASM share revision ${revision}`;
}
