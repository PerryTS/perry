// #13645 negative: `next` replaced on the array iterators' prototype chain.

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
// DEC 250 locks %ArrayIteratorPrototype% and Array.prototype; the change
// lands on user prototypes below them that these arrays (UAP) and their
// iterators (UIP) adopt, so the for-of still reads it from the live chain.
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
const next = AIP.next;
let calls = 0;
UIP.next = function (this: any) { calls++; return next.call(this); };
probe("break", arr(1, 2, 3));
console.log("ret", probeReturn(arr(1, 2, 3)), "calls", calls);
