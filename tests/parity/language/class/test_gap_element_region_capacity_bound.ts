// An element region over `const b = bs[i]` whose counter bound runs past
// `bs.length`: every read at `i >= length` must be a hole (`undefined`), as
// in node. The region's array guard proves `bound <= capacity`, so its bare
// element loads stay inside the backing store; reads past the capacity are
// left to the plain copy. The arrays allocated right after `bs` hold other
// instances of the same class, so a load past the backing store would find
// a word that passes the element's shape guard (or no mapped memory at all):
// without the bound this test reads wrong values or faults.

class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function sumPast(bs: P[], extra: number): number {
  let s = 0;
  const n = bs.length + extra;
  for (let i = 0; i < n; i++) {
    const b = bs[i];
    if (b !== undefined) s += b.x + b.y;
  }
  return s;
}

function run(len: number, extra: number): string {
  const bs: P[] = [];
  for (let i = 0; i < len; i++) bs.push(new P(i, 1));
  const other: P[] = [];
  for (let i = 0; i < 64; i++) other.push(new P(1000, 1000));
  const lit = [new P(1, 1), new P(2, 2), new P(3, 3), new P(4, 4)];
  const nb = [
    new P(7000, 7000),
    new P(7000, 7000),
    new P(7000, 7000),
    new P(7000, 7000),
    new P(7000, 7000),
    new P(7000, 7000),
  ];
  return [sumPast(bs, extra), sumPast(lit, extra), other.length + nb.length].join(" ");
}

for (const len of [1, 3, 4, 7, 8, 16]) {
  for (const extra of [1, 4, 16, 64]) console.log(len, extra, run(len, extra));
}
