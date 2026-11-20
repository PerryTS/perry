// Primitive fast paths and the observable conversions they must fall through to.
function show(v: any): string {
  if (typeof v === 'number') {
    if (Object.is(v, -0)) return '-0';
    if (Number.isNaN(v)) return 'NaN';
  }
  return String(v);
}
function add(a: any, b: any): any { return a + b; }
function coerce(a: any): number { return Number(a); }
for (const v of [0, -0, -12, 1.25, 2147483648, NaN, Infinity, -Infinity]) {
  console.log(show(v), show(add(v, -0)), show(coerce(v)));
}
for (const v of [undefined, null, true, false, '12', '', 'bad', 123n]) {
  console.log(show(v), show(coerce(v)));
}
console.log(add('prefix', 12), show(add(12n, 3n)));
try { add(1n, 1); } catch (e) { console.log(e instanceof TypeError); }
try { coerce(Symbol('x')); } catch (e) { console.log(e instanceof TypeError); }
let events = '';
const a = { valueOf() { events += 'a'; return 7; } };
const b = { valueOf() { events += 'b'; return 9; } };
console.log(add(a, b), events);
events = '';
const c = { [Symbol.toPrimitive](hint: string) { events += hint; return '4'; } };
console.log(add(c, 3), coerce(c), events);
class Base {
  x: any;
  constructor(x: any) { this.x = x; }
  value(): any { return this.x; }
  sum(): any { return this.value() + this.x; }
}
class Derived extends Base { value(): any { return 2; } }
const d = new Derived(5);
console.log(d.sum());
d.x = 's';
console.log(d.sum());
Derived.prototype.value = function() { return 3; };
console.log(d.sum());
(Base as any).valueOf = function() { return 17; };
console.log(coerce(Base), add(Base, 1));
