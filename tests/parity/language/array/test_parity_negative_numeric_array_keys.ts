// #13687: the absent-key proof must be withdrawn by every observable mutation.
// Run this with Node 26.12.1 and a compiled Perry executable and compare stdout.
function read(a: number[], k: any): any { return a[k]; }
function show(a: number[], k: any): void { console.log(read(a, k)); }
const a: number[] = [11, 22, 33];
show(a, -1);
show(a, -0);
show(a, -0.5);
show(a, -4294967296);
show(a, -1e21);
show(a, 4294967295);
show(a, 9);
delete a[1];
show(a, 1);
(a as any)[-1] = 41;
show(a, -1);
delete (a as any)[-1];
show(a, -1);
let gets = 0;
Object.defineProperty(a, '-1', { configurable: true, get() { gets++; return this[0] + 40; } });
show(a, -1);
console.log(gets);
delete (a as any)[-1];
let conversions = 0;
const key = { toString() { conversions++; return '-1'; } };
show(a, key);
console.log(conversions);
const symbol = Symbol('negative');
(a as any)[symbol] = 61;
show(a, symbol);
const custom: any = { '-1': 71 };
Object.setPrototypeOf(a, custom);
show(a, -1);
custom[-1] = 72;
show(a, -1);
Object.setPrototypeOf(a, Array.prototype);
show(a, -1);
(Array.prototype as any)[-1] = 81;
show(a, -1);
(Array.prototype as any)[-1] = 82;
show(a, -1);
delete (Array.prototype as any)[-1];
show(a, -1);
Object.defineProperty(Array.prototype, '-1', { configurable: true, get() { return this[0] + 80; } });
show(a, -1);
delete (Array.prototype as any)[-1];
(Object.prototype as any)[-1] = 101;
show(a, -1);
delete (Object.prototype as any)[-1];
Object.defineProperty(Object.prototype, '-1', { configurable: true, get() { return this[0] + 100; } });
show(a, -1);
delete (Object.prototype as any)[-1];
show(a, -1);
// Erased declared array receivers, and Proxy traps, cannot use the proof.
show(({ '-1': 121 } as any), -1);
show(('abc' as any), -1);
const proxy = new Proxy([11], { get(target, k) { return k === '-1' ? 131 : Reflect.get(target, k); } });
show(proxy, -1);
// Large negative own keys preserve the existing generic key conversion.
(a as any)[-4294967296] = 141;
show(a, -4294967296);
(a as any)[-0.5] = 151;
show(a, -0.5);
