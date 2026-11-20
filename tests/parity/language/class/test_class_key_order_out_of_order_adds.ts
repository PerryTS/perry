// A constructor run may add keys out of the class layout's order (#14485 lays
// the keys out in first-store order along the text; another branch, a later
// loop iteration or a subclass can add them in another). The runtime then
// orders the object by its overflow, and every key added from that point on
// must be added there: it must be enumerated, in the order of the adds.

class Mixed {
  [k: string]: any;
  constructor(m: number) {
    this.a = 1;
    if (m === 0) { this.b = "b0"; this.c = "c0"; }
    else if (m === 1) { this.c = "c1"; this.b = "b1"; }
    else if (m === 2) { this.c = "c2"; }
    else if (m === 3) { this.b = "b3"; }
    this.d = m;
    if (m === 4) { this.c = "c4"; this.b = "b4"; }
    this.e = "e" + m;
  }
}
function readD(o: Mixed): any { return o.d; }
function readE(o: Mixed): any { return o.e; }
function keys(o: any): string {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  return [Object.keys(o).join(","), forIn.join(","), JSON.stringify(o), "d" in o, o.hasOwnProperty("e")].join(" ");
}
for (let m = 0; m < 6; m++) {
  const o = new Mixed(m);
  console.log("mixed", m, readD(o), readE(o), keys(o));
  o.d = "again";
  console.log("  stored", readD(o), keys(o));
}

// Both arms store both keys, in opposite orders.
class Swap {
  [k: string]: any;
  constructor(f: boolean) {
    this.a = 1;
    if (f) { this.x = 1; this.y = 2; } else { this.y = 2; this.x = 1; }
    this.z = 3;
  }
}
function readX(o: Swap): any { return o.x; }
function readZ(o: Swap): any { return o.z; }
for (const f of [true, false]) {
  const o = new Swap(f);
  console.log("swap", f, readX(o), readZ(o), keys(o));
}

// A later loop iteration adds a key the first one skipped.
class Late {
  [k: string]: any;
  constructor(n: number) {
    this.p = 1;
    for (let i = 0; i < n; i++) { if (i === 1) { this.opt = i; } this.q = i; }
    this.r = "r";
  }
}
function readR(o: Late): any { return o.r; }
for (const n of [0, 1, 3]) {
  const o = new Late(n);
  console.log("late", n, readR(o), keys(o));
}

// A subclass stores a parent's key late, then its own.
class Base {
  [k: string]: any;
  constructor(c: number) { if (c & 1) { this.bk = "bk"; } this.bt = c; }
}
class Sub extends Base {
  constructor(c: number) { super(c); if (c & 2) { this.sk = "sk"; } else if (c & 4) { this.bk = "late"; } this.st = "st"; }
}
function readSt(o: Sub): any { return o.st; }
for (let c = 0; c < 8; c++) {
  const o = new Sub(c);
  console.log("sub", c, readSt(o), keys(o));
}

// A subclass with a field definition, under a parent whose run reordered.
class Child extends Mixed {
  own = "own";
  constructor(m: number) { super(m); this.after = m; }
}
function readOwn(o: Child): any { return o.own; }
for (const m of [0, 1, 4]) {
  const o = new Child(m);
  console.log("child", m, readOwn(o), o.after, keys(o));
}

// A constructor function: its layout is its first stores in the text too.
function Fn(this: any, m: number) {
  this.a = 1;
  if (m) { this.c = 1; this.b = 2; } else { this.b = 2; this.c = 1; }
  this.d = 4;
}
for (const m of [0, 1]) {
  const o = new (Fn as any)(m);
  console.log("fn", m, o.d, keys(o));
}

// A `break` in a do-while body skips the stores after it.
class DoBreak {
  [k: string]: any;
  constructor(n: number) {
    this.s = 0;
    do { if (n === 0) break; this.dw = n; } while (false);
    this.e = n;
  }
}
function readDw(o: DoBreak): any { return o.dw; }
for (const n of [0, 1]) {
  const o = new DoBreak(n);
  const v = readDw(o);
  console.log("dobreak", n, v, v === undefined, typeof v, String(v), keys(o));
}
