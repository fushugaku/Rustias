// Native Rust reads these base values directly after each control message.
// Their movement must not rebuild modulation clocks or resend the whole program.
const directValues=new Set(['synth','global','drum','sample']);
export function createPluginAutomationSync(post){
  let previous;
  return {
    reset(){previous=undefined;},
    send({config,macros,targets,effects,circuits}){
      const slots=[],chains=[],graphs=[];
      for(const {target} of targets){
        if(target.kind==='effect')slots[target.slot]=effects.slots[target.slot];
        else if(target.kind==='chain-effect'){
          const chain=chains[target.timbre]??=[];
          const program=effects.chains?.[target.timbre]?.find(fx=>fx.id===target.id);
          if(program&&!chain.some(fx=>fx.id===program.id))chain.push(program);
        }else if(target.kind==='module')graphs[target.timbre]=circuits[target.timbre];
      }
      const state={config,macros,targets,effects:{slots,chains},circuits:graphs};
      const identity=JSON.stringify({...state,
        targets:targets.map(({base,...spec})=>directValues.has(spec.target.kind)?spec:{...spec,base}),
        macros:{...macros,bases:macros.bases.map(({value,...base})=>directValues.has(base.target.kind)?base:{...base,value})},
      });
      if(identity===previous)return false;
      post({type:'modulation',state});previous=identity;return true;
    },
  };
}
