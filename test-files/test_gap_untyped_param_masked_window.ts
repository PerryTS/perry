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
