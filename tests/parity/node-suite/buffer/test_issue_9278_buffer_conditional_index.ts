const W = 64;
const H = 64;
const N = W * H;
const src = Buffer.alloc(N);
const ternaryDst = Buffer.alloc(N);
const ifDst = Buffer.alloc(N);

for (let i = 0; i < N; i++) {
  src[i] = (i * 7 + 13) & 0xff;
}

function writeTernary(y: number, x: number): void {
  const yy = y < 0 ? 0 : y > H - 1 ? H - 1 : y;
  ternaryDst[y * W + x] = src[yy * W + x] & 0xff;
}

function writeIf(y: number, x: number): void {
  let yy = y;
  if (yy < 0) {
    yy = 0;
  }
  if (yy > H - 1) {
    yy = H - 1;
  }
  ifDst[y * W + x] = src[yy * W + x] & 0xff;
}

for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    writeTernary(y, x);
    writeIf(y, x);
  }
}

let ternarySum = 0;
let ifSum = 0;
for (let i = 0; i < N; i++) {
  ternarySum += ternaryDst[i];
  ifSum += ifDst[i];
}

console.log("ternary=" + ternarySum);
console.log("if=" + ifSum);
