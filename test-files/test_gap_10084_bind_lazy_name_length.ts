// Gap test for #10084: `Function.prototype.bind` eagerly materialized the
// bound function's `.name` string and `set_builtin_property_attrs` records
// for `.name`/`.length` on every call, even when neither was ever read. The
// fix defers building "bound " + name (and the dynamic-prop cache entry) to
// first actual `.name` read, and drops the now-redundant attrs calls
// entirely. This test pins the full spec surface the fix must preserve.

function add(this: { bias: number }, a: number, b: number): number {
  return this.bias + a + b;
}

// A bind that never reads .name/.length must still work correctly when
// called — the lazy-metadata path must not affect invocation.
const bound = add.bind({ bias: 10 }, 1);
console.log("call result:", bound(2)); // 13

// `.name` reads "bound " + target name, and `.length` = max(0, target.length
// - boundArgs.length).
console.log("name:", bound.name); // bound add
console.log("length:", bound.length); // 1

// Chained bind: reading the outer bound function's name must recurse through
// the inner (also-lazy) bound function's own name synthesis.
const chained = add.bind({ bias: 0 }).bind({ bias: 0 });
console.log("chained name:", chained.name); // bound bound add
console.log("chained length:", chained.length); // 2

// `Object.defineProperty` override on the target, observed through bind.
function target() {}
Object.defineProperty(target, "name", { value: "renamedTarget" });
console.log("override name:", target.bind().name); // bound renamedTarget

// Non-string override on the target falls back to the empty string, not the
// declared name. Matches Test262's bind/instance-name-non-string.js exactly:
// the function expression is passed directly to `defineProperty` (never
// bound to a variable, so no NamedEvaluation name inference applies) — a
// truly nameless target sidesteps a pre-existing, unrelated ambiguity
// between "no name override" and "name explicitly set to `undefined`" (both
// read back as the same sentinel) that a named target would otherwise hit.
const anon = Object.defineProperty(function () {}, "name", {
  value: undefined,
});
console.log("non-string override name:", anon.bind().name); // bound

// name/length are non-enumerable: absent from Object.keys/for-in, and
// hasOwnProperty still reports them present.
const enumKeys: string[] = [];
for (const k in bound) enumKeys.push(k);
console.log("for-in keys:", JSON.stringify(enumKeys)); // []
console.log("Object.keys:", JSON.stringify(Object.keys(bound))); // []
console.log(
  "hasOwnProperty name/length:",
  bound.hasOwnProperty("name"),
  bound.hasOwnProperty("length"),
); // true true

// Property descriptor attributes match spec defaults even though bind never
// wrote them explicitly.
const desc = Object.getOwnPropertyDescriptor(bound, "name")!;
console.log(
  "name descriptor:",
  desc.value,
  desc.writable,
  desc.enumerable,
  desc.configurable,
); // bound add false false true
const lenDesc = Object.getOwnPropertyDescriptor(bound, "length")!;
console.log(
  "length descriptor:",
  lenDesc.value,
  lenDesc.writable,
  lenDesc.enumerable,
  lenDesc.configurable,
); // 1 false false true

// A write to .name/.length throws under strict mode (non-writable) —
// this file runs as an ES module, so every write attempt is strict.
let nameWriteThrew = false;
try {
  (bound as any).name = "clobbered";
} catch {
  nameWriteThrew = true;
}
let lengthWriteThrew = false;
try {
  (bound as any).length = 99;
} catch {
  lengthWriteThrew = true;
}
console.log(
  "write threw / unchanged:",
  nameWriteThrew,
  lengthWriteThrew,
  bound.name,
  bound.length,
); // true true bound add 1

// Beyond-u32 (here +Infinity) target length forwards through bind's own
// dynamic-prop fallback path.
function infLen() {}
Object.defineProperty(infLen, "length", { value: Infinity });
console.log("infinity length:", infLen.bind().length); // Infinity

// A class target still binds correctly and reports its name lazily.
class Widget {
  static tag = "w";
}
const BoundWidget = Widget.bind(null);
console.log("class bind name:", BoundWidget.name); // bound Widget

// console.log on a bound function whose .name was NEVER read must still
// display the synthesized name (formatting must not bypass the lazy path).
const neverRead = add.bind({ bias: 0 }, 1);
console.log("display:", neverRead);
