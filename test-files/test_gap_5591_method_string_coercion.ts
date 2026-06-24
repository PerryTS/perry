// #5591: string-coercing a class/object method via `+` or a template literal
// segfaulted. The `+` operator's fully-dynamic ToPrimitive path bit-cast the
// method closure to an ObjectHeader and read+called a bogus `valueOf` slot
// (`"" + C.prototype.method` → EXC_BAD_ACCESS). Verified against
// `node --experimental-strip-types`.
//
// The asserts below only print values that agree between Perry and Node:
// the *exact* function-source text of a method differs (Perry emits the
// NativeFunction form, Node the source), so we check shape/consistency, not
// the literal string.

class C {
  m() {
    return 1;
  }
  static s() {}
  get g() {
    return 2;
  }
}

// (1) Coercion completes (previously segfaulted) and yields a non-empty string.
const viaPlus = "" + C.prototype.m;
console.log(typeof viaPlus, viaPlus.length > 0);

// (2) `+` coercion agrees with explicit `.toString()` within the runtime.
console.log(("" + C.prototype.m) === C.prototype.m.toString());

// (3) Template-literal and String() coercion of a static method also work.
console.log(typeof `${C.s}`, typeof String(C.s));

// (4) Getter functions coerce too.
const gd = Object.getOwnPropertyDescriptor(C.prototype, "g")!;
console.log(typeof ("" + gd.get));

// (5) An object-literal shorthand method coerces without crashing.
const o = { meth() {} };
console.log(typeof ("" + o.meth));

// (6) A user-defined `valueOf` on a function still wins over toString in the
//     `+` path (default/number hint): `1 + f` is 43, `"" + f` is "42".
function f() {}
(f as unknown as { valueOf: () => number }).valueOf = () => 42;
console.log(1 + f, "" + f);

// (7) Numeric coercion of a method is NaN (valueOf returns the function, so
//     ToNumber falls through to the source string → NaN).
console.log(Number(C.prototype.m));
