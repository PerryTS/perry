function read(a: any, k: any) { return a[k]; }
function write(a: any, k: any, v: any) { return a[k] = v; }
const a: any = [10, 20, 30];
a.label = 'named';
Object.defineProperty(a, 'unrelated', { get() { return 77; } });
console.log(read(a, 0), write(a, 1, 22), read(a, 1), read(a, 'label'));
Object.defineProperty(a, '0', { get() { return 41; }, set(v) { console.log('set-index', v); }, configurable: true });
console.log(read(a, 0), write(a, 0, 12));
delete a[0];
const proto: any = Object.create(Array.prototype);
Object.defineProperty(proto, '0', { get() { return 91; }, configurable: true });
Object.setPrototypeOf(a, proto);
console.log(read(a, 0), read(a, 1), read(a, 100));
const forwarded: any = [1];
const saved: any = { a: forwarded };
for (let i = 1; i < 80; ++i) forwarded.push(i);
forwarded.label = true;
console.log(read(saved.a, 70), write(saved.a, 70, 700), read(saved.a, 70));
const o: any = { alpha: 1, beta: 2 };
for (const k of ['alpha', 'beta', 'new_name']) console.log(k, write(o, k, 42), read(o, k));
let n = 0;
const k: any = { toString() { n++; return 'alpha'; } };
console.log(write(o, k, 43), read(o, 'alpha'), n);
const sym = Symbol('CK');
console.log(write(o, sym, 7), read(o, sym));
Object.defineProperty(o, 'beta', { writable: false });
try { write(o, 'beta', 99); } catch(e) { console.log('readonly', e instanceof TypeError); }
console.log(read(o, 'alpha'), read(o, 'beta'));
Object.freeze(o);
try { write(o, 'alpha', 99); } catch(e) { console.log('frozen', e instanceof TypeError); }
console.log(read(o, 'alpha'));
const ta: any = new Uint16Array([11, 22]);
console.log(read(ta, 0), write(ta, 1, 65537), read(ta, 1), read(ta, 8));
console.log(write(ta, 'named', 5), read(ta, 'named'));
class Point { x = 13; y = 29; }
function finite(p: Point, b: boolean) { return p[b ? 'x' : 'y']; }
console.log(finite(new Point(), true), finite(new Point(), false));
export {};
