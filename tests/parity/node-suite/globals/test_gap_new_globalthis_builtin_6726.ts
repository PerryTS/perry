// #6726 — `new globalThis.<Builtin>()` must construct the exact same intrinsic
// as `new <Builtin>()`. The member-expression callee (`globalThis.Set`) used to
// fall through to an empty-object placeholder with none of the builtin's
// methods, so `new globalThis.Set().has(x)` threw "has is not a function". The
// bare form (`new Set()`) and the local-alias form (`const S = globalThis.Set;
// new S()`) always worked — the failure was specific to the inline member
// callee.

// The exact reproduction from the issue.
const values = new globalThis.Set();
console.log(values.has(42));

// Set with an iterable argument.
const s2 = new globalThis.Set([1, 2, 2, 3]);
console.log(s2.size, s2.has(2), s2.has(9));

// Map, empty and from entries.
const m0 = new globalThis.Map();
m0.set("k", 7);
console.log(m0.get("k"), m0.has("k"));
const m1 = new globalThis.Map([
  ["a", 1],
  ["b", 2],
]);
console.log(m1.size, m1.get("a"), m1.get("b"));

// Date from a fixed timestamp (deterministic — no wall clock).
const d = new globalThis.Date(0);
console.log(d.getTime(), d.toISOString());

// WeakMap / WeakSet through the global object.
const wm = new globalThis.WeakMap();
const key = {};
wm.set(key, "v");
console.log(wm.get(key), wm.has(key));
const ws = new globalThis.WeakSet();
ws.add(key);
console.log(ws.has(key));

// Error family through the global object.
const te = new globalThis.TypeError("boom");
console.log(te instanceof Error, te instanceof TypeError, te.message, te.name);

// Array through the global object.
const arr = new globalThis.Array(1, 2, 3);
console.log(arr.length, arr[0], arr[2]);

// Promise through the global object still resolves.
new globalThis.Promise<number>((resolve) => resolve(11)).then((v) =>
  console.log("promise", v),
);

// Regression guards: the two forms that already worked must keep working.
const bare = new Set([5]);
console.log(bare.has(5));
const S = globalThis.Set;
const aliased = new S();
aliased.add(8);
console.log(aliased.has(8), aliased.size);

// #6726 review (CodeRabbit): `globalThis.<Builtin>` is the intrinsic even when a
// lexical binding shadows the bare name — a block-scoped `class Set {}` must NOT
// capture `new globalThis.Set()`.
{
  class Set {
    isLocal = true;
  }
  const shadowed = new globalThis.Set([1, 2]);
  console.log(shadowed.has(1), (shadowed as any).isLocal, shadowed.size);
  // The bare name still resolves to the local class in the same scope.
  console.log((new Set() as any).isLocal);
}

// Conversely, a locally-rebound `globalThis` is an ordinary object, so
// `new globalThis.Set()` must construct THAT object's `Set`, not the intrinsic.
{
  const globalThis = {
    Set: class {
      isFake = true;
    },
  };
  const fake = new globalThis.Set();
  console.log((fake as any).isFake);
}
