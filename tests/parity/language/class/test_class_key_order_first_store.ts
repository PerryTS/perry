// A class's own keys enumerate in the order the constructor first stores
// them, whether the store is at top level or inside a branch, loop or try,
// and whatever the run takes: an arm that is skipped, a key added later, a
// key deleted and added again.

function show(label: string, o: any): void {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  console.log(label, Object.keys(o).join(","), JSON.stringify(o), forIn.join(","),
    Object.getOwnPropertyNames(o).join(","));
}

// if/else stores `pre` in both arms, then a top-level store of `build`.
class IfElse {
  [k: string]: any;
  constructor(i: number) {
    this.raw = "r";
    if (i % 2 === 0) { this.pre = []; } else { this.pre = [i]; }
    this.build = [];
  }
}

// Nested branches; the inner key is stored only when both conditions hold.
class Nested {
  [k: string]: any;
  constructor(a: boolean, b: boolean) {
    this.x = 1;
    if (a) {
      if (b) { this.deep = 2; } else { this.shallow = 3; }
      this.mid = 4;
    }
    this.y = 5;
  }
}

// A key stored in a loop body (zero, one or many iterations).
class Loop {
  [k: string]: any;
  constructor(n: number) {
    this.first = 0;
    for (let i = 0; i < n; i++) { this.count = i; }
    this.last = n;
  }
}

// A key first stored in a `try`, another in its `catch`.
class Tried {
  [k: string]: any;
  constructor(fail: boolean) {
    this.a = 1;
    try {
      if (fail) throw new Error("x");
      this.ok = true;
    } catch (e) {
      this.err = "caught";
    } finally {
      this.fin = 1;
    }
    this.z = 2;
  }
}

// A key stored in only one arm, followed by a top-level key.
class OneArm {
  [k: string]: any;
  constructor(take: boolean) {
    this.p = 1;
    if (take) { this.opt = "yes"; }
    this.q = 2;
  }
  addLater(): void { this.opt = "later"; }
}

// Field initializers first, then the constructor's stores.
class Fields {
  [k: string]: any;
  f1 = "f1";
  f2 = 2;
  constructor(c: boolean) {
    if (c) { this.cb = 1; } else { this.cb = 2; }
    this.top = 3;
  }
}

// A subclass: the parent's keys first, then the subclass's.
class Base {
  [k: string]: any;
  constructor(c: boolean) {
    if (c) { this.bk = "t"; }
    this.bt = 1;
  }
}
class Sub extends Base {
  constructor(c: boolean) {
    super(c);
    if (!c) { this.sk = "s"; } else { this.sk = "S"; }
    this.st = 2;
  }
}

for (let i = 0; i < 2; i++) show("ifelse" + i, new IfElse(i));
show("nested-tt", new Nested(true, true));
show("nested-tf", new Nested(true, false));
show("nested-f", new Nested(false, true));
show("loop0", new Loop(0));
show("loop3", new Loop(3));
show("try-ok", new Tried(false));
show("try-fail", new Tried(true));
show("onearm-t", new OneArm(true));
const skipped = new OneArm(false);
show("onearm-f", skipped);
skipped.addLater();
show("onearm-f-later", skipped);
show("fields-t", new Fields(true));
show("fields-f", new Fields(false));
show("sub-t", new Sub(true));
show("sub-f", new Sub(false));

// delete and re-add: the key goes to the end.
const d = new IfElse(0);
delete d.pre;
show("deleted", d);
d.pre = "again";
show("readded", d);
const n = new Nested(true, true);
delete n.x;
n.x = 9;
show("nested-readd", n);

// Many objects of one shape, both arms, read back.
let sum = 0;
for (let i = 0; i < 1000; i++) {
  const o = new IfElse(i);
  sum += o.pre.length + Object.keys(o).length;
}
console.log("sum", sum);

// Read back a key stored in one arm: absent at birth, added later after
// another key, read through a field read, a method, a function the facts
// can prove, and every enumeration; deleted and added again; and the same
// class when the arm is taken.
class Late {
  [k: string]: any;
  constructor(take: boolean) {
    this.p = 1;
    if (take) { this.opt = "arm"; }
    this.q = 2;
  }
  setOpt(v: string): void { this.opt = v; }
  readOpt(): any { return this.opt; }
}
function readLate(o: Late): any { return o.opt; }
function readBack(label: string, o: Late): void {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  console.log(label, o.opt, o.readOpt(), readLate(o), o.q, "opt" in o,
    Object.keys(o).join(","), JSON.stringify(o), forIn.join(","),
    JSON.stringify(Object.entries(o)));
}
for (const take of [false, true]) {
  const o = new Late(take);
  readBack("late-" + take + "-birth", o);
  o.setOpt("added");
  readBack("late-" + take + "-added", o);
  o.setOpt("again");
  readBack("late-" + take + "-again", o);
  delete o.opt;
  readBack("late-" + take + "-deleted", o);
  o.setOpt("readded");
  readBack("late-" + take + "-readded", o);
}

// A loop body that stores a key on a later iteration, after the key the
// text stores after it.
class LoopLate {
  [k: string]: any;
  constructor(n: number) {
    this.p = 1;
    for (let i = 0; i < n; i++) { if (i === 1) { this.opt = i; } this.q = i; }
  }
  readOpt(): any { return this.opt; }
}
function readLoopLate(o: LoopLate): any { return o.opt; }
for (const n of [0, 1, 3]) {
  const o = new LoopLate(n);
  console.log("looplate" + n, o.opt, o.readOpt(), readLoopLate(o), Object.keys(o).join(","), JSON.stringify(o));
}

// An arm that leaves the constructor early stores keys the path to the end
// stores in another order.
class Exit {
  [k: string]: any;
  constructor(c: number) {
    if (c === 1) { this.raw = "early"; this.set = [1]; return; }
    if (c === 2) { this.only = 1; return; }
    this.options = c;
    this.raw = "late";
    this.set = [2];
  }
  readRaw(): any { return this.raw; }
}
function readExit(o: Exit): any { return o.raw; }
for (const c of [0, 1, 2]) {
  const o = new Exit(c);
  console.log("exit" + c, o.raw, o.readRaw(), readExit(o), Object.keys(o).join(","), JSON.stringify(o));
  o.only = "x";
  console.log("exit" + c + "-added", Object.keys(o).join(","), JSON.stringify(o));
}

let lateSum = 0;
for (let i = 0; i < 1000; i++) {
  const o = new Late(i % 3 === 0);
  if (i % 2 === 0) o.setOpt("v" + i);
  const r = readLate(o);
  if (r !== undefined) lateSum += r.length;
  const e = new Exit(i % 3);
  lateSum += Object.keys(o).join(",").length + Object.keys(e).join(",").length + String(readExit(e)).length;
}
console.log("lateSum", lateSum);
