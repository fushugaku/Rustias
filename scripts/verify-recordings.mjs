import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {RecordingTap} from '../web/recording-tap.js';
import {wavHeader,recordingTime,recordingFilename} from '../web/recording-format.js';

export function verifyRecordings() {
  const messages=[];
  const tap=new RecordingTap((data,transfer)=>{messages.push(data);if(data.pcm)assert.equal(transfer[0],data.pcm.buffer);},48000,3);
  const left=Float32Array.of(-2,-.5,0,.5,2,NaN,Infinity),right=Float32Array.of(1,.25,-1,-.25,0,1,-1);
  tap.capture(left,right);assert.equal(messages.length,0,'An idle recorder emits no buffers');
  tap.start('one');tap.start('duplicate');tap.capture(left,right);tap.stop('wrong');assert.equal(tap.id,'one');tap.stop('one');
  assert.deepEqual(messages.filter(m=>m.pcm).map(m=>m.index),[0,1,2]);
  assert.deepEqual(messages.filter(m=>m.pcm).map(m=>m.frames),[3,6,7]);
  assert.deepEqual(messages.filter(m=>m.pcm).flatMap(m=>Array.from(m.pcm)),[-32768,32767,-16384,8192,0,-32768,16384,-8192,32767,0,0,32767,0,-32768]);
  assert.equal(messages.at(-1).frames,7,'The final partial chunk must be retained');
  const count=messages.length;tap.capture(left,right);assert.equal(messages.length,count);
  tap.start('two');tap.capture(left.subarray(0,1),right.subarray(0,1));tap.stop();assert.equal(messages.at(-1).frames,1,'A new take resets its frame count');
  assert.equal(recordingTime(3601),'1:00:01');assert.equal(recordingFilename('Program / 1','Take: 2','wav'),'Program _ 1 - Take_ 2.wav');
  assert.throws(()=>wavHeader(0x40000000,48000),/limit/);
  const source=fs.readFileSync(new URL('../web/vendor/lamejs/lame.all.js',import.meta.url));
  assert.equal(createHash('sha256').update(source).digest('hex'),'026bd88846040f357a937cd85821a48492a362eff0812cda734f23fca55fea3b','The licensed encoder is unchanged from the pinned distribution');
  const sandbox={};vm.runInNewContext(source.toString(),sandbox);
  for(const rate of [44100,48000]) {
    const header=wavHeader(rate,rate),view=new DataView(header);
    const read=sandbox.lamejs.WavHeader.readHeader(view);
    assert.equal(read.channels,2);assert.equal(read.sampleRate,rate);assert.equal(read.dataLen,rate*4);
    const encoder=new sandbox.lamejs.Mp3Encoder(2,rate,320),parts=[];
    for(let offset=0;offset<rate;offset+=1152){
      const length=Math.min(1152,rate-offset),l=new Int16Array(length),r=new Int16Array(length);
      for(let i=0;i<length;i++){l[i]=Math.round(12000*Math.sin(2*Math.PI*440*(offset+i)/rate));r[i]=Math.round(12000*Math.sin(2*Math.PI*880*(offset+i)/rate));}
      parts.push(Buffer.from(encoder.encodeBuffer(l,r)));
    }
    parts.push(Buffer.from(encoder.flush()));const mp3=Buffer.concat(parts);let frames=0,position=0;
    while(position<mp3.length){
      const a=mp3[position],b=mp3[position+1],c=mp3[position+2],d=mp3[position+3];
      assert.equal(a,255);assert.equal(b&254,250,'MPEG-1 Layer III header');assert.equal(c>>4,14,'320 kbps');
      assert.equal([44100,48000,32000][c>>2&3],rate);assert.notEqual(d>>6,3,'Stereo MP3');
      position+=Math.floor(144*320000/rate)+(c>>1&1);frames++;
    }
    assert.equal(position,mp3.length,'Every MP3 frame is complete');assert.ok(frames*1152/rate>=1&&frames*1152/rate<1.1);
  }
  return 'Browser recording: continuous stereo output, ordered transferable PCM chunks and partial tails; independent valid 16-bit WAV and real 320 kbps MP3 at 44.1/48 kHz';
}
