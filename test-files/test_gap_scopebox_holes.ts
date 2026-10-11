"use strict";
function fill(out: any[], value: any) { out[0] = value; out[1] = value; }
const plain = new Array(3);
fill(plain, 7);
console.log(plain.length, plain[0], plain[1], 2 in plain);
// Exercise hole fills before any process-wide prototype invalidation.
const pointerSlots = new Array(2);
const payload = { value: 'hole-payload' };
fill(pointerSlots, payload);
console.log(pointerSlots[0] === payload, pointerSlots[1] === payload);
// Logical length does not prove that the backing slot exists.
const pristineSparse = new Array(1000000000);
pristineSparse[64] = 17;
console.log(pristineSparse.length, pristineSparse[64], 63 in pristineSparse);
const custom = new Array(2);
let calls = 0;
const proto = Object.create(Array.prototype);
Object.defineProperty(proto, '0', { set(v) { calls += v; }, get() { return 90; }, configurable: true });
Object.setPrototypeOf(custom, proto);
fill(custom, 3);
console.log(calls, custom[0], custom[1], Object.hasOwn(custom, '0'));
const inherited = new Array(2);
Object.defineProperty(Array.prototype, '0', { set(v) { calls += v; }, get() { return 80; }, configurable: true });
fill(inherited, 4);
delete Array.prototype[0];
console.log(calls, inherited[0], inherited[1], Object.hasOwn(inherited, '0'));
const frozen = Object.freeze(new Array(2));
try { fill(frozen as any[], 5); } catch (e) { console.log(e instanceof TypeError); }
const length = new Array(2); length.length = 0;
fill(length, 8);
console.log(length.length, length[0], length[1]);
const lied: any = { 0: 1, 1: 2 }; fill(lied, 9);
console.log(lied[0], lied[1]);
const sparse = new Array(1000000000);
fill(sparse, 6);
console.log(sparse.length, sparse[0], sparse[1], 2 in sparse);
const fractional: any[] = new Array(2);
fractional[0.5] = 12;
fractional[-1] = 13;
console.log(fractional.length, fractional[0.5], fractional[-1], 0 in fractional);
