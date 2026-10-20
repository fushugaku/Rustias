import {ENVELOPE_BASE} from './limits.js';

export const ENVELOPE_MODES=['RADIAS','Q · ADSR','Q · ADS1DS2R','Q · One Shot','Q · Loop S1S2','Q · Loop All'];
export const ENVELOPES=[
  {key:'eg1',adsr:[32,33,34,35],curve:40},
  {key:'eg2',adsr:[3,4,5,6],curve:44},
  {key:'eg3',adsr:[36,37,38,39],curve:48},
].map((p,i)=>({...p,base:ENVELOPE_BASE+i*4}));

// Same discrete 2 kHz timing table as q_envelope.rs. Zero is instantaneous.
export const qEnvelopeSeconds=value=>value===0?0:Math.max(1,Math.round(120000**((value-1)/126)))/2000;
export function envelopeParameter(id){
  return ENVELOPES.find(p=>p.adsr.includes(id)||id>=p.curve&&id<=p.curve+3||id>=p.base&&id<p.base+4);
}
export function envelopePreview(v,index){
  const p=ENVELOPES[index],mode=v[p.base]??0,[a,d,s,r]=p.adsr.map(id=>v[id]);
  if(mode===0){
    const attack=8+a/127*52,decay=8+d/127*52,release=8+r/127*52,total=attack+decay+release+56,y=60-s/127*48;
    return `M0 60 L${attack/total*240} 12 L${(attack+decay)/total*240} ${y} H${(attack+decay+56)/total*240} L240 60`;
  }
  const extended=[2,4,5].includes(mode),width=x=>x===0?0:8+x/127*52;
  const stages=[{width:width(a),level:extended?v[p.base+1]/127:1,linear:true},{width:width(d),level:s/127}];
  if(extended)stages.push({width:width(v[p.base+2]),level:v[p.base+3]/127});
  if(mode===4)stages.push({width:width(d),level:s/127},{width:width(v[p.base+2]),level:v[p.base+3]/127});
  if(mode===1||mode===2)stages.push({width:48,level:stages.at(-1).level,linear:true});
  stages.push({width:width(r),level:0});
  const total=stages.reduce((sum,s)=>sum+s.width,0)||1;let x=0,level=0,path='M0 60';
  for(const stage of stages){
    for(let i=1;i<=12;i++){const t=i/12,shape=stage.linear?t:1-(1-t)**3;path+=` L${((x+stage.width*t)/total*240).toFixed(2)} ${(60-(level+(stage.level-level)*shape)*48).toFixed(2)}`;}
    x+=stage.width;level=stage.level;
  }
  return path;
}
