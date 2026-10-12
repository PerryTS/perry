// Fixed bind births: metadata snapshots, argument order, construct and instanceof.
function combine(this: any, a: number, b: number, c: number) {
  return this.base + a * 100 + b * 10 + c;
}
const receiver = { base: 7 };
const f: any = combine;
for (const prefix of [[], [1], [1, 2], [1, 2, 3], [1, 2, 3, 4], [1, 2, 3, 4, 5]]) {
  const b = f.bind(receiver, ...prefix);
  console.log(b.name, b.length, b(1, 2, 3), Object.hasOwn(b, 'prototype'));
}
const first = f.bind(receiver, 1);
const twice = first.bind({ base: 999 }, 2);
console.log(twice.name, twice.length, twice(3), twice.call({ base: 0 }, 3));
Object.defineProperty(f, 'name', { value: 'changed', configurable: true });
Object.defineProperty(f, 'length', { value: 99, configurable: true });
console.log(first.name, first.length, twice.name, twice.length);
const changed = f.bind(receiver, 1, 2);
console.log(changed.name, changed.length);
Object.defineProperty(f, 'length', { get() { console.log('get length'); return 5; }, configurable: true });
Object.defineProperty(f, 'name', { get() { console.log('get name'); return 'observed'; }, configurable: true });
const observed = f.bind(receiver, 1);
console.log(observed.name, observed.length);
Object.defineProperty(f, 'name', { get() { throw new Error('name getter'); }, configurable: true });
try { f.bind(receiver); } catch (e: any) { console.log(e.message); }
class Record {
  value: number;
  constructor(a: number, b: number) { this.value = a * 10 + b; }
}
const C: any = Record;
const B = C.bind({ value: -1 }, 3);
const BB = B.bind(null, 4);
const one = new B(5);
const two = new BB();
console.log(B.name, B.length, BB.name, BB.length);
console.log(one.value, two.value, one instanceof Record, one instanceof B, two instanceof BB);
console.log(Object.getPrototypeOf(one) === Record.prototype, Object.hasOwn(B, 'prototype'));
function Plain(this: any, a: number, b: number) { this.value = a * 10 + b; }
const P: any = Plain;
const PB = P.bind({ value: -1 }, 6).bind(null, 7);
const p = new PB();
console.log(p.value, p instanceof P, p instanceof PB);
const custom = { label: 'custom' };
function protoTarget() {}
Object.setPrototypeOf(protoTarget, custom);
const protoBound = Function.prototype.bind.call(protoTarget, null);
console.log(Object.getPrototypeOf(protoBound) === custom);
const n: any = (() => 1).bind(null);
try { new n(); } catch (e: any) { console.log(e instanceof TypeError); }
function optional(a: number, b = 2, ...rest: number[]) { return a + b + rest.length; }
const optionalBound = optional.bind(null, 3);
console.log(optionalBound.length, optionalBound(4, 5, 6));
async function asyncTarget(a: number) { return a; }
const asyncBound = asyncTarget.bind(null, 3);
console.log(asyncBound.name, asyncBound.length, Object.getPrototypeOf(asyncBound) === Object.getPrototypeOf(asyncTarget));
