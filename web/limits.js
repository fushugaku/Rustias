// Browser capacity. Original desktop programs retain four timbres/six routes.
export const MAX_TIMBRES=8;
export const INITIAL_TIMBRES=4;
export const PATCH_ROUTES=8;
export const MASTER_EFFECT_SLOT=2*MAX_TIMBRES;
export const EFFECT_SLOTS=MASTER_EFFECT_SLOT+1;
export const timbreArray=(value=0)=>Array(MAX_TIMBRES).fill(value);
