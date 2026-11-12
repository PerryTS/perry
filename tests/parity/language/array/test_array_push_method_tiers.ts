// `a.push(v)` is `Call(Get(a, "push"), a, [v])`: every push tier that calls the
// builtin append by name must first know that `push` IS the builtin on its
// receiver. The receivers below are all annotated `number[]`:
//   (1) an Array with an own data property `push`,
//   (2) an Array with an own accessor `push`,
//   (3) an Array whose own [[Prototype]] holds `push` (the per-receiver
//       prototype patch; a compiled program may not replace
//       `Array.prototype.push` itself),
//   (4) an Array subclass that overrides `push`,
//   (5) an object that is no Array at all (`as any`).
// Each reaches the tiers through a parameter, a class field (value position,
// statement position, a constructor), a module binding and a local, with a
// literal, a proven Number call, and a value that is a Number by annotation
// only (the guarded numeric tier).

function enc2(from: number, to: number): number {
  return from | (to << 7);
}
function OWN(this: any, ...args: any[]): any {
  return "OWN(" + args.join(",") + ")";
}
function st(a: any): string {
  let s = Array.isArray(a) ? "A[" : "O[";
  const n = a.length;
  for (let i = 0; i < n; i++) s += String(a[i]) + ",";
  return s + "]";
}
function show(r: any): string {
  return typeof r + ":" + String(r);
}
function T(label: string, f: () => any, a: any): void {
  let r: string;
  try {
    r = show(f());
  } catch (e: any) {
    r = "THROW " + (e && e.name);
  }
  console.log(label + " -> " + r + " | " + st(a));
}

// An Array with room: the guarded numeric tier's runtime guard needs
// `length < capacity`.
function mk(): number[] {
  const a: number[] = [];
  a[0] = 3;
  a[1] = 1;
  a[2] = 2;
  return a;
}
class Sub extends Array<number> {
  push(...args: any[]): any {
    return "SUB(" + args.join(",") + ")";
  }
}
class Plain extends Array<number> {}
function mkOwn(): number[] {
  const a = mk();
  (a as any).push = OWN;
  return a;
}
function mkAcc(): number[] {
  const a = mk();
  Object.defineProperty(a, "push", {
    get() {
      return OWN;
    },
    configurable: true,
  });
  return a;
}
function mkProto(): number[] {
  const a = mk();
  const p: any = Object.create(Array.prototype);
  p.push = OWN;
  Object.setPrototypeOf(a, p);
  return a;
}
function mkSub(): number[] {
  const s = new Sub();
  return s as any;
}
function mkPlainSub(): number[] {
  const s = new Plain();
  return s as any;
}
function mkFake(): number[] {
  const o: any = { length: 0 };
  o.push = function (...args: any[]): any {
    return "FAKE(" + args.join(",") + ")";
  };
  return o as number[];
}
function mkNoPush(): number[] {
  const o: any = { length: 0 };
  return o as number[];
}
function mkNamed(): number[] {
  const a = mk();
  (a as any).tag = "t";
  return a;
}
function mkBorrowed(): number[] {
  const a = mk();
  (a as any).push = Array.prototype.push;
  return a;
}
function mkNonCallable(): number[] {
  const a = mk();
  (a as any).push = 5;
  return a;
}
function mkFrozen(): number[] {
  const a = mk();
  Object.freeze(a);
  return a;
}

const NN: number[] = JSON.parse("[3,4]") as number[];

// --- The sites. One function per value kind, so each keeps its own tier.
function P_lit(a: number[]): any {
  return a.push(7);
}
function P_call(a: number[]): any {
  return a.push(enc2(6, 1));
}
function P_ann(a: number[], na: number, nb: number): any {
  return a.push(na + nb);
}
function P_annl(a: number[], na: number, nb: number): any {
  const nv: number = na + nb;
  return a.push(nv);
}
function P_stmt(a: number[], na: number, nb: number): void {
  a.push(na + nb);
}
class F {
  k: number[] = mk();
  lit(): any {
    return this.k.push(7);
  }
  call(): any {
    return this.k.push(enc2(6, 1));
  }
  ann(na: number, nb: number): any {
    return this.k.push(na + nb);
  }
  stmt(na: number, nb: number): void {
    this.k.push(na + nb);
  }
  none(): any {
    return this.k.push();
  }
}
// A constructor's push of a field handed in from outside (no fresh birth).
class C {
  k: number[];
  r: any;
  constructor(k: number[], na: number, nb: number) {
    this.k = k;
    this.r = this.k.push(na + nb);
    this.k.push(enc2(6, 1));
  }
}
// A holder that is no class instance: `o.list.push(v)`.
interface Holder {
  list: number[];
}
function H_val(h: Holder, na: number, nb: number): any {
  return h.list.push(na + nb);
}
function H_stmt(h: Holder, na: number, nb: number): void {
  h.list.push(na + nb);
}
let G: number[] = mk();
function G_lit(): any {
  return G.push(7);
}
function G_ann(na: number, nb: number): any {
  return G.push(na + nb);
}

const KINDS: [string, () => number[]][] = [
  ["plain", mk],
  ["own", mkOwn],
  ["accessor", mkAcc],
  ["proto", mkProto],
  ["sub", mkSub],
  ["plainsub", mkPlainSub],
  ["fake", mkFake],
  ["nopush", mkNoPush],
  ["named", mkNamed],
  ["borrowed", mkBorrowed],
  ["noncallable", mkNonCallable],
  ["frozen", mkFrozen],
  // The sites have now seen every receiver: a plain Array again.
  ["plain again", mk],
];
for (const [name, make] of KINDS) {
  {
    const a = make();
    T("P lit " + name, () => P_lit(a), a);
  }
  {
    const a = make();
    T("P call " + name, () => P_call(a), a);
  }
  {
    const a = make();
    T("P ann " + name, () => P_ann(a, NN[0], NN[1]), a);
  }
  {
    const a = make();
    T("P annl " + name, () => P_annl(a, NN[0], NN[1]), a);
  }
  {
    const a = make();
    T("P stmt " + name, () => P_stmt(a, NN[0], NN[1]), a);
  }
  {
    const f = new F();
    f.k = make();
    T("F lit " + name, () => f.lit(), f.k);
  }
  {
    const f = new F();
    f.k = make();
    T("F call " + name, () => f.call(), f.k);
  }
  {
    const f = new F();
    f.k = make();
    T("F ann " + name, () => f.ann(NN[0], NN[1]), f.k);
  }
  {
    const f = new F();
    f.k = make();
    T("F stmt " + name, () => f.stmt(NN[0], NN[1]), f.k);
  }
  {
    const f = new F();
    f.k = make();
    T("F none " + name, () => f.none(), f.k);
  }
  {
    const a = make();
    T("C ctor " + name, () => new C(a, NN[0], NN[1]).r, a);
  }
  // A holder that is no class instance is an ordinary method site, not a push
  // tier. Its universal dispatcher still appends to an array-like that has no
  // `push` at all where node throws, so that one receiver is left out here.
  if (name !== "nopush") {
    {
      const h: Holder = { list: make() };
      T("H val " + name, () => H_val(h, NN[0], NN[1]), h.list);
    }
    {
      const h: Holder = { list: make() };
      T("H stmt " + name, () => H_stmt(h, NN[0], NN[1]), h.list);
    }
  }
  {
    G = make();
    T("G lit " + name, () => G_lit(), G);
  }
  {
    G = make();
    T("G ann " + name, () => G_ann(NN[0], NN[1]), G);
  }
}

// --- Locals: the receiver is born and patched in the function that pushes.
function L_own(na: number, nb: number): string {
  const a: number[] = [];
  (a as any).push = OWN;
  const r1: any = a.push(7);
  const r2: any = a.push(enc2(6, 1));
  const r3: any = a.push(na + nb);
  const nv: number = na + nb;
  const r4: any = a.push(nv);
  a.push(na + nb);
  return [r1, r2, r3, r4].join(" ") + " | " + st(a);
}
function L_acc(na: number, nb: number): string {
  const a: number[] = [];
  Object.defineProperty(a, "push", {
    get() {
      return OWN;
    },
    configurable: true,
  });
  const r1: any = a.push(7);
  const r3: any = a.push(na + nb);
  a.push(na + nb);
  return [r1, r3].join(" ") + " | " + st(a);
}
function L_proto(na: number, nb: number): string {
  const a: number[] = [];
  const p: any = Object.create(Array.prototype);
  p.push = OWN;
  Object.setPrototypeOf(a, p);
  const r1: any = a.push(7);
  const r3: any = a.push(na + nb);
  a.push(na + nb);
  return [r1, r3].join(" ") + " | " + st(a);
}
function L_late(na: number, nb: number): string {
  // Plain while the site first runs, patched afterwards, restored at the end.
  const a: number[] = [];
  let out = "";
  for (let i = 0; i < 6; i++) {
    if (i === 2) (a as any).push = OWN;
    if (i === 4) delete (a as any).push;
    const r: any = a.push(na + nb + i);
    out += String(r) + " ";
  }
  return out + "| " + st(a);
}
function L_sub(na: number, nb: number): string {
  const a: number[] = new Sub() as any;
  const r1: any = a.push(7);
  const r3: any = a.push(na + nb);
  a.push(na + nb);
  return [r1, r3].join(" ") + " | " + st(a);
}
function L_fake(na: number, nb: number): string {
  const a: number[] = mkFake();
  const r1: any = a.push(7);
  const r3: any = a.push(na + nb);
  a.push(na + nb);
  return [r1, r3].join(" ") + " | " + st(a);
}
console.log("L own", L_own(NN[0], NN[1]));
console.log("L accessor", L_acc(NN[0], NN[1]));
console.log("L proto", L_proto(NN[0], NN[1]));
console.log("L late", L_late(NN[0], NN[1]));
console.log("L sub", L_sub(NN[0], NN[1]));
console.log("L fake", L_fake(NN[0], NN[1]));

// --- The repro the push lane's off arm showed (`PERRY_FACTS_DIRECT_NUMBER_CALLS=0`
// sends a proven Number call to the guarded numeric tier): module level, an
// empty literal, an own `push`.
const own: number[] = [];
(own as any).push = function (v: number): number {
  return -v;
};
console.log(own.push(enc2(6, 1)), own.length);

// --- Negative controls: plain pushes of every value kind stay right, and the
// value of the expression is the new length.
function ctl(): string {
  const nums: number[] = [];
  let lens = 0;
  for (let i = 0; i < 300; i++) lens += nums.push(enc2(i & 127, i >> 7));
  for (let i = 0; i < 300; i++) lens += nums.push(i * 0.5);
  const ann: number[] = [];
  for (let i = 0; i < 300; i++) lens += ann.push(NN[0] + NN[1] + i);
  const strs: string[] = [];
  for (let i = 0; i < 40; i++) lens += strs.push("s" + i);
  const objs: { v: number }[] = [];
  for (let i = 0; i < 40; i++) lens += objs.push({ v: i });
  const anys: any[] = [];
  lens += anys.push(1);
  lens += anys.push("two");
  lens += anys.push({ three: 3 });
  lens += anys.push(undefined);
  lens += anys.push(null);
  lens += anys.push(true);
  lens += anys.push(10n);
  lens += anys.push([1, 2]);
  lens += anys.push(NaN);
  lens += anys.push(-0);
  let sum = 0;
  for (let i = 0; i < nums.length; i++) sum += nums[i];
  for (let i = 0; i < ann.length; i++) sum += ann[i];
  return (
    [lens, sum, nums.length, ann.length, strs.length, strs[39], objs.length, objs[39].v].join(" ") +
    " " +
    anys.map((x) => typeof x + ":" + String(x)).join(",") +
    " " +
    Object.is(anys[9], -0)
  );
}
console.log("ctl", ctl());

class Bag {
  items: number[] = [];
  names: string[] = [];
  total = 0;
  constructor(n: number) {
    for (let i = 0; i < n; i++) this.items.push(i * 3);
    this.total = this.items.push(-1);
    this.names.push("a");
  }
  add(v: number): number {
    return this.items.push(v);
  }
  addName(s: string): number {
    return this.names.push(s);
  }
  fill(n: number): void {
    for (let i = 0; i < n; i++) this.items.push(i);
  }
}
const bag = new Bag(50);
let bl = 0;
for (let i = 0; i < 200; i++) bl += bag.add(NN[0] + i);
bag.fill(100);
console.log("bag", bag.total, bl, bag.items.length, bag.items[49], bag.items[51], bag.addName("b"), bag.names.join(""));
// The same methods once the field's Array owns `push`.
(bag.items as any).push = OWN;
console.log("bag own", bag.add(1), bag.items.length);
bag.fill(2);
console.log("bag own fill", bag.items.length);
delete (bag.items as any).push;
console.log("bag restored", bag.add(1), bag.items.length);
