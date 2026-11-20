function read(o) { return o.a; }
function keyed(o, k) { return o[k]; }
function same(label, got, want) {
  if (got !== want) throw new Error(label + ': ' + got + ' !== ' + want);
  console.log(label, got);
}

let hits = 0;
let sets = 0;
let own = { marker: 11, get a() { hits++; return this.marker; }, set a(v) { sets += v; } };
same('own-get', read(own), 11);
own.a = 3;
same('own-set', sets, 3);
const proto = { get a() { hits++; return this.marker + 1; }, set a(v) { this.marker = v; } };
let inherited = Object.create(proto);
inherited.marker = 20;
same('proto-get', read(inherited), 21);
inherited.a = 30;
same('proto-set', read(inherited), 31);
same('getter-count', hits, 3);

let changing = { a: 1 };
let sum = 0;
for (let i = 0; i < 12; i++) {
  if (i === 3) Object.defineProperty(changing, 'a', { get() { return 2; }, configurable: true });
  if (i === 7) Object.defineProperty(changing, 'a', { value: 3, writable: true, configurable: true });
  sum += read(changing);
}
same('accessor-data', sum, 26);
const parent = { a: 9 };
const deleted = Object.create(parent);
deleted.a = undefined;
same('real-undefined', read(deleted), undefined);
deleted.a = 1;
sum = 0;
for (let i = 0; i < 10; i++) { if (i === 4) delete deleted.a; sum += read(deleted); }
same('delete', sum, 58);
const attrs = {};
Object.defineProperty(attrs, 'a', { value: 7, writable: false, enumerable: false, configurable: true });
same('read-only-set', Reflect.set(attrs, 'a', 8), false);
same('non-writable', read(attrs), 7);
same('non-enumerable', Object.keys(attrs).length, 0);

let traps = 0;
const proxy = new Proxy({ a: 4 }, { get(t, k, r) { traps++; return Reflect.get(t, k, r); } });
same('proxy', read(proxy), 4);
same('proxy-traps', traps, 1);
const root = {};
const middle = Object.create(root);
const child = Object.create(middle);
let text = '';
for (let i = 0; i < 8; i++) {
  if (i === 2) root.a = 5;
  if (i === 4) middle.a = 6;
  if (i === 6) delete middle.a;
  text += String(read(child)) + ',';
}
same('prototype-mutation', text, 'undefined,undefined,5,5,6,6,5,5,');

for (const p of [String.prototype, Number.prototype, Boolean.prototype, Symbol.prototype]) {
  Object.defineProperty(p, 'a', { get: function() { 'use strict'; return typeof this; }, configurable: true });
}
same('string-primitive', read('long primitive string'), 'string');
same('number-primitive', read(12), 'number');
same('zero-primitive', read(0), 'number');
same('boolean-primitive', read(false), 'boolean');
same('symbol-primitive', read(Symbol('receiver')), 'symbol');
for (const p of [String.prototype, Number.prototype, Boolean.prototype, Symbol.prototype]) delete p.a;
same('string-length', keyed('abc', 'length'), 3);
same('string-index', keyed('abc', '1'), 'b');
same('string-nonindex', keyed('abc', '01'), undefined);
let conversions = 0;
const indexKey = { toString() { conversions++; return '1'; } };
same('converted-index', keyed('abc', indexKey), 'b');
same('key-converted-once', conversions, 1);

const arrayProto = Object.create(Array.prototype);
arrayProto[1] = 8;
const array = [1, , 3];
Object.setPrototypeOf(array, arrayProto);
array.a = 17;
same('array-own', read(array), 17);
same('array-hole', keyed(array, '1'), 8);
same('array-absent', keyed(array, '10'), undefined);
Object.defineProperty(array, 'a', { get() { return 19; }, configurable: true });
same('array-getter', read(array), 19);

for (const value of [null, undefined]) {
  try { read(value); } catch (error) { console.log('nullish', error.name, error.message); }
}
const dictionary = {};
for (let i = 0; i < 160; i++) dictionary['key' + i] = i;
dictionary.a = 33;
for (let i = 0; i < 80; i++) delete dictionary['key' + i];
same('dictionary', read(dictionary), 33);
class Many { constructor(n) { for (let i = 0; i < n; i++) this['k' + i] = i; this.a = n; } }
sum = 0;
for (let i = 0; i < 20; i++) sum += read(new Many(i));
same('class-layouts', sum, 190);

const large = JSON.parse('{\"k0\":0,\"k1\":1,\"k2\":2,\"k3\":3,\"k4\":4,\"k5\":5,\"k6\":6,\"k7\":7,\"a\":41}');
same('large-layout', read(large), 41);
same('duplicate-key', read({a: 1, a: 2}), 2);

const methodReceiver = { marker: 7, m(n) { return this.marker + n; } };
function replaceMethod() { methodReceiver.m = function() { return 99; }; return 3; }
same('lookup-before-arguments', methodReceiver.m(replaceMethod()), 10);
