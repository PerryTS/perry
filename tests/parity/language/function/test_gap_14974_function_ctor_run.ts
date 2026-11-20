'use strict';
// A function constructor's run of `this` stores writes its birth layout's
// slots in place only while nothing on the chain intercepts a key: prototype
// setters, getters and non-writables installed before or during a run,
// borrowed and repeated calls, non-extensible receivers, a replaced
// prototype, early returns, throws and Reflect.construct.
const log: string[] = [];
function F(this: any, a: any, b: any) { this.x = a; this.y = this.z = b; this.w = 0; }
const f1 = new (F as any)(1, 2);
log.push(JSON.stringify(f1));
Object.defineProperty(F.prototype, 'z', { set(v) { log.push('set z ' + v); }, get() { return 'gz'; }, configurable: true });
const f2 = new (F as any)(3, 4);
log.push(JSON.stringify(f2) + ' z=' + f2.z + ' own=' + Object.hasOwn(f2, 'z'));
delete F.prototype.z;
Object.defineProperty(F.prototype, 'w', { value: 9, writable: false, configurable: true });
try { new (F as any)(5, 6); log.push('no throw'); } catch (e: any) { log.push(e.constructor.name); }
delete F.prototype.w;
const o: any = { q: 1 };
F.call(o, 7, 8);
log.push(JSON.stringify(o));
F.call(f1, 10, 11);
log.push(JSON.stringify(f1) + Object.keys(f1));
const fz = Object.preventExtensions({});
try { F.call(fz, 1, 2); log.push('ext ok'); } catch (e: any) { log.push('ext ' + e.constructor.name); }
F.prototype = { get y() { return 'gy'; }, set y(v) { log.push('set y ' + v); } };
const f3 = new (F as any)(1, 2);
log.push(JSON.stringify(f3) + ' y=' + f3.y + ' own=' + Object.hasOwn(f3, 'y'));
let n = 0;
function G(this: any, o: any) { this.a = 1; this.b = o.v; this.c = 3; if (n++ > 1) return; this.d = 4; }
const g1 = new (G as any)({ get v() { Object.defineProperty(G.prototype, 'c', { set(v) { log.push('set c ' + v); }, configurable: true }); return 2; } });
log.push(JSON.stringify(g1));
delete G.prototype.c;
for (let i = 0; i < 4; i++) log.push(Object.keys(new (G as any)({ v: i })).join(''));
function H(this: any, t: () => number) { this.a = 1; this.b = t(); this.c = 3; }
try { new (H as any)(() => { throw new Error('boom'); }); } catch (e: any) { log.push(e.message); }
log.push(JSON.stringify(new (H as any)(() => 2)));
function P() {}
P.prototype.marker = 1;
const r: any = Reflect.construct(F, [1, 2], P);
log.push(JSON.stringify(r) + r.marker);
let s = 0;
for (let i = 0; i < 100000; i++) { const k = new (G as any)({ v: i }); s += k.b; }
log.push(String(s));
console.log(log.join('\n'));
