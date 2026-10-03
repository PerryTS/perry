// #10753: computed string-key reads `o[k]` are answered from the receiver's
// shape (key word against the canonical key list) and from confirmed absent
// verdicts that rest on Object.prototype's shape. Every case below changes one
// of the facts such an answer depends on, after the read has been warmed.

const O: Record<string, any> = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 };
const HIT = ["a", "b", "c", "d", "e", "f", "g", "h"];
const MISS = ["a", "x", "c", "y", "e", "z", "g", "w"];

function hit(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += O[HIT[i & 7]];
  return s;
}
function miss(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = O[MISS[i & 7]];
    s += v === undefined ? 1 : v;
  }
  return s;
}
function constk(n: number): number {
  const K = "c";
  let s = 0;
  for (let i = 0; i < n; i++) s += O[K];
  return s;
}
function constBeforeUse(): string {
  try {
    // The read is in K's temporal dead zone and must throw.
    return String((O as any)[K2]);
  } catch (e) {
    return "tdz:" + (e as Error).constructor.name;
  } finally {
    // unreachable binding use keeps K2 a real local
  }
  const K2 = "d";
}

console.log("warm", hit(1000), miss(1000), constk(1000));
console.log("tdz", constBeforeUse());

// An absent key that appears on Object.prototype must be seen, and must go
// away again when it is deleted.
(Object.prototype as any).x = 100;
console.log("proto-add", miss(16), O["x"]);
delete (Object.prototype as any).x;
console.log("proto-del", miss(16), O["x"]);

// An own key added after the absent verdict was filed.
O.y = 50;
console.log("own-add", miss(16), O["y"]);
delete O.y;
console.log("own-del", miss(16), O["y"]);

// An own key whose VALUE is undefined is present, not absent.
O.z = undefined;
console.log("own-undef", "z" in O, miss(16), O["z"]);

// Keys taken from the object itself, and keys built at runtime.
const dyn: Record<string, number> = {};
for (let i = 0; i < 20; i++) dyn["k" + i] = i;
let t = 0;
for (let r = 0; r < 3; r++) for (const k of Object.keys(dyn)) t += dyn[k];
const five = "k" + String(5);
console.log("dyn", t, dyn[five], dyn["k" + "99"], dyn["k99"]);

// Long keys: past the short-string form, present and absent.
const LK: Record<string, any> = { someLongPropertyName: 1, anotherLongPropertyName: 2 };
const LKEYS = ["someLongPropertyName", "missingLongPropertyName", "anotherLongPropertyName"];
let lt = 0;
for (let i = 0; i < 300; i++) {
  const v = LK[LKEYS[i % 3]];
  lt += v === undefined ? 100 : v;
}
console.log("long", lt);

// Delete an own key after warming, then add a different one.
const M: Record<string, any> = { p: 1, q: 2 };
const MK = ["p", "q", "r"];
function readM(): string {
  return MK.map((k) => String(M[k])).join(",");
}
for (let i = 0; i < 50; i++) readM();
console.log("m0", readM());
delete M.p;
console.log("m1", readM());
M.r = 3;
console.log("m2", readM());
M.p = 4;
console.log("m3", readM());

// Numeric-string keys: elements on an array, named keys on an object.
const arr: any = [10, 20, 30];
const objn: Record<string, any> = { "0": "zero", "1": "one", x: "ex" };
const NK = ["0", "1", "2", "5", "length", "x"];
for (let r = 0; r < 3; r++) {
  console.log("num", r, NK.map((k) => String(arr[k])).join(","), NK.map((k) => String(objn[k])).join(","));
}

// Symbol and string keys with the same description never alias.
const sym = Symbol("s");
const os: any = { s: "str", [sym]: "sym" };
const SK: any[] = ["s", sym, "Symbol(s)"];
for (let r = 0; r < 3; r++) console.log("sym", r, SK.map((k) => String(os[k])).join(","));

// A null-prototype dictionary.
const np: any = Object.create(null);
np.a = 1;
const NPK = ["a", "toString", "b"];
for (let r = 0; r < 3; r++) console.log("null", r, NPK.map((k) => typeof np[k]).join(","));
np.b = 2;
console.log("null-add", NPK.map((k) => String(np[k])).join(","));

// Inherited Object.prototype members through a computed key.
const OPK = ["toString", "hasOwnProperty", "constructor", "__proto__", "valueOf", "nope"];
for (let r = 0; r < 3; r++) {
  console.log("objproto", r, OPK.map((k) => typeof (O as any)[k]).join(","));
}
console.log("ctor", (O as any)["constructor"] === Object, (O as any)["__proto__"] === Object.prototype);

// Primitive receivers.
const str = "abc";
const STRK = ["length", "1", "x"];
for (let r = 0; r < 2; r++) console.log("str", STRK.map((k) => String((str as any)[k])).join(","));
