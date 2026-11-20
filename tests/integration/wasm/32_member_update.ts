const o: any = { x: 5 };
const a = o.x++; console.log(a, o.x);
o.x++; console.log(o.x);
class C { n = 0; items: number[] = [0, 0];
  inc(): number { return this.n++; }
  bump(): number { return this.items[1]++; } }
const c = new C(); console.log(c.inc(), c.n);
const v = c.bump(); console.log(v); console.log(c.items[1]);
const arr = [5, 6]; let i = 0;
const e = arr[i++]++; console.log(e, arr.join(","), i);
const d = ++o.x; console.log(d, o.x);
const f = --o.x; console.log(f, o.x);
const k = "x";
const g = o[k]--; console.log(g, o.x);
