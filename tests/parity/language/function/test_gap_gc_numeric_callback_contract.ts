// A live callback contract keeps the closure rooted for the post-loop read,
// carries only IEEE scalar seeds in the private body, and closes on all exits.
function make(n: number): (x: number) => number {
  return (x: number): number => x + n;
}
function run(f: (x: number) => number, count: number, start: number): number {
  let value = start;
  for (let i = 0; i < count; ++i) value = f(value);
  console.log(f('after' as any));
  return value;
}
for (const n of [4, -0, .5, NaN, Infinity, 's', { valueOf() { return 3; } }] as any[]) {
  console.log(run(make(n), 3, 2));
}
function mutate(f: (x: number) => number, other: (x: number) => number): number {
  let value = 2;
  for (let i = 0; i < 3; ++i) { value = f(value); f = other; }
  return value;
}
console.log(mutate(make(1), make(9)));
function stop(f: (x: number) => number, count: number): number {
  let value = 2;
  for (let i = 0; i < count; ++i) {
    value = f(value);
    if (i === 2) break;
  }
  console.log(f('exit' as any));
  return value;
}
console.log(stop(make(3), 10), stop(make(3), 0));
function returnFromLoop(f: (x: number) => number): number {
  let value = 2;
  for (let i = 0; i < 4; ++i) { value = f(value); if (i === 2) return value; }
  return value;
}
console.log(returnFromLoop(make(3)));
let order = '';
const object: any = { valueOf() { order += 'c'; return 5; } };
console.log(run(make(object), 2, 1), order);
function makeMutable(n: number): any {
  return { call: (x: number): number => x + n, set: (v: any) => { n = v; } };
}
function runMutable(f: (x: number) => number, change: () => void): number {
  let value = 2;
  for (let i = 0; i < 3; ++i) { value = f(value); change(); }
  return value;
}
const mutable = makeMutable(1);
console.log(runMutable(mutable.call, () => mutable.set('s')));

let controlReads = 0;
const countObject: any = { valueOf() { ++controlReads; return 2; } };
console.log(run(make(1), countObject, 3), controlReads);
