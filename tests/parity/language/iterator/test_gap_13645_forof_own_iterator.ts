// #13645 negative: an array's own Symbol.iterator.

function probe(label: string, xs: number[]): void {
  const out: number[] = [];
  for (const v of xs) {
    out.push(v);
    if (out.length === 2) break;
  }
  console.log(label, out.join(","));
}
function probeReturn(xs: number[]): number {
  for (const v of xs) { if (v === 2) return v; }
  return -1;
}

const xs: number[] = [1, 2, 3];
(xs as any)[Symbol.iterator] = function* () { yield 7; yield 8; yield 9; };
probe("own", xs);
probe("plain", [1, 2, 3]);
const ys: number[] = [4, 5, 6];
Object.setPrototypeOf(ys, { __proto__: Array.prototype, *[Symbol.iterator]() { yield 40; yield 50; } });
probe("reparented", ys);
