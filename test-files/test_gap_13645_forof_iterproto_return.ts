// #13645 negative: %ArrayIteratorPrototype%.return added.

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
let closes = 0;
AIP.return = function () { closes++; return {}; };
probe("break", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]), "closes", closes);
try { for (const v of [1, 2]) { if (v === 1) throw new Error("x"); } } catch (e) { console.log("throw closes", closes); }
for (const v of [1, 2]) { void v; }
console.log("exhausted closes", closes);
