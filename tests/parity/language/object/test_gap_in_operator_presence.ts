// `in` presence across the receivers whose keys do not live in an ordinary
// keys array, and across the mutations that must change the answer: the walk
// resolves a recorded prototype only at its class-vtable fallback, so every
// one of these has to keep answering what Node answers.

class Base {
  baseField: number;
  constructor() {
    this.baseField = 1;
  }
  baseMethod(): number {
    return 1;
  }
}
class Derived extends Base {
  ownField: string;
  constructor() {
    super();
    this.ownField = "x";
  }
  derivedMethod(): number {
    return 2;
  }
}

const d = new Derived();
console.log(
  "class",
  "ownField" in d,
  "baseField" in d,
  "derivedMethod" in d,
  "baseMethod" in d,
  "toString" in d,
  "nope" in d,
);

// A plain object: own, inherited, absent, and index-like keys.
const plain: any = { a: 1, b: undefined };
console.log("plain", "a" in plain, "b" in plain, "c" in plain, "toString" in plain, "0" in plain);

// delete must flip presence, and re-adding must flip it back.
console.log("delete", "a" in plain, delete plain.a, "a" in plain, ((plain.a = 9), "a" in plain));

// A wide object crosses the keys-index threshold.
const wide: any = {};
for (let i = 0; i < 40; i++) wide["k" + i] = i;
console.log("wide", "k0" in wide, "k39" in wide, "k40" in wide, delete wide.k39, "k39" in wide);

// Object.setPrototypeOf records a prototype: presence must follow the new
// chain, and stop following the old one.
const protoA: any = { onA: 1 };
const protoB: any = { onB: 2 };
const movable: any = Object.create(protoA);
console.log("proto-a", "onA" in movable, "onB" in movable);
Object.setPrototypeOf(movable, protoB);
console.log("proto-b", "onA" in movable, "onB" in movable);
Object.setPrototypeOf(movable, null);
console.log("proto-null", "onB" in movable, "toString" in movable);

// A prototype gaining or losing a key after the first lookup.
const parent: any = {};
const child: any = Object.create(parent);
console.log("late-proto", "later" in child, ((parent.later = 1), "later" in child));
console.log("late-delete", (delete parent.later, "later" in child));

// Accessors, non-enumerable and symbol keys.
const withAccessor: any = {};
Object.defineProperty(withAccessor, "acc", { get: () => 1, configurable: true });
Object.defineProperty(withAccessor, "hidden", { value: 2, enumerable: false });
const sym = Symbol("s");
withAccessor[sym] = 3;
console.log("descriptors", "acc" in withAccessor, "hidden" in withAccessor, sym in withAccessor);

// Arrays: indices, length, holes, and inherited members.
const arr: any = [1, 2, 3];
arr[7] = 8;
console.log("array", 0 in arr, 2 in arr, 5 in arr, 7 in arr, "length" in arr, "map" in arr);

// A Proxy answers through its has trap.
const proxied: any = new Proxy({ real: 1 }, { has: (t, k) => k === "virtual" || k in t });
console.log("proxy", "virtual" in proxied, "real" in proxied, "other" in proxied);

// Built-in receivers whose members are not ordinary keys.
console.log("builtins", "size" in new Map(), "has" in new Set(), "byteLength" in new ArrayBuffer(8));
// NOTE: `"call" in function f(){}` is a separate, pre-existing gap — Perry
// answers false where Node answers true, on this commit's parent as well — so
// it is deliberately not asserted here; inherited Function.prototype members
// are not this fixture's subject.
console.log("fn", "length" in function g(a: number) {}, "name" in function h() {}, "x" in { x: 1 });

// process.env is backed by the OS, not a keys array.
(process.env as any).PERRY_IN_PROBE = "1";
console.log("env", "PERRY_IN_PROBE" in process.env, "PERRY_ABSENT_XYZ" in process.env);
console.log("env-proto", "toString" in process.env);

// A native-module namespace exposes virtual keys.
import * as pathMod from "node:path";
console.log("module", "join" in pathMod, "definitelyNot" in (pathMod as any));

// Numeric and coercing keys go through ToPropertyKey.
const numeric: any = { 307: "a", "1.5": "b" };
console.log("coerce", 307 in numeric, "307" in numeric, 1.5 in numeric, "1.5" in numeric);

// The same key, asked in a hot loop, must not drift.
let hits = 0;
for (let i = 0; i < 2000; i++) if ("ownField" in d) hits++;
console.log("hot", hits);
