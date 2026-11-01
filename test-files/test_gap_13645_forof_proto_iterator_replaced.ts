// #13645 negative: Array.prototype[Symbol.iterator] replaced.

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

const original = Array.prototype[Symbol.iterator];
(Array.prototype as any)[Symbol.iterator] = function* (this: number[]) {
  for (let i = this.length - 1; i >= 0; i--) yield this[i] * 100;
};
probe("reversed", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]));
(Array.prototype as any)[Symbol.iterator] = original;
probe("restored", [1, 2, 3]);
