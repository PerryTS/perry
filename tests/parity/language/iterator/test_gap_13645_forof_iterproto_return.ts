// #13645 negative: a `return` added on the array iterators' prototype chain.

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
// DEC 250 locks %ArrayIteratorPrototype%; the added `return` lives on a user
// prototype UIP below it, which the iterators of these arrays adopt, so the
// for-of close still looks it up on the array iterator's chain.
const UIP = Object.create(AIP);
const UAP = Object.create(Array.prototype);
UAP[Symbol.iterator] = function (this: any) {
  const it = Array.prototype.values.call(this);
  Object.setPrototypeOf(it, UIP);
  return it;
};
function arr(...v: number[]): number[] {
  Object.setPrototypeOf(v, UAP);
  return v;
}
let closes = 0;
UIP.return = function () { closes++; return {}; };
probe("break", arr(1, 2, 3));
console.log("ret", probeReturn(arr(1, 2, 3)), "closes", closes);
try { for (const v of arr(1, 2)) { if (v === 1) throw new Error("x"); } } catch (e) { console.log("throw closes", closes); }
for (const v of arr(1, 2)) { void v; }
console.log("exhausted closes", closes);
