// Negative controls of `test_array_quiet_strings`: loops over Arrays whose
// `length` and element reads look like the String ones but may run program
// code, which pops the Array the loop walks. Each must read the Array's
// length at every step (`PERRY_ARRAY_BASE_DIAG=1`: `refused`, never
// `kept`), so it stops where the popped Array ends, as in node.
class Popper {
  h: { xs: number[] };
  constructor(h: { xs: number[] }) {
    this.h = h;
  }
  get length(): number {
    this.h.xs.pop();
    return 1;
  }
}
// A `length` read whose receiver may be an object with a `length` getter.
function lengthGetter(q: any, h: { xs: number[] }): string {
  const xs = h.xs;
  let s = "";
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    n += q.length;
  }
  return s + n;
}
// A String object with an index getter past its end.
function boxedIndex(b: any, h: { xs: number[] }): string {
  const xs = h.xs;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ":" + b[i + 2] + ",";
  }
  return s;
}
const h1 = { xs: [1, 2, 3, 4, 5, 6] };
console.log(lengthGetter("ab", { xs: [9, 8] }), lengthGetter(new Popper(h1), h1));
const h2 = { xs: [1, 2, 3, 4, 5, 6] };
const so: any = new String("ab");
Object.defineProperty(so, "4", {
  get() {
    h2.xs.pop();
    return "g";
  },
});
console.log(boxedIndex("abcdefgh", { xs: [7, 7] }), boxedIndex(so, h2));
