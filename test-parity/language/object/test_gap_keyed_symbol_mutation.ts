const S = Symbol('answer');
const other = Symbol('answer');
function read(o: any, k: any): any { return o[k]; }
function typed(o: any, k: symbol): any { return o[k]; }
function show(o: any, k: any): void { for (let i = 0; i < 20; i++) read(o, k); console.log(read(o, k), typed(o, k)); }
const p: any = { [S]: 17, answer: 71 };
const mid: any = Object.create(p); mid.pad = 1;
const o: any = Object.create(mid); o.pad = 2;
show(o, S); p[S] = 18; show(o, S);
o[S] = 19; show(o, S); delete o[S]; show(o, S);
delete p[S]; show(o, S); mid[S] = 21; show(o, S);
console.log(read(o, 'answer'), read(o, other));
let calls = 0;
Object.defineProperty(mid, S, { configurable: true, get() { calls++; return this.pad + 30; } });
console.log(read(o, S), read(o, S), calls);
Object.defineProperty(mid, S, { configurable: true, value: 41, writable: false }); show(o, S);
Object.setPrototypeOf(o, { [S]: 51 }); show(o, S);
const prox = new Proxy({ [S]: 61 }, { get(t, k, receiver) { return Reflect.get(t, k, receiver) + receiver.pad; } });
Object.setPrototypeOf(o, prox); show(o, S);
console.log(Reflect.get(prox, S, { pad: 3 }));
Object.setPrototypeOf(o, null); show(o, S);
const deep: any = Object.create(Object.create(Object.create({})));
show(deep, S); Object.getPrototypeOf(Object.getPrototypeOf(deep))[S] = 81; show(deep, S);
const a: any = { [S]: 91 }; const b: any = { [S]: 92 }; show(a, S); show(b, S);
class C { static [S]() { return 101; } [S]() { return 102; } }
console.log(read(C, S)(), read(new C(), S)());
console.log(typeof read([], Symbol.iterator), typeof read(new Map(), Symbol.iterator));
// A symbol annotation cannot make a primitive receiver a heap pointer.
console.log(typed('abc', 'length' as any), typed(17, S));
try { typed(null, S); } catch (e) { console.log(e instanceof TypeError); }
