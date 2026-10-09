import assert from 'node:assert/strict';
import {bindModuleDrag} from '../web/circuit-drag.js';

export function verifyCircuitDrag(){
  class Heading extends EventTarget {
    captured=null;
    closest(){return null;}
    setPointerCapture(id){this.captured=id;}
    hasPointerCapture(id){return this.captured===id;}
    releasePointerCapture(id){if(this.captured===id){this.captured=null;fire(this,'lostpointercapture',{pointerId:id});}}
  }
  function fire(target,type,values={}){const e=new Event(type,{cancelable:true});Object.assign(e,{button:0,buttons:1,pointerId:1,pointerType:'mouse',isPrimary:true,clientX:40,clientY:80,...values});target.dispatchEvent(e);}
  const heading=new Heading(),host=new EventTarget(),position={x:40,y:80};
  let zoom=.5,rect={left:20,top:40},starts=0,ends=0;
  const bind=()=>bindModuleDrag({heading,host,canvas:{getBoundingClientRect:()=>rect},position,getZoom:()=>zoom,onStart:()=>starts++,onMove:(x,y)=>Object.assign(position,{x,y}),onEnd:()=>ends++});
  let dispose=bind();
  fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:42});assert.equal(starts,0,'A click or jitter must not start a module drag');
  fire(heading,'pointermove',{pointerId:2,clientX:90});assert.equal(starts,0,'Another pointer cannot take over the gesture');
  fire(heading,'pointermove',{clientX:60,clientY:90});assert.deepEqual(position,{x:80,y:100},'Movement follows the canvas zoom');
  rect={left:10,top:20};fire(heading,'pointermove',{clientX:60,clientY:90});assert.deepEqual(position,{x:100,y:140},'Canvas scrolling preserves the pointer-to-module offset');
  fire(heading,'pointerup');assert.equal(ends,1);assert.equal(heading.captured,null,'Mouse release also releases capture');
  fire(heading,'pointermove',{clientX:300});assert.deepEqual(position,{x:100,y:140},'Released modules must not follow the pointer');
  fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:70});heading.releasePointerCapture(1);fire(heading,'pointermove',{clientX:500});assert.equal(ends,2,'Lost capture ends and saves a drag exactly once');
  const afterLost={...position};fire(heading,'pointermove',{buttons:0,clientX:800});assert.deepEqual(position,afterLost);
  fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:80});fire(heading,'pointermove',{buttons:0,clientX:900});assert.equal(ends,3,'Missing mouseup is recovered from the buttons state');
  fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:90});host.dispatchEvent(new Event('blur'));assert.equal(ends,4,'Leaving the window cancels the gesture');
  fire(heading,'pointerdown',{pointerType:'touch',buttons:0});fire(heading,'pointermove',{pointerType:'touch',buttons:0,clientX:100});fire(heading,'pointercancel');assert.equal(ends,5,'Touch can move and cancel without mouse buttons');
  fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:110});dispose();const afterDispose={...position};fire(heading,'pointermove',{clientX:1000});assert.deepEqual(position,afterDispose,'Reparenting a module removes every drag handler');assert.equal(ends,6);
  zoom=1;dispose=bind();fire(heading,'pointerdown');fire(heading,'pointermove',{clientX:60});fire(heading,'pointerup');assert.equal(ends,7,'A re-render installs just one gesture handler');
  const beforeArrow={...position};fire(heading,'keydown',{key:'ArrowRight'});assert.equal(position.x,beforeArrow.x+16);assert.equal(ends,8);
  dispose();assert.equal(starts,ends,'Each actual edit has one undo snapshot and one layout notification');
  return 'Module drag: zoom/scroll anchoring, lost capture, mouse release, touch cancellation, window blur and render cleanup';
}
