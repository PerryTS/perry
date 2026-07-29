// Spec-ABI routing: a recursive function may specialize for its proven outer
// site while the recursive inner site (unproven args) keeps the boxed path;
// a function reference escaping as a VALUE (`const g = fill`) goes through
// the closure wrapper, which only ever calls the public boxed symbol.
// All routes must agree with Node byte-for-byte.
function fill(t: any, i: any, v: any): any {
  if (i >= 4) return t;
  t[i] = v ^ (i << 2);
  return fill(t, i + 1, v);
}

const T = new Int32Array(4);
fill(T, 0, 5);
console.log("direct:", T[0] | 0, T[1] | 0, T[2] | 0, T[3] | 0);

const g = fill;
const T2 = new Int32Array(4);
g(T2, 0, 9);
console.log("escaped:", T2[0] | 0, T2[1] | 0, T2[2] | 0, T2[3] | 0);

// The returned value is the same typed array (identity through the boxed
// return path).
console.log("identity:", fill(T, 4, 0) === T);
