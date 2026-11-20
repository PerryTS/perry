// A key a constructor stores in a branch, loop or `try` is reserved at its
// first store in the text (#14485): its slot is the key's own word and holds
// its value or the hole, and a read pairs the load with the hole compare.
// These reads go through typed functions (the proven slot read) on instances
// that skipped the store, stored it in order, or stored it out of order
// (the value then lives in the object's overflow), before and after updates,
// deletes and re-adds, with a prototype value behind the hole.

class LoopLate {
  [k: string]: any;
  constructor(n: number) {
    this.p = 1;
    for (let i = 0; i < n; i++) { if (i === 1) { this.opt = i; } this.q = i; }
  }
}
function readOpt(o: LoopLate): any { return o.opt; }
function readQ(o: LoopLate): any { return o.q; }
function writeOpt(o: LoopLate, v: any) { o.opt = v; }

function shape(o: any): string {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  return [("opt" in o), o.hasOwnProperty("opt"), Object.keys(o).join(","), forIn.join(","),
    Object.getOwnPropertyNames(o).join(","), JSON.stringify(o)].join(" ");
}

for (const n of [0, 1, 2, 3, 5]) {
  const o = new LoopLate(n);
  console.log("looplate", n, readOpt(o), readQ(o), shape(o));
  writeOpt(o, "w" + n);
  console.log("  written", readOpt(o), shape(o));
  delete o.opt;
  console.log("  deleted", readOpt(o), shape(o));
  o.opt = n;
  o.opt++;
  console.log("  re-added", readOpt(o), shape(o));
}

// Instances store the reserved keys in different orders.
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
  }
}
function readB(o: Mixed): any { return o.b; }
function readC(o: Mixed): any { return o.c; }
for (let m = 0; m < 6; m++) {
  const o = new Mixed(m);
  console.log("mixed", m, readB(o), readC(o), shape(o), Object.keys(o).join(","));
}

// One site sees every state.
class Many {
  [k: string]: any;
  constructor(n: number) {
    this.p = n;
    for (let i = 0; i < n % 4; i++) { if (i === 1) { this.opt = i * 100 + n; } this.q = i; }
  }
}
function readMany(o: Many): any { return o.opt; }
const many: Many[] = [];
for (let n = 0; n < 400; n++) many.push(new Many(n));
let sum = 0, absent = 0;
for (const o of many) { const v = readMany(o); if (v === undefined) absent++; else sum += v; }
console.log("many", sum, absent);
for (const o of many) { if (o.p % 3 === 0) o.opt = -1; }
sum = 0; absent = 0;
for (const o of many) { const v = readMany(o); if (v === undefined) absent++; else sum += v; }
console.log("many updated", sum, absent, Object.keys(many[3]).join(","), Object.keys(many[7]).join(","));

// An absent reserved key reads through the prototype chain.
class ProtoBase { [k: string]: any; }
(ProtoBase.prototype as any).opt = "from-proto";
class FromProto extends ProtoBase {
  constructor(f: boolean) { super(); this.a = 1; if (f) { this.opt = "own"; } this.z = 2; }
}
function readProto(o: FromProto): any { return o.opt; }
console.log("proto", readProto(new FromProto(false)), readProto(new FromProto(true)), shape(new FromProto(false)));

// A subclass adds reserved keys after its parent's.
class Base {
  [k: string]: any;
  constructor(c: number) { if (c & 1) { this.bk = "bk"; } this.bt = c; }
}
class Sub extends Base {
  constructor(c: number) { super(c); if (c & 2) { this.sk = "sk"; } else if (c & 4) { this.bk = "late"; } this.st = "st"; }
}
function readBk(o: Base): any { return o.bk; }
function readSk(o: Sub): any { return o.sk; }
for (let c = 0; c < 8; c++) {
  const s = new Sub(c);
  console.log("sub", c, readBk(s), readSk(s), Object.keys(s).join(","));
}

// try / catch / finally skip and reorder stores.
class Tried {
  [k: string]: any;
  constructor(f: number) {
    this.a = 1;
    try { if (f === 1) throw new Error("x"); this.ok = f; if (f === 2) throw 2; this.ok2 = true; }
    catch (e) { this.err = String(e); }
    finally { if (f !== 3) this.fin = f; }
    this.z = "z";
  }
}
function readOk(o: Tried): any { return o.ok; }
function readErr(o: Tried): any { return o.err; }
for (let f = 0; f < 4; f++) {
  const o = new Tried(f);
  console.log("tried", f, readOk(o), readErr(o), Object.keys(o).join(","));
}
