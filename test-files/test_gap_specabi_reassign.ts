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

// NOTE: reassigning P to a PLAIN array (`P = [99]`) trips a PRE-EXISTING
// declared-type staleness bug on main (reads through the reassigned binding
// return undefined) that is independent of the spec-ABI routing this test
// gates — kept out so the test can gate on the routing property.
P = new Int32Array(1);
P[0] = 99;
console.log("third:", first(P), P.length);
