// Gap test: computed-member get on a primitive (number) receiver must not
// SIGSEGV. Regression for #5429 — `dayjs(1749820051142).format()` segfaulted
// because the timestamp float (0x4279_7696_70ec_6000) had its low 48 bits
// masked into a plausible-looking heap pointer by the computed-member get
// path, which the by-name runtime helper then dereferenced. Reading a
// property off a number returns `undefined` (or a bound primitive method),
// never a crash.
// Run: node --experimental-strip-types test_gap_5429_computed_get_primitive_receiver.ts

// The exact bit pattern from the issue: a large millisecond timestamp whose
// low 48 bits land in the platform heap range.
const ms: any = 1749820051142;
const key: any = "format";

// Computed get — the crashing path. Must match the dotted-get result.
console.log("computed missing:", ms[key]); // undefined
console.log("dotted missing:", ms.format); // undefined

// A primitive proto method read still resolves to a callable, not a crash.
console.log("toString is fn:", typeof ms["toString"]); // function
console.log("valueOf is fn:", typeof ms["valueOf"]); // function

// Floats, ints, and other low-48-bit shapes all stay safe.
const f: any = 3.141592653589793;
const i: any = 42;
console.log("float missing:", f["nope"], "int missing:", i["nope"]); // undefined undefined

// Real object / array / string receivers keep working (heap-pointer tags are
// still masked as before).
const obj: any = { a: 1, b: 2 };
const arr: any = [10, 20, 30];
const str: any = "hello";
const ka = "a";
console.log("obj get:", obj[ka], "arr get:", arr["1"], "str len:", str["length"]);

// Class-ref computed get (the test262 propertyHelper `C[name]` shape) is
// unaffected — its tag bits are still preserved.
class C {
  static answer = 42;
}
function readStatic(K: any, k: string) {
  return K[k];
}
console.log("classref get:", readStatic(C, "answer")); // 42
