// Every evaluation of a closure literal creates a fresh function object
// (ECMA-262 OrdinaryFunctionCreate). The captured-closure singleton cache
// conflated two evaluations of the same literal whenever their capture bits
// matched — observable through `===`, and fatally through
// `Object.setPrototypeOf(a, b)`, which for a conflated pair is a SELF-set
// and throws "TypeError: Cyclic __proto__ value". pi's esbuild bundle died
// at boot on exactly that wiring (setPrototypeOf(wrapped, original) where
// both came from one arrow literal).

// Case 1: arrow literal capturing the same constant value in both evals.
const K = { tag: 1 };
function mkCaptured() {
  return () => K;
}
const c1 = mkCaptured();
const c2 = mkCaptured();
console.log("captured-arrow distinct:", c1 !== c2);
Object.setPrototypeOf(c1, c2);
console.log("captured-arrow proto:", Object.getPrototypeOf(c1) === c2);

// Case 2: captureless arrow literal.
function mkBare() {
  return () => 1;
}
const b1 = mkBare();
const b2 = mkBare();
console.log("bare-arrow distinct:", b1 !== b2);
Object.setPrototypeOf(b1, b2);
console.log("bare-arrow proto:", Object.getPrototypeOf(b1) === b2);

// Case 3: expandos must not alias across evaluations.
const e1: any = mkBare();
const e2: any = mkBare();
e1.mark = "one";
e2.mark = "two";
console.log("expando isolation:", e1.mark === "one" && e2.mark === "two");

// Case 4: a GENUINE self-set must still throw (the cycle check stays).
const solo: any = mkBare();
let threw = false;
try {
  Object.setPrototypeOf(solo, solo);
} catch (err: any) {
  threw = err instanceof TypeError;
}
console.log("genuine self-set throws:", threw);
