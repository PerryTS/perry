import { CallbackBase, CallbackDerived, setCallback } from './fixtures/class_virtual/callback_defs.ts';
function invoke(cs: CallbackBase[]): number { return cs[0].step(2); }
let reads = 0;
const base: CallbackBase[] = [new CallbackBase(3)];
const derived: CallbackBase[] = [new CallbackDerived(5)];
let current: CallbackBase = base[0];
setCallback(() => { Object.defineProperty(current, 'scale', { configurable: true, get() { reads++; return 7; } }); });
const b = invoke(base); current = derived[0];
console.log('callback-getter', b, invoke(derived), reads);
