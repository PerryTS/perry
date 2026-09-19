// #10761 — a property read spelled `O[K]` with a hoisted `const K = "a"` must
// be observably identical to `O.a` and `O["a"]` in EVERY case, now that the
// module-const fold rewrites the folded `IndexGet { _, String }` into a
// `PropertyGet` (perry-transform/src/module_const_fold.rs, phase 2).
//
// Each case prints the three spellings side by side. A rewrite that changed
// ANY of [[Get]]'s obligations shows up as a divergence between columns, and
// a rewrite that changed the answer outright shows up against node, which runs
// this same file as the oracle.
//
// The one guard in the rewrite is `is_numeric_index_string`: an array index
// key must keep `IndexGet` semantics. Cases 15/16 are its witnesses — delete
// the guard and they print element values where they must print `undefined`.
const K = "a";
const M = "missing";
const NUM0 = "0";
const NUM1 = "1";
const NUM07 = "07";
const FRAC = "1.5";
const NEG = "-1";
const EMPTY = "";
const LEN = "length";

const out: string[] = [];
function show(label: string, a: unknown, b: unknown, c: unknown): void {
  out.push(label + " | " + String(a) + " | " + String(b) + " | " + String(c));
}
function trap(f: () => unknown): string {
  try {
    return "value:" + String(f());
  } catch (e) {
    return "throw:" + (e instanceof TypeError ? "TypeError" : String(e));
  }
}

// 1 — plain own data property
const o1: any = { a: 1, b: 2 };
show("1 own-data", o1[K], o1["a"], o1.a);

// 2 — own accessor installed by defineProperty
const o2: any = {};
let getCalls = 0;
Object.defineProperty(o2, "a", {
  get() {
    getCalls++;
    return 42;
  },
  configurable: true,
});
show("2 own-getter", o2[K], o2["a"], o2.a);
out.push("2 getter-call-count " + getCalls);

// 3 — setter-only own accessor reads as undefined
const o3: any = {};
Object.defineProperty(o3, "a", { set(_v: number) {}, configurable: true });
show("3 setter-only", o3[K], o3["a"], o3.a);

// 4 — accessor on the prototype chain
const proto4: any = {};
Object.defineProperty(proto4, "a", {
  get() {
    return "from-proto";
  },
  configurable: true,
});
const o4: any = Object.create(proto4);
show("4 proto-getter", o4[K], o4["a"], o4.a);

// 5 — non-enumerable data descriptor
const o5: any = {};
Object.defineProperty(o5, "a", { value: 5, enumerable: false, writable: true, configurable: true });
show("5 nonenum-data", o5[K], o5["a"], o5.a);

// 6 — non-writable, non-configurable
const o6: any = {};
Object.defineProperty(o6, "a", { value: 6, writable: false, configurable: false });
show("6 frozen-slot", o6[K], o6["a"], o6.a);

// 7 — frozen object
const o7: any = Object.freeze({ a: 7 });
show("7 frozen-obj", o7[K], o7["a"], o7.a);

// 8 — sealed object
const o8: any = Object.seal({ a: 8 });
show("8 sealed-obj", o8[K], o8["a"], o8.a);

// 9 — delete then read
const o9: any = { a: 9, z: 0 };
show("9 before-delete", o9[K], o9["a"], o9.a);
delete o9.a;
show("9 after-delete", o9[K], o9["a"], o9.a);

// 10 — own shadows inherited
const proto10: any = { a: "proto" };
const o10: any = Object.create(proto10);
show("10 inherited", o10[K], o10["a"], o10.a);
o10.a = "own";
show("10 shadowed", o10[K], o10["a"], o10.a);
delete o10.a;
show("10 unshadowed", o10[K], o10["a"], o10.a);

// 11 — setPrototypeOf after the site has run
const o11: any = {};
show("11 no-proto", o11[K], o11["a"], o11.a);
Object.setPrototypeOf(o11, { a: "late-proto" });
show("11 late-proto", o11[K], o11["a"], o11.a);

// 12 — __proto__ assignment
const o12: any = {};
show("12 pre-__proto__", o12[K], o12["a"], o12.a);
o12.__proto__ = { a: "via-dunder" };
show("12 post-__proto__", o12[K], o12["a"], o12.a);

// 13 — Proxy receiver: the trap must see the same key for all three spellings
const seen: string[] = [];
const p13: any = new Proxy(
  { a: "target" },
  {
    get(t: any, k: any) {
      if (typeof k === "string") seen.push(k);
      return k === "a" ? "trapped" : Reflect.get(t, k);
    },
  },
);
show("13 proxy", p13[K], p13["a"], p13.a);
out.push("13 trap-keys " + seen.join(","));

// 14 — nullish receivers must throw TypeError, not read undefined
const nul: any = null;
const undef: any = undefined;
out.push("14 null-const " + trap(() => nul[K]));
out.push("14 null-lit " + trap(() => nul["a"]));
out.push("14 null-dot " + trap(() => nul.a));
out.push("14 undef-const " + trap(() => undef[K]));
out.push("14 undef-lit " + trap(() => undef["a"]));
out.push("14 undef-dot " + trap(() => undef.a));

// 15 — GUARD WITNESS: a canonical numeric key on an ARRAY is an element read,
// not a named read. `arr[NUM0]` must be the element; `arr[FRAC]`/`arr[NEG]`/
// `arr[NUM07]`/`arr[EMPTY]` are names and must miss.
const arr: any = ["zero", "one", "two"];
out.push("15 arr-0 " + String(arr[NUM0]) + " | " + String(arr["0"]) + " | " + String(arr[0]));
out.push("15 arr-1 " + String(arr[NUM1]) + " | " + String(arr["1"]));
out.push("15 arr-07 " + String(arr[NUM07]) + " | " + String(arr["07"]));
out.push("15 arr-frac " + String(arr[FRAC]) + " | " + String(arr["1.5"]));
out.push("15 arr-neg " + String(arr[NEG]) + " | " + String(arr["-1"]));
out.push("15 arr-empty " + String(arr[EMPTY]) + " | " + String(arr[""]));
out.push("15 arr-length " + String(arr[LEN]) + " | " + String(arr["length"]) + " | " + String(arr.length));

// 16 — GUARD WITNESS: a numeric-string OWN property on a plain object, with an
// array-index twin, so a fold that treats "0" as a name is visible.
const o16: any = { "0": "named-zero", a: 16 };
out.push("16 obj-0 " + String(o16[NUM0]) + " | " + String(o16["0"]) + " | " + String(o16[0]));
const mixed: any = ["elem0"];
mixed["0"] = "overwritten";
out.push("16 mixed-0 " + String(mixed[NUM0]) + " | " + String(mixed[0]) + " len=" + mixed.length);

// 17 — a missing key
const o17: any = { b: 1 };
show("17 absent", o17[M], o17["missing"], o17.missing);

// 18 — an accessor installed AFTER the read site has already executed
const o18: any = { a: "data" };
show("18 data-first", o18[K], o18["a"], o18.a);
Object.defineProperty(o18, "a", {
  get() {
    return "now-accessor";
  },
  configurable: true,
});
show("18 accessor-after", o18[K], o18["a"], o18.a);

// 19 — a getter that mutates the receiver during the read
const o19: any = { z: 0 };
Object.defineProperty(o19, "a", {
  get() {
    o19.z = o19.z + 1;
    return o19.z;
  },
  configurable: true,
});
show("19 mutating-getter", o19[K], o19["a"], o19.a);

// 20 — Symbol key congruence control (never folded; must still agree)
const SYM = Symbol.for("perry.10761");
const o20: any = { [SYM]: "sym-value", a: 20 };
out.push("20 symbol " + String(o20[SYM]) + " | " + String(o20[K]));

// 21 — string receiver named read, and a numeric key on a string
const s21: any = "abc";
out.push("21 str-length " + String(s21[LEN]) + " | " + String(s21["length"]) + " | " + String(s21.length));
out.push("21 str-0 " + String(s21[NUM0]) + " | " + String(s21["0"]) + " | " + String(s21[0]));

// 22 — class instance: own field, prototype method, prototype accessor
class C22 {
  a = 22;
  get g(): string {
    return "getter";
  }
  m(): string {
    return "method";
  }
}
const GKEY = "g";
const MKEY = "m";
const c22: any = new C22();
show("22 field", c22[K], c22["a"], c22.a);
show("22 proto-getter", c22[GKEY], c22["g"], c22.g);
out.push("22 proto-method " + String(typeof c22[MKEY]) + " | " + String(typeof c22["m"]) + " | " + String(typeof c22.m));

// 23 — the read in a hot loop, so the inline cache is primed and then broken
const o23: any = { a: 1 };
let sum = 0;
for (let i = 0; i < 50; i++) sum = sum + o23[K];
out.push("23 warm-sum " + sum);
Object.defineProperty(o23, "a", {
  get() {
    return 100;
  },
  configurable: true,
});
let sum2 = 0;
for (let i = 0; i < 5; i++) sum2 = sum2 + o23[K];
out.push("23 post-accessor-sum " + sum2);

// 24 — a polymorphic site: three different shapes through one const-key read
const shapes: any[] = [{ a: 1 }, { x: 0, a: 2 }, Object.create({ a: 3 })];
let poly = "";
for (let i = 0; i < shapes.length; i++) poly = poly + String(shapes[i][K]) + ",";
out.push("24 poly " + poly);

console.log(out.join("\n"));
