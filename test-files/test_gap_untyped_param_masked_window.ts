// #6750 follow-up: masked-window array reads through UNTYPED (any) function
// parameters — the bcryptjs Blowfish S-box shape. The dense range-loop
// versioning probes the runtime receiver at loop entry (typed-array tiers +
// plain-array guard tiers); every shape below must produce byte-identical
// output to Node whether a fast copy fires or the loop deopts to the slow
// path.

// The canonical hot shape: S[i & mask] reduction over an untyped param.
function sumMasked(S: any, n: number): number {
  let s = 0 | 0;
  for (let i = 0; i < n; i++) s = (s + S[i & 15]) | 0;
  return s;
}

// f64-context reads (no |0 on the load): exercises the unsigned/float
// materialization of the typed-array tiers.
function sumMaskedF64(S: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 15];
  return s;
}

// Two untyped params in one loop (bcryptjs reads P and S together).
function sumTwoArrays(P: any, S: any, n: number): number {
  let s = 0 | 0;
  for (let i = 0; i < n; i++) s = (s + P[i & 7] + S[i & 15]) | 0;
  return s;
}

// Reassigning the binding inside the loop must keep full JS semantics.
function sumWithReassign(S: any, T: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += S[i & 15];
    if (i === 8) S = T;
  }
  return s;
}

const i32 = new Int32Array(16);
const u32 = new Uint32Array(16);
const f64 = new Float64Array(16);
const u8 = new Uint8Array(16);
const plain: number[] = new Array(16);
for (let i = 0; i < 16; i++) {
  i32[i] = (i * 2654435761) | 0; // mixed-sign int32 values
  u32[i] = (i * 2654435761) >>> 0; // values above i32::MAX — must stay unsigned
  f64[i] = i * 1.5 + 0.25; // fractional — width 8, no i32 tier
  u8[i] = (i * 37) & 0xff; // unsupported probe kind — must deopt cleanly
  plain[i] = (i * 2654435761) | 0;
}
const small = new Int32Array(8); // shorter than the [0,16) window — probe must reject
for (let i = 0; i < 8; i++) small[i] = i + 1;
const holey: number[] = new Array(16); // hole inside the window — dense guard must reject
for (let i = 0; i < 16; i++) if (i !== 5) holey[i] = i + 1;
const mixed: any[] = new Array(16); // non-number element — guard must reject
for (let i = 0; i < 16; i++) mixed[i] = i === 7 ? 'x' : i + 1;
const offsetView = new Int32Array(new ArrayBuffer(128), 32, 16); // byteOffset view
for (let i = 0; i < 16; i++) offsetView[i] = (i + 1) * 3;

console.log('i32:', sumMasked(i32, 1000));
console.log('u32:', sumMasked(u32, 1000));
console.log('f64:', sumMasked(f64, 1000));
console.log('u8:', sumMasked(u8, 1000));
console.log('plain:', sumMasked(plain, 1000));
console.log('i32 f64-ctx:', sumMaskedF64(i32, 1000));
console.log('u32 f64-ctx:', sumMaskedF64(u32, 1000)); // unsigned values summed as doubles
console.log('f64 f64-ctx:', sumMaskedF64(f64, 1000));
console.log('plain f64-ctx:', sumMaskedF64(plain, 1000));
console.log('short window:', sumMasked(small, 1000)); // OOB reads -> undefined -> NaN|0
console.log('short f64-ctx:', sumMaskedF64(small, 1000)); // NaN
console.log('holey:', sumMaskedF64(holey, 1000)); // hole -> undefined -> NaN
console.log('mixed elems:', sumMasked(mixed, 1000)); // 'x' + num coercions
console.log('offset view:', sumMasked(offsetView, 1000));
console.log('two arrays i32/i32:', sumTwoArrays(i32, i32, 1000));
console.log('two arrays plain/plain:', sumTwoArrays(plain, plain, 1000));
console.log('two arrays MIXED plain/i32:', sumTwoArrays(plain, i32, 1000)); // heterogeneous -> deopt
console.log('two arrays MIXED i32/f64:', sumTwoArrays(i32, f64, 1000)); // TA kinds disagree -> deopt
console.log('reassign mid-loop:', sumWithReassign(i32, plain, 32));

// Polymorphic call site: the same loop re-probes per entry.
const receivers: any[] = [i32, plain, u32, f64, i32, 'abcdefghijklmnop', { 3: 41 }];
for (const r of receivers) {
  console.log('poly:', sumMaskedF64(r, 8));
}

// Detached backing: views over a transferred ArrayBuffer read undefined.
const buf = new ArrayBuffer(64);
const view = new Int32Array(buf, 0, 16);
for (let i = 0; i < 16; i++) view[i] = i + 1;
console.log('pre-detach:', sumMasked(view, 100));
(buf as any).transfer();
console.log('post-detach:', sumMaskedF64(view, 100)); // length 0 -> undefined reads -> NaN

// ---- STRAIGHT-LINE region shapes (the unrolled bcryptjs _encipher form):
// >= 8 masked reads with no loop, exercising the region versioner.

// The _encipher shape: untyped locals fed from an untyped array, then a
// run of masked reads mixed through bitwise ops.
function encipherish(lr: any, off: any, P: any, S: any): number {
  let n = 0;
  let l = lr[off];
  let r = lr[off + 1];
  l ^= P[0];
  n = S[l >>> 28];
  n += S[8 | ((l >> 16) & 7)];
  n ^= S[(l >> 8) & 15];
  n += S[l & 15];
  r ^= n ^ P[1];
  n = S[r >>> 28];
  n += S[8 | ((r >> 16) & 7)];
  n ^= S[(r >> 8) & 15];
  n += S[r & 15];
  l ^= n ^ P[2];
  return ((l | 0) + (r | 0) + (n | 0)) | 0;
}

// Region with the binding REASSIGNED mid-run: T's reads must see T.
function regionReassign(S: any, T: any): number {
  let a = 0;
  a += S[1 & 15];
  a += S[2 & 15];
  a += S[3 & 15];
  a += S[4 & 15];
  S = T;
  a += S[5 & 15];
  a += S[6 & 15];
  a += S[7 & 15];
  a += S[8 & 15];
  return a;
}

// Region inside try/catch (privatization disabled): a mid-region throw via
// a valueOf that returns a non-number must leave the partial sums correct.
function regionInTry(S: any, poison: any): string {
  let a = 0;
  try {
    a += S[1 & 15];
    a += S[2 & 15];
    a += S[3 & 15];
    a += S[4 & 15];
    a += S[5 & 15];
    a += S[6 & 15];
    a += S[7 & 15];
    a += S[8 & 15];
    a ^= poison;
  } catch (e: any) {
    return 'caught a=' + a + ' ' + e.message;
  }
  return 'ok a=' + a;
}

// BigInt flowing through a region must NOT be refined to Number: `x * x`,
// `-x`, `~x` on BigInts yield BigInts (a pure-unknown operator proves
// nothing), and mixing a BigInt with a guard-proven numeric element read
// must throw TypeError in fast and slow copies alike.
function regionBigintSide(S: any, b: any): string {
  let x = b;
  let y = b;
  let acc = 0;
  acc += S[1 & 15];
  acc += S[2 & 15];
  acc += S[3 & 15];
  acc += S[4 & 15];
  x = x * x;
  y = -y;
  acc += S[5 & 15];
  acc += S[6 & 15];
  acc += S[7 & 15];
  acc += S[8 & 15];
  return acc + ' ' + x + ' ' + y;
}
function regionBigintMixThrow(S: any, b: any): string {
  let x = b;
  let acc = 0;
  try {
    acc += S[1 & 15];
    acc += S[2 & 15];
    acc += S[3 & 15];
    acc += S[4 & 15];
    acc += S[5 & 15];
    acc += S[6 & 15];
    acc += S[7 & 15];
    x = x * S[8 & 15];
  } catch (e: any) {
    return 'caught acc=' + acc + ' x=' + x;
  }
  return 'no-throw acc=' + acc + ' x=' + x;
}

const lrPlain = [11, 22];
console.log('encipherish i32:', encipherish(lrPlain, 0, i32, i32));
console.log('encipherish plain:', encipherish(lrPlain, 0, plain, plain));
console.log('encipherish mixed:', encipherish(lrPlain, 0, plain, i32));
console.log('encipherish u32:', encipherish(lrPlain, 0, u32, u32));
console.log('encipherish f64:', encipherish(lrPlain, 0, f64, f64));
console.log('encipherish holey:', encipherish(lrPlain, 0, holey, holey));
console.log('encipherish short:', encipherish(lrPlain, 0, small, small));
console.log('region reassign:', regionReassign(i32, plain), regionReassign(plain, u32));
console.log('region try ok:', regionInTry(i32, 3));
const thrower = {
  valueOf() {
    throw new Error('boom');
  },
};
console.log('region try throw:', regionInTry(i32, thrower));
console.log('region bigint side:', regionBigintSide(i32, 3n));
console.log('region bigint side plain:', regionBigintSide(plain, 5n));
console.log('region bigint mix:', regionBigintMixThrow(i32, 7n));
console.log('region bigint mix num:', regionBigintMixThrow(i32, 2)); // number path completes
