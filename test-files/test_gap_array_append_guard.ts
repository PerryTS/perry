'use strict';
function append(a: any, v: any) { a[a.length] = v; return a.length; }
function put(a: any, k: any, v: any) { a[k] = v; }
function push(a: any, v: any) { return a.push(v); }
function attempt(a: any) {
    try { append(a, 99); console.log('write', a.length); }
    catch (e) { console.log('write', e instanceof TypeError); }
    try { push(a, 98); console.log('push', a.length); }
    catch (e) { console.log('push', e instanceof TypeError); }
}
const a: any = [];
for (let i = 0; i < 65; i++) append(a, i);
console.log('grown', a.length, a[0], a[64]);
let s = 'abcd'; s += 'efgh';
append(a, s); s += 'changed';
console.log('string', a[65], s);
append(a, { value: 17 });
console.log('mixed', a.length, a[66].value);
const holey: any = new Array(3);
append(holey, 4);
console.log('holes', holey.length, 0 in holey, 3 in holey, holey[3]);
attempt(Object.freeze([1]));
attempt(Object.seal([1]));
attempt(Object.preventExtensions([1]));
const fixed: any = [1];
Object.defineProperty(fixed, 'length', { writable: false }); attempt(fixed);
const typed: any = new Uint8Array([1, 2]);
console.log('typed', append(typed, 9), typed[0], typed[1], typed[2]);
class Sub extends Array {}
const sub: any = new Sub();
console.log('sub', append(sub, 3), push(sub, 4), sub[0], sub[1], sub instanceof Sub);
let reads = 0;
const obj: any = { get length() { reads++; return 2; } };
console.log('getter', append(obj, 7), reads, obj[2]);
const custom: any = [];
let seen = 0;
Object.setPrototypeOf(custom, { set 0(v) { seen = v; } });
console.log('custom', append(custom, 12), seen, Object.hasOwn(custom, '0'));
const keys: any = [];
put(keys, '0', 1); put(keys, -1, 2); put(keys, 1.5, 3);
console.log('keys', keys.length, keys[0], keys[-1], keys[1.5]);
function effectStore(a: any, f: any) { a[a.length] = f(); }
const changed: any = [];
effectStore(changed, () => { changed.push(11); return 22; });
console.log('rhs length', changed.length, changed[0]);
const grew: any = [];
effectStore(grew, () => { for (let i = 0; i < 33; i++) grew.push(i); return 44; });
console.log('rhs grow', grew.length, grew[0], grew[32]);
const restricted: any = [1];
try { effectStore(restricted, () => { Object.freeze(restricted); return 2; }); }
catch (e) { console.log('rhs frozen', e instanceof TypeError, restricted.length); }
// A prototype mutation after a hot append loop must stand down the append arm.
const intercepted: any = [];
Object.defineProperty(Array.prototype, '0', { set(v) { seen = v; }, configurable: true });
append(intercepted, 31);
const got = seen; const len = intercepted.length; const own = Object.hasOwn(intercepted, '0');
delete (Array.prototype as any)[0];
console.log('prototype', got, len, own);
console.log('restored', append(intercepted, 32), intercepted[0]);
