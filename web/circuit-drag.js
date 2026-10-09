// A drag belongs to one pointer and ends before a module is reparented.
export function bindModuleDrag({heading,canvas,position,getZoom,onStart,onMove,onEnd,host=window}){
  let gesture=null;
  const point=e=>{const r=canvas.getBoundingClientRect(),z=getZoom();return{x:(e.clientX-r.left)/z,y:(e.clientY-r.top)/z};};
  function finish(){
    if(!gesture)return;const {id,moved}=gesture;gesture=null;
    if(heading.hasPointerCapture(id))heading.releasePointerCapture(id);
    if(moved)onEnd();
  }
  function down(e){
    if(e.button!==0||e.isPrimary===false||e.target.closest('button,input,select,textarea,a'))return;
    finish();const p=point(e);gesture={id:e.pointerId,x:e.clientX,y:e.clientY,dx:p.x-position.x,dy:p.y-position.y,moved:false};
    e.preventDefault();heading.setPointerCapture(e.pointerId);
  }
  function move(e){
    const g=gesture;if(!g||e.pointerId!==g.id)return;
    if(e.pointerType!=='touch'&&!(e.buttons&1)){finish();return;}
    if(!g.moved){if(Math.hypot(e.clientX-g.x,e.clientY-g.y)<4)return;g.moved=true;onStart();}
    const p=point(e);onMove(p.x-g.dx,p.y-g.dy);
  }
  const end=e=>{if(gesture&&e.pointerId===gesture.id)finish();};
  function key(e){
    if(e.target!==heading)return;const d={ArrowLeft:[-16,0],ArrowRight:[16,0],ArrowUp:[0,-16],ArrowDown:[0,16]}[e.key];
    if(d){finish();e.preventDefault();onStart();onMove(position.x+d[0],position.y+d[1]);onEnd();}
  }
  const events={pointerdown:down,pointermove:move,pointerup:end,pointercancel:end,lostpointercapture:end,keydown:key};
  for(const [name,handler]of Object.entries(events))heading.addEventListener(name,handler);
  host.addEventListener('blur',finish);host.addEventListener('pagehide',finish);
  return ()=>{finish();for(const [name,handler]of Object.entries(events))heading.removeEventListener(name,handler);host.removeEventListener('blur',finish);host.removeEventListener('pagehide',finish);};
}
