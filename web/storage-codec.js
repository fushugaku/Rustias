// Small, synchronous LZW container for localStorage. Exported program JSON stays
// ordinary JSON. The byte alphabet preserves Unicode through UTF-8; CLEAR keeps
// dictionary growth bounded and malformed containers cannot expand indefinitely.
const CLEAR=256,LIMIT=65535,MAX_BYTES=64*1024*1024;
function base64(bytes){let text='';for(let i=0;i<bytes.length;i+=32768)text+=String.fromCharCode(...bytes.subarray(i,i+32768));return btoa(text);}
export function compressStorage(text){
  const input=new TextEncoder().encode(text),codes=[CLEAR];let dictionary=new Map(),next=257,word='';
  for(const byte of input){const char=String.fromCharCode(byte),joined=word+char;if(dictionary.has(joined)||joined.length===1){word=joined;continue;}
    codes.push(word.length===1?word.charCodeAt(0):dictionary.get(word));
    if(next<=LIMIT)dictionary.set(joined,next++);else{codes.push(CLEAR);dictionary=new Map();next=257;}
    word=char;
  }
  if(word)codes.push(word.length===1?word.charCodeAt(0):dictionary.get(word));
  const bytes=new Uint8Array(codes.length*2);codes.forEach((code,i)=>{bytes[2*i]=code>>8;bytes[2*i+1]=code&255;});return base64(bytes);
}
export function decompressStorage(encoded){
  if(typeof encoded!=='string'||encoded.length>MAX_BYTES||!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded))throw new Error('Invalid compressed library.');
  const bytes=atob(encoded);if(bytes.length%2||bytes.length<2||((bytes.charCodeAt(0)<<8)|bytes.charCodeAt(1))!==CLEAR)throw new Error('Invalid compressed library.');
  let dictionary=[],next=257,word='',size=0;const chunks=[];
  for(let i=0;i<bytes.length;i+=2){const code=(bytes.charCodeAt(i)<<8)|bytes.charCodeAt(i+1);if(code===CLEAR){dictionary=[];next=257;word='';continue;}
    const entry=code<256?String.fromCharCode(code):dictionary[code]??(code===next&&word?word+word[0]:null);
    if(entry==null)throw new Error('Invalid compressed library code.');size+=entry.length;if(size>MAX_BYTES)throw new Error('Compressed library is too large.');chunks.push(entry);
    if(word&&next<=LIMIT)dictionary[next++]=word+entry[0];word=entry;
  }
  const output=new Uint8Array(size);let offset=0;for(const chunk of chunks)for(let i=0;i<chunk.length;i++)output[offset++]=chunk.charCodeAt(i);
  return new TextDecoder('utf-8',{fatal:true}).decode(output);
}
