function make(n: number): (x: number) => number {
  return function(x: number): number { return (x * 31 + n) % 1000003; };
}
function runCaptured(f: (x: number) => number, start: number, count: number): number {
  let x = start;
  for (let i = 0; i < count; ++i) x = f(x);
  return x;
}
function show(v: any): string {
  return Object.is(v, -0) ? '-0' : String(v);
}
const values: any[] = [0, -0, 1, -1, .5, -2.5, NaN, Infinity, -Infinity,
  2147483647, -2147483648, 2147483648, 9007199254740991, 5e-324, '2'];
for (const n of values) {
  for (const x of values) console.log(show(n), show(x), show(runCaptured(make(n), x, 3)));
}
let order = '';
const n: any = { valueOf() { order += 'n'; return 4; } };
const x: any = { valueOf() { order += 'x'; return 8; } };
console.log(show(runCaptured(make(n), x, 2)), order);
let mutable = 1;
const closure = (x: number): number => (x * 31 + mutable) % 1000003;
console.log(show(runGeneric(closure, 3, 2)));
mutable = 9;
console.log(show(runGeneric(closure, 3, 2)));
const add = make(7);
console.log(show(runCaptured(add, 8, 0)), show(runCaptured(add, 8, -1)));
try { console.log(runCaptured(make(2n as any), 1, 2)); } catch (e) { console.log(e instanceof TypeError); }
function byThis(this: any, x: number): number { return this ? x + this.n : -1; }
console.log(runGeneric(byThis, 1, 3));
function identity(x: any): any { return x; }
console.log(runGeneric(identity, 'kept' as any, 3));

function runGeneric(f: (x: number) => number, start: number, count: number): number {
  let x = start;
  for (let i = 0; i < count; ++i) x = f(x);
  return x;
}
