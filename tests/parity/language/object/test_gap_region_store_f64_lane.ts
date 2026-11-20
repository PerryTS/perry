// Charter step 5: a loop region's bare store runs no field-representation
// check. A store of a value not proven a Number into a slot that is an F64
// lane of the receiver's shape must not happen bare (the region refuses the
// word); the stored value must read back exactly, and later reads of every
// field must agree with node.

class Pt {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function storeAny(p: Pt, v: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    (p as any).x = v;
    h = h + p.y;
  }
  return h;
}

function storeNum(p: Pt, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    p.x = i * 0.5;
    h = h + p.y;
  }
  return h;
}

function storeLit(o: any, v: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.a = v;
    h = h + o.b;
  }
  return h;
}

const pts: Pt[] = [];
for (let i = 0; i < 64; i++) pts.push(new Pt(i + 0.25, i * 2));
console.log(storeNum(pts[3], 100), pts[3].x, pts[3].y);
const vals: any[] = ["s", { k: 1 }, null, undefined, 7, 2.5, NaN, -0, Infinity, [1, 2]];
for (const v of vals) {
  const p = new Pt(1.5, 3);
  console.log(storeAny(p, v, 50), String(p.x), typeof p.x, p.y);
}
for (let r = 0; r < 3; r++) {
  const junk: any[] = [];
  for (let j = 0; j < 20000; j++) junk.push({ j, s: "x" + j });
  for (const p of pts) storeAny(p, r === 1 ? "t" + p.y : p.y + 0.5, 3);
  console.log(r, junk.length, pts.map((p) => String(p.x)).slice(0, 5).join(","));
}
for (const v of vals) {
  const o: any = { a: 1.25, b: 4 };
  console.log(storeLit(o, v, 50), String(o.a), o.b);
}
const q: any = { a: 0.5, b: 1 };
q.c = 2.5;
console.log(storeLit(q, "str", 10), q.a, q.c);
