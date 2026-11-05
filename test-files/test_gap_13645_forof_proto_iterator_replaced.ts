// #13645 negative: [Symbol.iterator] replaced on the arrays' prototype chain.
// DEC 250 locks Array.prototype, so the replace lands on a user array
// prototype UAP that the arrays adopt.
const UAP = Object.create(Array.prototype);
function arr(...v: number[]): number[] {
  Object.setPrototypeOf(v, UAP);
  return v;
}

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
UAP[Symbol.iterator] = function* (this: number[]) {
  for (let i = this.length - 1; i >= 0; i--) yield this[i] * 100;
};
probe("reversed", arr(1, 2, 3));
console.log("ret", probeReturn(arr(1, 2, 3)));
UAP[Symbol.iterator] = original;
probe("restored", arr(1, 2, 3));
