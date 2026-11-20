// Regression test for "callable function value with properties attached":
// chalk / express / ms — three flavors of the same idiom.
//
// Before the fix, chalk's `Object.setPrototypeOf(closure, ClassProto)` line
// at module init threw `TypeError: value is not a function` because the
// generic `(Object).setPrototypeOf(...)` PropertyGet → Call fallback
// dispatched a non-callable (Perry's `Object` isn't a runtime object with
// methods). `Object.defineProperties` had the same shape. The fix adds
// dedicated HIR variants for both — `Object.setPrototypeOf` returns the
// target (matching the spec), `Object.defineProperties` desugars to a
// sequence of `defineProperty` calls (statically when the descriptor is
// an object literal, dynamically otherwise via a runtime helper).
//
// `js_object_entries/keys/values` also gained an is_valid_obj_ptr guard:
// chalk's `Object.entries(ansiStyles)` on a cross-module default-import
// could land on a non-pointer low-48-bit value (~0x1 from
// `TAG_UNDEFINED & POINTER_MASK`), and the unguarded `(*obj).keys_array`
// load SIGSEGV'd at 0x14. Now those callers see an empty array instead.

declare function gc(): void;

function hasGc(): boolean {
    return typeof gc === "function";
}

// ---------- 1. Function with directly-attached properties (ms shape) ----------
function base(s: string): string {
    return "base:" + s;
}
(base as any).extra = "hello";
(base as any).greet = (n: string) => "greet:" + n;

console.log("typeof base:", typeof base);
console.log("base call:", base("x"));
console.log("base.extra:", (base as any).extra);
console.log("base.greet call:", (base as any).greet("y"));
if (hasGc()) gc();
console.log("base.extra after gc:", (base as any).extra);
console.log("base.greet after gc:", (base as any).greet("z"));

// ---------- 2. Object.assign(fn, props) (ms / common decorate shape) ----------
const fn1 = function (s: string): string {
    return "fn1:" + s;
};
const decorated: any = Object.assign(fn1, {
    tag: "TAG",
    inner: (n: string) => "inner:" + n,
});

console.log("typeof decorated:", typeof decorated);
console.log("decorated call:", decorated("a"));
console.log("decorated.tag:", decorated.tag);
console.log("decorated.inner call:", decorated.inner("b"));

// ---------- 3. Object.setPrototypeOf(closure, Foo.prototype) (chalk shape) ----------
class Foo {}

function createFactory(): any {
    const chalkLike: any = (...strings: string[]) => strings.join(" ");
    Object.setPrototypeOf(chalkLike, Foo.prototype);
    return chalkLike;
}

const chalkLike = createFactory();
console.log("typeof chalkLike:", typeof chalkLike);
console.log("chalkLike call:", chalkLike("hello", "world"));

// ---------- 4. Object.defineProperties on a regular object (chalk shape) ----------
const target: any = {};
Object.defineProperties(target, {
    a: { value: 1, enumerable: true },
    b: { value: 2, enumerable: true },
});
console.log("target.a:", target.a);
console.log("target.b:", target.b);

// ---------- 5. Object.entries(undefined) no-ops instead of SIGSEGV-ing ----------
// (Cross-module default-import that resolved to TAG_UNDEFINED used to crash.)
const empty = Object.entries(undefined as any);
console.log("Object.entries(undefined) length:", empty.length);
const emptyKeys = Object.keys(undefined as any);
console.log("Object.keys(undefined) length:", emptyKeys.length);
const emptyVals = Object.values(undefined as any);
console.log("Object.values(undefined) length:", emptyVals.length);
