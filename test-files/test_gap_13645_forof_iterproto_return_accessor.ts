// #13645 negative: a return accessor on the array iterators' prototype chain.

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
// DEC 250 locks %ArrayIteratorPrototype%; the return accessor lives on a user
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
let gets = 0;
Object.defineProperty(UIP, "return", { configurable: true, get() { gets++; return undefined; } });
probe("break", arr(1, 2, 3));
console.log("ret", probeReturn(arr(1, 2, 3)), "gets", gets);
