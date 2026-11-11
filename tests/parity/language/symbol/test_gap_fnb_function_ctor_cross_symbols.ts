import keysA from './test_gap_fnb_keys_a.cjs';
import keysB from './test_gap_fnb_keys_b.cjs';
const { unique, alias, registry } = keysA;
const { same, distinct, registry: otherRegistry } = keysB;
function First(this: any) { this.id = 1; this[unique] = 2; this[registry] = 3; }
function Second(this: any) { this.id = 4; this[same] = 5; this[distinct] = 6; this[otherRegistry] = 7; }
const x = new First(), y = new Second();
console.log('identity', unique === alias, unique === same, distinct === unique, registry === otherRegistry);
console.log('values', x[same], x[distinct], y[unique], y[distinct], x[otherRegistry], y[registry]);
console.log('order', Object.getOwnPropertySymbols(x).map(k => k === unique ? 'u' : 'r').join(','), Object.getOwnPropertySymbols(y).map(k => k === unique ? 'u' : k === distinct ? 'd' : 'r').join(','));
delete y[same]; y[unique] = 9;
console.log('reinsert', Object.getOwnPropertySymbols(y).map(k => k === unique ? 'u' : k === distinct ? 'd' : 'r').join(','));
