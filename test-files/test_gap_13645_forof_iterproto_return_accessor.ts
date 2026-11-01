// #13645 negative: a return accessor on %ArrayIteratorPrototype%.

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

const AIP = Object.getPrototypeOf([1].values());
let gets = 0;
Object.defineProperty(AIP, "return", { configurable: true, get() { gets++; return undefined; } });
probe("break", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]), "gets", gets);
