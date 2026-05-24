// 5x5 Gaussian blur on a 3840x2160 RGB image.
// In-memory input + output checksum; see workload README.md for rationale.
//
// IMPLEMENTATION NOTE — module-level buffers:
// Passing a 24MB Buffer through a function parameter triggers a GC-scan gap
// in Perry 0.5.29 — the param lands in a callee-saved register that the
// conservative stack scan can miss, and a Buffer.alloc() inside the function
// body can trigger a collect that sweeps the param buffer mid-blur (SIGSEGV).
// Hoisting both buffers to module globals sidesteps this because v0.5.28
// registers all module-level globals as explicit GC roots. The compute work
// is unchanged; this is purely a scoping workaround.

const W = 3840;
const H = 2160;
const N = W * H * 3;
const SEED = 0x9E3779B9 >>> 0;

const KERNEL: number[][] = [
  [1, 4, 7, 4, 1],
  [4, 16, 26, 16, 4],
  [7, 26, 41, 26, 7],
  [4, 16, 26, 16, 4],
  [1, 4, 7, 4, 1],
];
const KSUM = 273;

const src = Buffer.alloc(N);

function clampIdx(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}
function clampU8(v: number): number {
  if (v < 0) return 0;
  if (v > 255) return 255;
  return v | 0;
}
function imul32(a: number, b: number): number {
  const aHi = (a >>> 16) & 0xffff;
  const aLo = a & 0xffff;
  const bHi = (b >>> 16) & 0xffff;
  const bLo = b & 0xffff;
  return ((aLo * bLo) + (((aHi * bLo + aLo * bHi) << 16) >>> 0)) | 0;
}

// ---- generate input (gradient + xorshift noise) ----
for (let y = 0; y < H; y++) {
  const rowBase = ((y * 255) / H) | 0;
  const off0 = y * W * 3;
  for (let x = 0; x < W; x++) {
    const off = off0 + x * 3;
    src[off] = ((x * 255) / W) | 0;
    src[off + 1] = rowBase;
    src[off + 2] = (((x + y) * 255) / (W + H)) | 0;
  }
}
let s = SEED >>> 0;
let ni = 0;
while (ni + 4 <= N) {
  s = (s ^ ((s << 13) >>> 0)) >>> 0;
  s = (s ^ (s >>> 17)) >>> 0;
  s = (s ^ ((s << 5) >>> 0)) >>> 0;
  src[ni] = (src[ni] + (s & 0xff)) & 0xff;
  src[ni + 1] = (src[ni + 1] + ((s >>> 8) & 0xff)) & 0xff;
  src[ni + 2] = (src[ni + 2] + ((s >>> 16) & 0xff)) & 0xff;
  src[ni + 3] = (src[ni + 3] + ((s >>> 24) & 0xff)) & 0xff;
  ni += 4;
}
while (ni < N) {
  s = (s ^ ((s << 13) >>> 0)) >>> 0;
  s = (s ^ (s >>> 17)) >>> 0;
  s = (s ^ ((s << 5) >>> 0)) >>> 0;
  src[ni] = (src[ni] + (s & 0xff)) & 0xff;
  ni += 1;
}

const dst = Buffer.alloc(N);

// ---- blur ----
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    let rAcc = 0;
    let gAcc = 0;
    let bAcc = 0;
    for (let ky = -2; ky <= 2; ky++) {
      const yy = clampIdx(y + ky, 0, H - 1);
      const row = yy * W;
      const krow = KERNEL[ky + 2];
      for (let kx = -2; kx <= 2; kx++) {
        const xx = clampIdx(x + kx, 0, W - 1);
        const idx = (row + xx) * 3;
        const k = krow[kx + 2];
        rAcc += src[idx] * k;
        gAcc += src[idx + 1] * k;
        bAcc += src[idx + 2] * k;
      }
    }
    const outIdx = (y * W + x) * 3;
    dst[outIdx] = clampU8((rAcc / KSUM) | 0);
    dst[outIdx + 1] = clampU8((gAcc / KSUM) | 0);
    dst[outIdx + 2] = clampU8((bAcc / KSUM) | 0);
  }
}

// ---- FNV-1a 32-bit checksum ----
let h = 0x811c9dc5 | 0;
for (let i = 0; i < dst.length; i++) {
  h = (h ^ dst[i]) | 0;
  h = imul32(h, 0x01000193);
}
const hash = h >>> 0;
const hex = hash.toString(16).padStart(8, '0');
console.log(`checksum=${hex} dims=${W}x${H} bytes=${dst.length}`);
