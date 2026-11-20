// #13645 negative: an Array subclass with its own iterator.

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

class Rev extends Array<number> {
  *[Symbol.iterator]() { for (let i = this.length - 1; i >= 0; i--) yield this[i]; }
}
const r = new Rev();
r.push(1, 2, 3);
probe("subclass", r);
class Plain extends Array<number> {}
const p = new Plain();
p.push(4, 5, 6);
probe("plain subclass", p);
