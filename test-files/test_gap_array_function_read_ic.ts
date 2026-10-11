// Array/function reads must invalidate warmed inherited and absent answers.
function ctor(v: any): any { return v.constructor; }
function marker(v: any): any { return v.arrayicMarker; }
function absent(v: any): any { return v.arrayicAbsent; }
function isBuffer(v: any): boolean {
  return !!(v.constructor && typeof v.constructor.isBuffer === 'function' && v.constructor.isBuffer(v));
}
const a: any = [1, 2];
const f: any = function named(a: any, b: any) {};
for (let i = 0; i < 20; i++) { ctor(a); marker(a); absent(a); absent(f); isBuffer(a); }
console.log(ctor(a) === Array, isBuffer(a), absent(a), absent(f));
a.constructor = { isBuffer(v: any) { return v === a; } };
console.log(isBuffer(a));
delete a.constructor;
console.log(ctor(a) === Array, isBuffer(a));
(Array.prototype as any).arrayicMarker = 17;
(Function.prototype as any).arrayicAbsent = 19;
console.log(marker(a), absent(f));
(Array.prototype as any).arrayicMarker = 23;
(Function.prototype as any).arrayicAbsent = 29;
console.log(marker(a), absent(f));
a.arrayicMarker = 31;
f.arrayicAbsent = 37;
console.log(marker(a), absent(f));
delete a.arrayicMarker;
delete f.arrayicAbsent;
console.log(marker(a), absent(f));
Object.defineProperty(a, 'arrayicMarker', { get() { return this.length + 40; }, configurable: true });
Object.defineProperty(f, 'arrayicAbsent', { get() { return this.length + 50; }, configurable: true });
console.log(marker(a), absent(f));
function length(v: any): any { return v.length; }
function name(v: any): any { return v.name; }
for (let i = 0; i < 20; i++) { length(a); length(f); name(f); }
console.log(length(a), length(f), name(f));
a.push(3);
Object.defineProperty(f, 'length', { value: 7, configurable: true });
Object.defineProperty(f, 'name', { value: 'renamed', configurable: true });
console.log(length(a), length(f), name(f));
const p: any = new Proxy(a, { get(target, key, receiver) { if (key === 'arrayicMarker') return 61; return Reflect.get(target, key, receiver); } });
for (let i = 0; i < 20; i++) marker(p);
console.log(marker(p), marker(a));
const pf: any = new Proxy(f, { get(target, key, receiver) { if (key === 'arrayicAbsent') return 83; return Reflect.get(target, key, receiver); } });
for (let i = 0; i < 20; i++) absent(pf);
console.log(absent(pf), absent(f));
const other: any = [];
for (let i = 0; i < 20; i++) { marker(other); absent(other); }
Object.setPrototypeOf(other, { arrayicMarker: 67, arrayicAbsent: 71 });
console.log(marker(other), absent(other));
const g: any = () => 0;
for (let i = 0; i < 20; i++) absent(g);
Object.setPrototypeOf(g, { arrayicAbsent: 73 });
console.log(absent(g));
const untouched: any = [];
for (let i = 0; i < 20; i++) absent(untouched);
const original = Object.getPrototypeOf(Array.prototype);
Object.setPrototypeOf(Array.prototype, { arrayicAbsent: 79 });
console.log(absent(a), absent(untouched));
Object.setPrototypeOf(Array.prototype, original);
delete (Array.prototype as any).arrayicMarker;
delete (Function.prototype as any).arrayicAbsent;
console.log(marker([]), absent(() => 0));
