// The same holey Array (`new Array(n)` plus pushes), walked by a for-of of
// the module body over a call result: an IteratorRecord loop, whose element
// binding `x` lives in module init. Under PERRY_FACTS_SABOTAGE=arrdense
// every `new Array(...)` is born hole-free, so the element is a proven
// Number and `x` takes the int32 slot the facts give it. The hole's
// `undefined` must be reported where it is read or where it enters that
// slot, not converted to 0.
const RUN = Math.random() < 2 ? 1 : 0;
function use(m: number): number {
  return (m & 127) + 1;
}
function build(n: number): number[] {
  const xs: number[] = new Array(n);
  for (let i = 0; i < n; i++) xs.push(i * 3 + RUN);
  return xs;
}
let s = 0;
for (const x of build(4 * RUN)) s += x === undefined ? 100 : use(x);
console.log(s);
