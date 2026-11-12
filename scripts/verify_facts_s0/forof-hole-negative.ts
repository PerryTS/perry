// `new Array(n)` is born with `n` holes and the pushes append behind them:
// the for-of yields `undefined` four times. Under
// PERRY_FACTS_SABOTAGE=nohole the facts say no Array ever holds a hole
// (`Model::hole`), so the walk's element is a proven Number and `x` an
// int32. Nothing is wrong at any store: the false claim is first met where
// the walk reads the slot, and the read's witness must report the hole.
// A bound the compiler cannot fold keeps the Array a run-time value.
const RUN = Math.random() < 2 ? 1 : 0;
function use(m: number): number {
  return (m & 127) + 1;
}
function build(n: number): number[] {
  const xs: number[] = new Array(n);
  for (let i = 0; i < n; i++) xs.push(i * 3 + RUN);
  return xs;
}
function total(xs: number[]): number {
  let s = 0;
  for (const x of xs) s += x === undefined ? 100 : use(x);
  return s;
}
console.log(total(build(4 * RUN)));
