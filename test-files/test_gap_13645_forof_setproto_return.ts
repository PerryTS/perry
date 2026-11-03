// #13645 negative: setPrototypeOf on %ArrayIteratorPrototype% adds return.

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
const IP = Object.getPrototypeOf(AIP);
let closes = 0;
const mid = Object.create(IP);
mid.return = function () { closes++; return {}; };
Object.setPrototypeOf(AIP, mid);
probe("break", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]), "closes", closes);
