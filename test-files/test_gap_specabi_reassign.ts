// Spec-ABI soundness: a reassigned binding must never prove TaPtr — after
// `P = ...`, every call must observe the NEW value through the boxed path.
function first(a: any) {
  return a[0];
}

let P = new Int32Array(4);
P[0] = 42;
console.log("before:", first(P));

P = new Int32Array([7, 8]);
console.log("after:", first(P));

// #6906: the source-level typed-array type is not a lifetime proof. An
// `as any` assignment can replace the binding with a plain array, and every
// later access must use runtime dispatch rather than stale typed-array
// lowering.
P = [99, 101] as any;
console.log("plain:", first(P), P[1], P.length);

P = new Int32Array(1);
P[0] = 123;
console.log("third:", first(P), P.length);
