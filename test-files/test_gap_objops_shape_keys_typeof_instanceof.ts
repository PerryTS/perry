function kind(v: any): string { return typeof v; }
function inst(v: any, c: any): boolean { return v instanceof c; }
function keys(v: any): string { return Object.keys(v).join(','); }
const o: any = { z: 1, '10': 10, '2': 2, a: 3, '01': 1 };
const sym = Symbol('hidden');
o[sym] = 4;
console.log('bulk-order', keys(o));
let calls = 0;
Object.defineProperty(o, 'get', { get() { calls++; return 9; }, enumerable: true });
console.log('bulk-getter', keys(o), calls);
Object.defineProperty(o, 'secret', { value: 5, enumerable: false });
console.log('keys', keys(o), calls);
console.log('fresh', Object.keys(o) === Object.keys(o));
const first = Object.keys(o); first.reverse(); first.push('extra');
console.log('stable', keys(o), o.z, o.a);
const d: any = Object.create(null); d.c = 1; d.b = 2; delete d.c; d.c = 3;
console.log('null', keys(d));
const dict: any = {}; for (let i = 0; i < 80; i++) dict['k' + i] = i;
delete dict.k2; Object.defineProperty(dict, 'k3', { enumerable: false });
console.log('dict', Object.keys(dict).length, Object.keys(dict)[0], Object.keys(dict)[77]);
console.log('freeze', keys(Object.freeze({ b: 2, a: 1 })));
console.log('exotic', keys([1, , 3]), keys(new Uint8Array([1, 2])), keys(new Map()), keys(new Set()));
class List extends Array {}
const list = new List(); list.push(1, 2); (list as any).extra = 3; delete list[0];
console.log('subclass', keys(list), keys(new String('abc')));
const px = new Proxy({ a: 1, b: 2 }, { ownKeys() { return ['b', 'a']; } });
console.log('proxy', keys(px));
class C {}
function classValue(): any { return class {}; }
console.log('class-expression', kind(classValue()));
const rv = Proxy.revocable(function () {}, {});
const callable = new Proxy(function () {}, {});
const plain = new Proxy({}, {});
console.log('types', kind({}), kind([]), kind(new Date()), kind(new WeakMap()), kind(C), kind(callable), kind(plain), kind(Symbol()), kind(Symbol.for('x')), kind(Symbol.iterator));
rv.revoke(); console.log('revoked', kind(rv.proxy));
console.log('all-like', kind({ [Symbol.toStringTag]: 'HTMLAllCollection' }));
for (const ctor of [Date, RegExp, Map, Set, Array, Error, Uint8Array]) {
    console.log('prototype', inst(Object.create(ctor.prototype), ctor), inst({}, ctor), inst(3, ctor));
}
const date = Date;
Object.defineProperty(date, 'name', { value: 'Renamed' });
console.log('rename', inst(new Date(0), date));
Object.defineProperty(date, Symbol.hasInstance, { configurable: true, value(v: any) { return v === 7; } });
console.log('override', inst(7, date), inst(new Date(0), date));
delete (date as any)[Symbol.hasInstance];
let hooks = 0;
Object.defineProperty(date, Symbol.hasInstance, { configurable: true, get() { hooks++; return undefined; } });
console.log('getter', inst(new Date(0), date), hooks);
delete (date as any)[Symbol.hasInstance];
Object.defineProperty(date, Symbol.hasInstance, { configurable: true, value: 1 });
try { inst({}, date); } catch(e) { console.log('noncallable', e instanceof TypeError); }
delete (date as any)[Symbol.hasInstance];
const bound = Date.bind(null);
console.log('bound', inst(new Date(0), bound), inst({}, bound));
