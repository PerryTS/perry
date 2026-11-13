// `xs[k] = 7` with `k = n + 2` writes past the end of the pushed Array and
// leaves two holes, which the for-of yields as `undefined`. Under
// PERRY_FACTS_SABOTAGE=inbounds the facts take every index store under a
// Number key for one whose key is at most the length (`Cons::IndexKept`):
// the Array counts as hole-free and the element as a Number. The witness
// at the store must report the key past the length.
// A bound the compiler cannot fold keeps the store's key a run-time value.
const RUN = Math.random() < 2 ? 1 : 0;
function use(m: number): number {
  return (m & 127) + 1;
}
function build(n: number): number[] {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i * 3 + RUN);
  const k = n + 2 * RUN;
  xs[k] = 7;
  return xs;
}
let s = 0;
for (const x of build(40)) s += x === undefined ? 100 : use(x);
console.log(s);
