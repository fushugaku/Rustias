import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const assets = path.resolve(process.argv[2] ?? path.join(root, 'android/app/build/generated/rustiasAssets/public'));
const html = fs.readFileSync(path.join(assets, 'index.html'), 'utf8');
assert.match(html, /<script src="\.\/native-bridge\.js" defer><\/script>/);
assert.deepEqual(fs.readFileSync(path.join(assets, 'rustias.wasm')), fs.readFileSync(path.join(root, 'dist/rustias.wasm')), 'Android packages the same WASM bytes as the website');
assert.equal(fs.readdirSync(path.join(assets, 'samples')).filter(name => /\.wav$/i.test(name)).length, 64);
assert.equal(fs.readdirSync(path.join(assets, 'samples/tr-909')).filter(name => /\.wav$/i.test(name)).length, 160);
for (const file of html.matchAll(/(?:src|href)="\.\/([^"?]+)(?:\?[^"\s]*)?"/g)) assert.ok(fs.existsSync(path.join(assets, file[1])), `Missing Android entry asset: ${file[1]}`);
const wasm = await WebAssembly.compile(fs.readFileSync(path.join(assets, 'rustias.wasm')));
assert.ok(WebAssembly.Module.exports(wasm).some(value => value.name === 'rustias_osc3_control'));

const source = fs.readFileSync(path.join(root, 'android/web/native-bridge.js'), 'utf8');
function harness(options = {}) {
  const messages = [], chunks = [], handlers = [], attributes = new Map();
  const body = {dataset:{}}, alert = {hidden:true}, fullscreen = {setAttribute:(name,value)=>attributes.set(name,value)};
  const native = {
    postMessage(raw) {
      const data = JSON.parse(raw); messages.push(data);
      let result = {request:data.request, ok:true};
      if (data.type === 'chunk') {
        const bytes = Buffer.from(data.data, 'base64');
        assert.ok(bytes.length <= 128 * 1024);
        assert.equal(data.offset, chunks.reduce((total, part) => total + part.length, 0));
        chunks.push(bytes); result.offset = data.offset + bytes.length;
        if (options.badChunk) result.offset--;
      }
      if (data.type === 'finish') result.cancelled = !!options.cancel;
      if (data.type === 'fullscreen') result.fullscreen = data.active;
      queueMicrotask(() => native.onmessage({data:JSON.stringify(result)}));
    },
  };
  const document = {
    body,
    querySelector(selector) { return selector === '#error' ? alert : selector === '#fullscreen' ? fullscreen : null; },
    querySelectorAll() { return []; },
    addEventListener(type, handler, capture) { handlers.push({type, handler, capture}); },
  };
  const window = {RustiasFiles:native, dispatchEvent() {}};
  vm.runInNewContext(source, {window, document, Uint8Array, Event, setTimeout, clearTimeout, btoa:value=>Buffer.from(value,'binary').toString('base64'), fetch:async()=>({blob:async()=>new Blob(['{"program":1}'],{type:'application/json'})})});
  return {api:window.RustiasAndroid, messages, chunks, handlers, body, attributes, alert};
}

const test = harness();
const bytes = Buffer.alloc(128 * 1024 * 2 + 151);
for (let i = 0; i < bytes.length; i++) bytes[i] = i * 17 & 255;
const blob = new Blob([bytes], {type:'audio/wav'});
const pending = test.api.exportBlob(blob, 'Program 1 — take.wav');
await assert.rejects(test.api.exportBlob(blob, 'duplicate.wav'), /Finish the current file export/);
assert.equal((await pending).cancelled, false);
assert.deepEqual(Buffer.concat(test.chunks), bytes, 'Recording bytes survive the chunked native bridge exactly');
assert.deepEqual(test.messages.map(value=>value.type), ['begin','chunk','chunk','chunk','finish']);
assert.equal(test.messages[0].mime, 'audio/wav');
assert.equal(test.messages[0].name, 'Program 1 — take.wav');
assert.equal(test.messages[0].size, bytes.length);
const cancelled = harness({cancel:true});
assert.equal((await cancelled.api.exportBlob(new Blob(), 'empty.json')).cancelled, true);
const broken = harness({badChunk:true});
await assert.rejects(broken.api.exportBlob(blob, 'broken.wav'), /incomplete chunk/);
assert.equal(broken.messages.at(-1).type, 'abort');
assert.ok(!broken.messages.some(value=>value.type === 'finish'));

const click = harness();
let prevented = false;
const link = {href:'blob:https://appassets.androidplatform.net/test', download:'patch.json', setAttribute(){}, removeAttribute(){}};
click.handlers.find(value=>value.type==='click'&&!value.capture).handler({target:{closest:()=>link}, defaultPrevented:false, preventDefault(){prevented=true;}});
await new Promise(resolve=>setTimeout(resolve,20));
assert.equal(prevented,true);
assert.equal(click.messages.at(-1).type,'finish');
assert.equal(Buffer.concat(click.chunks).toString(),'{"program":1}');
click.api.setFullscreen(true);
assert.equal(click.body.dataset.fullscreen,'on');
assert.equal(click.attributes.get('aria-pressed'),'true');
assert.equal(click.api.back(),false);
console.log('Android assets, shared WASM, 224 samples, chunked WAV/JSON exports, cancellation and recovery verified without an audio device.');
