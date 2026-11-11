// #6486: an array grown by `push` past its inline capacity (16) inside a
// `for-of` loop in a function moves via `js_array_grow`, leaving a
// GC_FLAG_FORWARDED header at the old address. The caller's variable still
// holds the stale pre-grow pointer; `js_typed_array_new_from_array` raw-read
// `(*arr).length` without following the forwarding chain, so
// `new Float32Array(arr)` saw the forwarding pointer's bytes as the element
// count (garbage, varies per run) while `arr.length` and indexed reads —
// which do follow the chain via `clean_arr_ptr` — stayed correct.

// The exact minimal repro from the issue: fn + for-of + 3-arg push × 6 iter.
function fill(out: number[], a: number[]): void {
  const vs = [a, a, a, a, a, a];
  for (const v of vs) out.push(v[0], v[1], v[2]);
}
const verts: number[] = [];
fill(verts, [1, 2, 3]);
const vd = new Float32Array(verts);
console.log(verts.length, vd.length);
console.log(vd[0], vd[1], vd[2], vd[15], vd[16], vd[17]);

// Single-arg pushes corrupt the same way (arity is irrelevant).
function fillOnes(out: number[], a: number[]): void {
  const vs = [a, a, a];
  for (const v of vs) {
    out.push(v[0]);
    out.push(v[1]);
    out.push(v[2]);
    out.push(v[0]);
    out.push(v[1]);
    out.push(v[2]);
  }
}
const ones: number[] = [];
fillOnes(ones, [4, 5, 6]);
const od = new Float64Array(ones);
console.log(ones.length, od.length, od[17]);

// More iterations of a smaller push (3-arg × 6 iter) — same trigger shape.
function fillIter(out: number[], a: number[]): void {
  const vs = [a, a, a, a, a, a];
  for (const v of vs) out.push(v[2], v[1], v[0]);
}
const many: number[] = [];
fillIter(many, [7, 8, 9]);
const md = new Int32Array(many);
console.log(many.length, md.length, md[0], md[17]);

// Uint8Array goes through the same from-array constructor path.
const u8 = new Uint8Array(verts);
console.log(u8.length, u8[0], u8[17]);

// Sparse shapes: logical length exceeds dense capacity (clean_arr_ptr's
// sparse exception) — the byte snapshot must bounds-check per element via
// js_array_get_f64 instead of walking `length` raw slots (OOB read).
const sparseLen: number[] = [1, 2, 3];
sparseLen.length = 21;
const sl = new Uint8Array(sparseLen);
console.log(sl.length, sl[0], sl[1], sl[2], sl[3], sl[20]);

const sparseFar: number[] = [5];
sparseFar[40] = 9;
const sf = new Uint8Array(sparseFar);
console.log(sf.length, sf[0], sf[1], sf[40]);

const sparseBuf: number[] = [7, 8];
sparseBuf.length = 30;
const sb = Buffer.from(sparseBuf);
console.log(sb.length, sb[0], sb[1], sb[2], sb[29]);

// Below-capacity control: must keep working (never corrupted before either).
function fillSmall(out: number[], a: number[]): void {
  const vs = [a, a, a];
  for (const v of vs) out.push(v[0], v[1], v[2]);
}
const small: number[] = [];
fillSmall(small, [1, 2, 3]);
const sd = new Float32Array(small);
console.log(small.length, sd.length, sd[8]);
