// `xs[k]` with `k` up to `n + 2` reads past the end of the pushed Array:
// `undefined`. Under PERRY_FACTS_SABOTAGE=inread the facts take every
// computed read under a Number key for one a length test bounds
// (`Cons::IdxLoad` with `kept`): the read counts as in bounds, its value as
// a Number. The witness at the read must report the key at the length.
// A bound the compiler cannot fold keeps the key a run-time value.
const RUN = Math.random() < 2 ? 1 : 0;
function use(m: number): number {
  return (m & 127) + 1;
}
function build(n: number): number[] {
  const xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i * 3 + RUN);
  return xs;
}
function probe(xs: number[], from: number): number {
  let s = 0;
  for (let k = from; k < from + 4 * RUN; k++) {
    const v = xs[k];
    s += v === undefined ? 100 : use(v);
  }
  return s;
}
console.log(probe(build(40), 38));
