let onAssign;
export const setMacroAssignmentHandler=handler=>{onAssign=handler;};
// The same gesture belongs to rack, FX, sample and added-module controls.
export function bindMacroTarget(element,getTarget){
  if(!getTarget)return;
  let timer,start,consumed=false;
  const cancel=()=>{clearTimeout(timer);timer=null;start=null;};
  const open=()=>{const target=getTarget();if(!element.disabled&&target&&onAssign){onAssign(target);return true;}return false;};
  element.addEventListener('contextmenu',event=>{if(!onAssign)return;event.preventDefault();cancel();open();});
  element.addEventListener('keydown',event=>{if(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10'){event.preventDefault();open();}});
  element.addEventListener('pointerdown',event=>{cancel();consumed=false;if(event.pointerType!=='touch'||event.button!==0||element.disabled)return;start={x:event.clientX,y:event.clientY};timer=setTimeout(()=>{if(open()){consumed=true;if(element.hasPointerCapture(event.pointerId))element.releasePointerCapture(event.pointerId);}cancel();},500);},{capture:true});
  element.addEventListener('pointermove',event=>{if(start&&Math.hypot(event.clientX-start.x,event.clientY-start.y)>6)cancel();},{capture:true});
  for(const name of ['pointerup','pointercancel','lostpointercapture'])element.addEventListener(name,cancel,{capture:true});
  element.addEventListener('click',event=>{if(consumed){event.preventDefault();event.stopImmediatePropagation();consumed=false;}},{capture:true});
}
