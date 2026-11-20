// #11415: borrowed Array methods use class length accessors and indexed properties.
class Bag {
  #n = 0;
  get length() { return this.#n; }
  set length(v: number) { this.#n = v; }
}
const b: any = new Bag();
console.log('push', Array.prototype.push.call(b, 'a', 'b'));
console.log('after push', b.length, b[0], b[1]);
console.log('pop', Array.prototype.pop.call(b), b.length);
console.log('splice', Array.prototype.splice.call(b, 0, 1, 'x', 'y').join(','));
console.log('after splice', b.length, b[0], b[1]);
console.log('unshift', Array.prototype.unshift.apply(b, ['z']));
console.log('shift', Array.prototype.shift.call(b), b.length, b[0], b[1]);
const borrowed = Array.prototype.push.bind(b);
console.log('bound', borrowed('q'), b.length, b[2]);
class Data { length = 0; }
const data: any = new Data();
console.log('data', Array.prototype.push.call(data, 10, 20), Array.prototype.pop.call(data), data.length, data[0]);
class Child extends Bag { push() { return 'own'; } }
const child: any = new Child();
console.log('override', Array.prototype.push.call(child, 'child'), child.push(), child.length, child[0]);
let gets = 0;
let sets = 0;
class Counted {
  n = 0;
  get length() { gets++; return this.n; }
  set length(value: number) { sets++; this.n = value; }
}
const counted: any = new Counted();
console.log('count push', Array.prototype.push.call(counted, 7), gets, sets);
console.log('count pop', Array.prototype.pop.call(counted), gets, sets);
const proto = { get length() { return this.n; }, set length(value: number) { this.n = value; } };
const inherited: any = Object.create(proto);
inherited.n = 1; inherited[0] = 'old';
console.log('inherited read', Array.prototype.join.call(inherited, ','), inherited.n);
class ReadOnly { get length() { return 0; } }
const readOnly: any = new ReadOnly();
try { Array.prototype.push.call(readOnly, 'written'); } catch (error: any) { console.log('readonly', error.name, readOnly[0], readOnly.length); }
const nullProto: any = Object.create(null);
console.log('missing', Array.prototype.push.call(nullProto, 'first'), nullProto[0], nullProto.length);
const arr = [1, 2];
console.log('array', Array.prototype.push.call(arr, 3), Array.prototype.pop.call(arr), arr.join(','));
