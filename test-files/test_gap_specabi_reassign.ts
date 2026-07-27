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

const arr: any = [99];
P = arr;
console.log("plain:", first(P));
