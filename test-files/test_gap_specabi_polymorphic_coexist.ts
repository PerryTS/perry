// Representation-selection Phase 2 (spec-ABI) routing: statically-proven call
// sites may take the specialized raw-ABI entry while polymorphic/unproven
// sites keep the boxed public path — every route must produce Node-identical
// output, including an out-of-bounds write through the specialized entry
// (silent no-op) and a differently-shaped typed-array site (tuple mismatch →
// boxed fallback).
function mix(t: any, o: any, a: any, b: any) {
  let x = t[o];
  x ^= a[0];
  x += a[1];
  x ^= b[x & 3];
  t[o] = x;
  t[o + 1] = (x >>> 3) | 0;
  return t;
}

const T = new Int32Array(4);
const A = new Int32Array([0x1234, 7]);
const B = new Int32Array([11, 22, 33, 44]);
T[0] = 0x0f0f0f0f | 0;
for (let i = 0; i < 1000; i++) mix(T, 0, A, B);
console.log("proven:", T[0] | 0, T[1] | 0, T[2] | 0, T[3] | 0);

// Same proven shape, but the trailing write lands out of bounds (o+1 == 4):
// an OOB typed-array store is a silent no-op on every route.
mix(T, 3, A, B);
console.log("oob-write:", T[0] | 0, T[1] | 0, T[2] | 0, T[3] | 0);

// Polymorphic site: plain arrays through the very same function.
const t2: any = [9, 8, 7, 6];
const a2: any = [5, 6];
const b2: any = [1, 1, 1, 1];
mix(t2, 0, a2, b2);
console.log("poly:", t2[0], t2[1], t2[2], t2[3]);

// Differently-shaped typed-array site (length 1): tuple mismatch keeps the
// boxed path; reading t[5] yields undefined, so x goes NaN and the stores
// coerce it to 0.
const small = new Int32Array(1);
small[0] = 123;
mix(small, 5, A, B);
console.log("mismatch:", small[0] | 0);
