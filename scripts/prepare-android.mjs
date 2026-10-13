import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

// Package the already-built website: Android never has its own synthesis source.
export function prepareAndroid(source, destination) {
  const html = fs.readFileSync(path.join(source, 'index.html'), 'utf8');
  if (!html.includes('name="rustias-build"')) throw new Error('Build the versioned web app first: bash scripts/build-web.sh');
  fs.rmSync(destination, {recursive:true, force:true});
  fs.mkdirSync(destination, {recursive:true});
  fs.cpSync(source, destination, {recursive:true});
  const bridge = fileURLToPath(new URL('../android/web/native-bridge.js', import.meta.url));
  fs.copyFileSync(bridge, path.join(destination, 'native-bridge.js'));
  for (const name of fs.readdirSync(destination).filter(name => /^index(?:\.[a-f0-9]{16})?\.html$/.test(name))) {
    const file = path.join(destination, name);
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('<head>', '<head>\n  <script src="./native-bridge.js" defer></script>'));
  }
  console.log(`Android offline assets ready: ${destination}`);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  prepareAndroid(path.resolve(process.argv[2] ?? 'dist'), path.resolve(process.argv[3] ?? 'android/app/build/generated/rustiasAssets/public'));
}
