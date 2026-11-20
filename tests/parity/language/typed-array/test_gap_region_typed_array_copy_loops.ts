// Loops over typed arrays indexed by the counter, with an untyped bound
// (`n` declared `any`, so the region's counter-bound scope would admit
// them). A region over a typed array is served only for an owning
// Float64Array; an integer kind, Float32Array and Buffer form none. Every
// output must equal node's, and each loop must compile (a region guard once
// named an undefined global here).

function cpI32(dst: Int32Array, src: Int32Array, n: any): void {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
function cpU8(dst: Uint8Array, src: Uint8Array, n: any): void {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
function cpF32(dst: Float32Array, src: Float32Array, n: any): void {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
function cpF64(dst: Float64Array, src: Float64Array, n: any): void {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
function cpBuf(dst: Buffer, src: Buffer, n: any): void {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
function sumI32(a: Int32Array, n: any): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += a[i];
  return s;
}
function sumF64(a: Float64Array, n: any): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += a[i] * 0.5;
  return s;
}

const N = 9;
const i32a = new Int32Array(N), i32b = new Int32Array(N);
const u8a = new Uint8Array(N), u8b = new Uint8Array(N);
const f32a = new Float32Array(N), f32b = new Float32Array(N);
const f64a = new Float64Array(N), f64b = new Float64Array(N);
const bufa = Buffer.alloc(N), bufb = Buffer.alloc(N);
for (let i = 0; i < N; i++) {
  i32b[i] = i * 3 - 7;
  u8b[i] = i * 31;
  f32b[i] = i / 3;
  f64b[i] = i * 1.25 - 2;
  bufb[i] = 200 + i;
}
cpI32(i32a, i32b, N);
cpU8(u8a, u8b, N);
cpF32(f32a, f32b, N);
cpF64(f64a, f64b, N);
cpBuf(bufa, bufb, N);
console.log("i32", i32a.join(","));
console.log("u8", u8a.join(","));
console.log("f32", Array.from(f32a, (x) => x.toFixed(4)).join(","));
console.log("f64", f64a.join(","));
console.log("buf", Array.from(bufa).join(","));
// A bound that is not a Number, or past the end.
cpI32(i32a, i32b, "4");
console.log("i32 str bound", i32a.join(","));
cpF64(f64a, f64b, N + 5);
console.log("f64 past end", f64a.join(","));
console.log("sums", sumI32(i32b, N), sumF64(f64b, N), sumF64(f64b, undefined));
// A typed array passed where another kind is declared.
cpI32(i32a, f64b as any, N);
console.log("i32 from f64", i32a.join(","));
cpF64(f64a, i32b as any, N);
console.log("f64 from i32", f64a.join(","));
cpF64(f64a, [1, 2, 3, 4, 5, 6, 7, 8, 9] as any, N);
console.log("f64 from array", f64a.join(","));

// fannkuch-redux (the bench's copy loops over Int32Array), n = 7.
function fannkuch(n: number): number[] {
  const perm = new Int32Array(n);
  const perm1 = new Int32Array(n);
  const count = new Int32Array(n);
  let maxFlips = 0, checksum = 0, permCount = 0;
  for (let i = 0; i < n; i++) perm1[i] = i;
  let r = n;
  while (true) {
    while (r !== 1) { count[r - 1] = r; r--; }
    for (let i = 0; i < n; i++) perm[i] = perm1[i];
    let flips = 0;
    let k = perm[0];
    while (k !== 0) {
      let i = 0, j = k;
      while (i < j) {
        const t = perm[i]; perm[i] = perm[j]; perm[j] = t;
        i++; j--;
      }
      flips++;
      k = perm[0];
    }
    if (flips > maxFlips) maxFlips = flips;
    checksum += (permCount % 2 === 0) ? flips : -flips;
    while (true) {
      if (r === n) return [checksum, maxFlips];
      const p0 = perm1[0];
      for (let i = 0; i < r; i++) perm1[i] = perm1[i + 1];
      perm1[r] = p0;
      count[r] = count[r] - 1;
      if (count[r] > 0) break;
      r++;
    }
    permCount++;
  }
}
const res = fannkuch(7);
console.log(res[0] + "\nPfannkuchen(7) = " + res[1]);
