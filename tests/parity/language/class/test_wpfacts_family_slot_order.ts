// Check mode (`PERRY_VERIFY_FACTS=1`): a family layout's slots are a
// permutation of its key positions. The key `a` sits at position 4 in most
// of the program's layouts, so it is coloured to slot 4; in `Wide` (keys
// x, a, b, c, d) it takes slot 4 and b, c, d move down to slots 1..3. A
// proven read of `w.d` loads slot 3, which is the storage of `d` although
// the key at position 3 is `c`: the witness must compare the key's storage
// slot, not the key at that position. Every case prints what node prints.
class P1 {
  p = 1; q = 2; r = 3; s = 4; a = 5;
}
class P2 {
  p = 10; q = 20; r = 30; s = 40; a = 50;
}
class P3 {
  p = 100; q = 200; r = 300; s = 400; a = 500;
}
class Wide {
  x: number;
  a: number;
  b: number;
  c: number;
  d: number;
  constructor(x: number, a: number, b: number, c: number, d: number) {
    this.x = x;
    this.a = a;
    this.b = b;
    this.c = c;
    this.d = d;
  }
}
const keep: unknown[] = [];
let pa = 0;
for (let i = 0; i < 50; i++) {
  const o1 = new P1(), o2 = new P2(), o3 = new P3();
  keep.push(o1, o2, o3);
  pa += o1.a + o2.a + o3.a + o1.p + o2.q + o3.s;
}
let sx = 0, sa = 0, sb = 0, sc = 0, sd = 0;
for (let i = 0; i < 1000; i++) {
  const w = new Wide(i, i + 1, i + 2, i + 3, i + 0.25);
  keep.push(w);
  sx += w.x;
  sa += w.a;
  sb += w.b;
  sc += w.c;
  sd += w.d;
}
console.log(pa, sx, sa, sb, sc, sd);
const w = new Wide(1, 2, 3, 4, 5);
console.log(Object.keys(w).join(","), JSON.stringify(w));
w.d = 9;
console.log(w.d, w.c, w.b, w.a, w.x);
