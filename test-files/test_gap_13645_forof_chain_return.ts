// #13645 negative: Iterator.prototype / Object.prototype gain return.

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

let closes = 0;
const IP = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
IP.return = function () { closes++; return {}; };
probe("break", [1, 2, 3]);
console.log("ret", probeReturn([1, 2, 3]), "closes", closes);
delete IP.return;
(Object.prototype as any).return = function () { closes += 10; return {}; };
probe("objproto", [1, 2, 3]);
console.log("closes", closes);
delete (Object.prototype as any).return;
