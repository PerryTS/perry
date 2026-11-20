// GC rooting for typed-array CONSTRUCTOR SOURCES (#6981).
//
// `new Int32Array(src)` / `Int32Array.from(src)` hand the source to a runtime
// helper as a bare NaN-boxed argument. That argument lives only in a register /
// C-ABI stack slot, which is NOT a precise root, and the helper's own
// source-classification probes allocate before it ever dereferences the
// source (measured: the collection lands inside
// `js_util_types_is_generator_object`, which walks own properties). Under
// `PERRY_CONSERVATIVE_STACK_SCAN=off` a collection there swept the source and
// `clean_arr_ptr` nulled it, so `new Int32Array([7, 8])` silently produced a
// LENGTH-0 array — `a[0]` read `undefined` with no crash.
//
// Every construction below must survive a collection landing between the
// helper's entry and its first read of the source. Run under the evacuating
// precise-roots arm to gate it:
//   PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off

function first(a: any) {
  return a[0];
}

// 1. Array-literal temporary: the source has NO named binding at all, so the
//    C-ABI argument is its only reference for the whole classification.
const fromLiteral = new Int32Array([7, 8]);
console.log("literal:", fromLiteral.length, first(fromLiteral), fromLiteral[1]);

// 2. Reassigned binding (the #6981 minimal reproducer shape). The binding is
//    a root, but the value the CALL observed is what must survive.
let P = new Int32Array(4);
P[0] = 42;
console.log("before:", first(P));
P = new Int32Array([7, 8]);
console.log("after:", first(P), P.length);

// 3. Named plain-array source through a rooted binding.
const src = [11, 22, 33];
const fromNamed = new Int32Array(src);
console.log("named:", fromNamed.length, fromNamed[0], fromNamed[2], src.length);

// 4. `%TypedArray%.from` over a literal, one per width.
const f8 = Int8Array.from([-5, 100, -128, 7]);
const f16 = Int16Array.from([-5, 30000, -32768, 7]);
const f32 = Float32Array.from([1.5, -2.5, 3.5]);
const f64 = Float64Array.from([1.5, 2.25, -3.75]);
console.log("from:", f8.length, f8[2], f16[1], f32[0], f64[2]);

// 5. Array-like object source: runs the `length` / index Get path, which
//    allocates key strings while the source must stay live.
const arrayLike = { length: 3, 0: 5, 1: 6, 2: 7 };
const fromArrayLike = new Int32Array(arrayLike as any);
console.log("arraylike:", fromArrayLike.length, fromArrayLike[0], fromArrayLike[2]);

// 6. Iterable object source: runs USER code (`@@iterator`) with the source
//    live across every `next()`.
const iterable = {
  *[Symbol.iterator]() {
    yield 3;
    yield 1;
    yield 4;
    yield 1;
  },
};
const fromIterable = new Int32Array(iterable as any);
console.log("iterable:", fromIterable.length, fromIterable[0], fromIterable[3]);

// 7. Typed-array source (copy construction).
const fromTa = new Int32Array(fromIterable);
console.log("ta:", fromTa.length, fromTa[2]);

// 8. Sustained churn: build the same shapes repeatedly so a collection lands
//    inside the classification window many times over, not just once.
let acc = 0;
for (let i = 0; i < 400; i++) {
  const t = new Int32Array([i, i + 1, i + 2]);
  acc = (acc + t[0] + t[2] + t.length) | 0;
  const g = Uint16Array.from([i & 0xff, (i + 5) & 0xff]);
  acc = (acc + g[1]) | 0;
  const h = new Float64Array([i * 0.5, i * 1.5]);
  acc = (acc + h[1]) | 0;
}
console.log("churn:", acc);
console.log("still:", first(fromLiteral), fromNamed[1], f64[0], fromIterable[1]);
