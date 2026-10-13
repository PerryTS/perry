// #12531: retain prototypes through their owners, including after forced GC.
declare function gc(): void;
function collect() { if (typeof gc === 'function') { gc(); gc(); } }
function factory(n: number): any { return function (this: any) { this.n = n; }; }
const f = factory(7);
const original = f.prototype;
console.log('stable', original === f.prototype, original.constructor === f);
collect();
console.log('gc identity', original === f.prototype, original.constructor === f);
f.prototype.value = function (this: any) { return this.n; };
const first = new f();
collect();
console.log('new', first instanceof f, Object.getPrototypeOf(first) === f.prototype, first.value(), first.constructor === f);
const other = { tag: 42, constructor: f };
f.prototype = other;
const second = new f();
collect();
console.log('replace', f.prototype === other, !(first instanceof f), second instanceof f, Object.getPrototypeOf(first) === original, Object.getPrototypeOf(second) === other, second.tag);
const functions: any[] = [];
for (let i = 0; i < 12; i++) functions.push(factory(i));
// Only the function array retains these prototypes across the collection.
for (let i = 0; i < functions.length; i++) functions[i].prototype.token = i;
collect();
let count = 0;
for (let i = 0; i < functions.length; i++) {
  const ctor = functions[i];
  const instance = new ctor();
  if (instance.n === i && ctor.prototype.token === i && ctor.prototype.constructor === ctor && Object.getPrototypeOf(instance) === ctor.prototype && instance instanceof ctor) count++;
}
console.log('loop owners', count);
function Base(this: any) { this.base = 9; }
(Base as any).prototype.method = function (this: any) { return this.base; };
class Derived extends (Base as any) {}
const derived = new Derived();
collect();
console.log('heritage', derived instanceof Base, derived instanceof Derived, (derived as any).method(), Object.getPrototypeOf(Derived.prototype) === (Base as any).prototype);
const bound = f.bind(null);
const arrow: any = () => 1;
async function asyncFunction() { return 1; }
function* generator() { yield 1; }
async function* asyncGenerator() { yield 1; }
const gp = generator.prototype;
const agp = asyncGenerator.prototype;
collect();
console.log('kinds', !Object.hasOwn(bound, 'prototype'), bound.prototype === undefined, !Object.hasOwn(arrow, 'prototype'), arrow.prototype === undefined, !Object.hasOwn(asyncFunction, 'prototype'), (asyncFunction as any).prototype === undefined, gp === generator.prototype, agp === asyncGenerator.prototype);
class A { m() { return 11; } }
class B extends A {}
const b = new B();
collect();
console.log('classes', b instanceof B, b instanceof A, b.m(), Object.getPrototypeOf(b) === B.prototype, b.constructor === B);
