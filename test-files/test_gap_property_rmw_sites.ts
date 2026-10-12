// Read-modify-write must preserve Reference identity, call order and ToNumeric.
class Counter {
  value: any = 7;
  add(n: number) { this.value += n; return this.value++; }
}
const c = new Counter();
console.log(c.value++, ++c.value, c.value--, --c.value, c.add(4), c.value);
c.value = '12';
console.log(c.value++, typeof c.value, --c.value);
c.value = 12n;
console.log(String(c.value++), String(++c.value), String(c.value--), String(--c.value));
const o: any = { x: 8 };
o.x += 2; o.x -= 1; o.x *= 3; o.x /= 9; o.x %= 2; o.x **= 3;
o.x |= 4; o.x &= 5; o.x ^= 2; o.x <<= 2; o.x >>= 1; o.x >>>= 1;
console.log(o.x);
o.x = 'a'; o.x += 2; console.log(o.x);
let trace = '';
let value: any = '5';
const accessed: any = {};
Object.defineProperty(accessed, 'x', {
  configurable: true,
  get() { trace += 'get;'; return value; },
  set(v: any) { trace += 'set:' + String(v) + ';'; value = v; }
});
function base() { trace += 'base;'; return accessed; }
function key() { trace += 'key;'; return 'x'; }
function rhs() { trace += 'rhs;'; return 3; }
console.log(base()[key()]++, trace); trace = '';
console.log(++base()[key()], trace); trace = '';
console.log(base()[key()] += rhs(), trace); trace = '';
value = 2;
accessed.x += 1;
console.log(value, trace); trace = '';
value = 0;
console.log(accessed.x &&= rhs(), trace); trace = '';
console.log(accessed.x ||= rhs(), trace); trace = '';
console.log(accessed.x ??= rhs(), trace); trace = '';
value = null;
console.log(accessed.x ??= rhs(), trace); trace = '';
value = 9n;
console.log(String(accessed.x++), String(value), trace); trace = '';
value = { valueOf() { trace += 'numeric;'; return 20; } };
console.log(accessed.x++, value, trace); trace = '';
let target: any = { x: 10 };
const original = target;
console.log(target.x += (target = { x: 100 }, 2), original.x, target.x);
// A RHS can reshape the target between GetValue and PutValue.
const changed: any = { x: 1 };
function reshape() {
  Object.defineProperty(changed, 'x', { set(v) { trace += 'newset:' + v; }, configurable: true });
  return 4;
}
changed.x += reshape();
console.log(trace);
// Same update site, changing representations and descriptors: negative control.
function step(p: any) { return p.x++; }
const n: any = { x: 4 };
console.log(step(n), n.x); n.x = '8'; console.log(step(n), n.x);
n.x = 9n; console.log(String(step(n)), String(n.x));
Object.defineProperty(n, 'x', { get() { trace = 'read'; return 30; }, set(v) { trace += ':' + v; }, configurable: true });
console.log(step(n), trace);

const bigObject: any = { x: { valueOf() { return 40n; } } };
console.log(String(bigObject.x++), String(bigObject.x));
const boxedBig: any = { x: Object(50n) };
console.log(String(++boxedBig.x), String(boxedBig.x));
const power: any = { x: -1 };
power.x **= Infinity; console.log(Number.isNaN(power.x));
power.x = 1; power.x **= NaN; console.log(Number.isNaN(power.x));
const zero: any = { x: -0 };
console.log(Object.is(zero.x++, -0), zero.x);
