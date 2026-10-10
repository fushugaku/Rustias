// Browser capacity. Original desktop programs retain four timbres/six routes.
export const MAX_TIMBRES=8;
export const INITIAL_TIMBRES=4;
export const PATCH_ROUTES=8;
export const LFO3_BASE=163;
export const PARAMETER_COUNT=LFO3_BASE+8;
export const MASTER_EFFECT_SLOT=2*MAX_TIMBRES;
export const TIMBRE_EFFECTS=4;
export const EXTRA_EFFECT_START=MASTER_EFFECT_SLOT+1;
export const EFFECT_SLOTS=EXTRA_EFFECT_START+2*MAX_TIMBRES;
export const timbreEffectSlot=(timbre,role)=>role<2?2*timbre+role:EXTRA_EFFECT_START+2*timbre+role-2;
export const timbreEffectSlots=timbre=>Array.from({length:TIMBRE_EFFECTS},(_,role)=>timbreEffectSlot(timbre,role));
export const effectUsesMaster=slot=>slot===MASTER_EFFECT_SLOT||slot>=EXTRA_EFFECT_START&&slot<EFFECT_SLOTS;
export const effectTimbre=slot=>slot===MASTER_EFFECT_SLOT?null:slot<MASTER_EFFECT_SLOT?Math.floor(slot/2):Math.floor((slot-EXTRA_EFFECT_START)/2);
export const effectRole=slot=>slot===MASTER_EFFECT_SLOT?TIMBRE_EFFECTS:slot<MASTER_EFFECT_SLOT?slot%2:2+(slot-EXTRA_EFFECT_START)%2;
export const timbreArray=(value=0)=>Array(MAX_TIMBRES).fill(value);
