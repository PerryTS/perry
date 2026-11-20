// #9024: a scalar-replaced non-escaping object gives each DECLARED field an
// alloca. Writing a property the class does not declare has no slot, so the
// store was dropped and every later read answered `undefined`.
const a: any = {};
const a0 = a["k"];
a["k"] = 7;
console.log(a0, a["k"]);

const b: any = {};
const b0 = b.k;
b.k = 8;
console.log(b0, b.k);

class C {
  x = 1;
}
const c: any = new C();
const c0 = c["k"];
c["k"] = 9;
console.log(c0, c["k"], c.x);

// declared fields must still scalar-replace and read correctly
const d: any = { k: 0 };
const d0 = d["k"];
d["k"] = 10;
console.log(d0, d["k"]);

// read twice after the write
const e: any = {};
const e0 = e["k"];
e["k"] = 11;
const e1 = e["k"];
console.log(e0, e1, e["k"]);

// the ordinary cache shape this bug breaks
const cache: any = {};
function memo(n: number) {
  if (!cache["v"]) cache["v"] = n * 2;
  return cache["v"];
}
console.log(memo(21), memo(99));
