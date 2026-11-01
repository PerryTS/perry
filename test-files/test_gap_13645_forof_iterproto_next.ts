// #13645 negative: %ArrayIteratorPrototype%.next replaced.

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

const AIP = Object.getPrototypeOf([][Symbol.iterator]());
const next = AIP.next;
let calls = 0;
AIP.next = function (this: any) { calls++; return next.call(this); };
probe("break", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]), "calls", calls);
