// Negative controls of `test_array_quiet_sites`: loops without calls whose
// reads, stores and operators run no program code. Every one of them keeps
// `xs.length` in a slot (the legacy length hoist): the facts call each site
// quiet (plain Arrays under Number keys, data fields of known classes,
// primitive operands). `PERRY_ARRAY_BASE_DIAG=1` prints one
// `[perry quiet] ... verdict=kept` line per loop here and no `refused` line;
// with `PERRY_FACTS_QUIET_SITES=0` it prints neither.
class Plain {
  a: number = 0;
  label: string = "p";
}
class Holder {
  xs: number[];
  total: number = 0;
  constructor() {
    this.xs = [1, 2, 3, 4, 5, 6];
  }
  // A loop of a method over a local read from a field, storing a field.
  sum(): number {
    const xs = this.xs;
    for (let i = 0; i < xs.length; i++) {
      this.total = this.total + xs[i];
    }
    return this.total;
  }
}
function otherArray(): string {
  const h = new Holder();
  const xs = h.xs;
  const out: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    out[i] = xs[i] + 1;
    n += out[i];
  }
  return out.join(",") + ":" + n;
}
function ownStore(): string {
  const xs: number[] = [1, 2, 3, 4, 5, 6];
  for (let i = 0; i < xs.length; i++) {
    xs[i] = xs[i] * 2;
  }
  return xs.join(",");
}
function classField(): number {
  const h = new Holder();
  const xs = h.xs;
  const p = new Plain();
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    p.a = p.a + xs[i];
    n += p.a;
  }
  return n;
}
function numbers(): number {
  const h = new Holder();
  const xs = h.xs;
  const q: number = xs.length;
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    n = n + q * 2 - (i | 0) + (xs[i] < 3 ? 1 : -1) + Math.abs(xs[i] - 3);
  }
  return n;
}
function strings(): string {
  const h = new Holder();
  const xs = h.xs;
  const q: string = "ab";
  let t = "";
  for (let i = 0; i < xs.length; i++) {
    t = t + q + xs[i] + ",";
  }
  return t;
}
function nested(): number {
  const rows: number[][] = [
    [1, 2, 3],
    [4, 5],
    [6],
  ];
  let n = 0;
  for (let r = 0; r < rows.length; r++) {
    const row = rows[r];
    for (let c = 0; c < row.length; c++) {
      n += row[c] * (r + 1);
    }
  }
  return n;
}
function branches(): number {
  const h = new Holder();
  const xs = h.xs;
  let even = 0;
  let odd = 0;
  for (let i = 0; i < xs.length; i++) {
    if ((xs[i] & 1) === 0) {
      even += xs[i];
    } else {
      odd -= xs[i];
    }
  }
  return even * 100 + odd;
}
console.log(otherArray());
console.log(ownStore());
console.log(classField());
console.log(numbers());
console.log(strings());
console.log(nested());
console.log(branches());
console.log(new Holder().sum());
