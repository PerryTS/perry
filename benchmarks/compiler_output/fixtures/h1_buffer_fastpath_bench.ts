const SIZE = 8192;
const ROUNDS = 256;

const src = Buffer.alloc(SIZE);
const dst = Buffer.alloc(SIZE);
const n = Math.min(src.length, dst.length);

seed:
for (let i = 0; i < n; i++) {
  src[i] = (i + 17) & 255;
  dst[i] = (i * 3) & 255;
}

transform:
for (let r = 0; r < ROUNDS; r++) {
  const twist = (r * 31) & 255;
  for (let i = 0; i < n; i++) {
    dst[i] = (src[i] + dst[i] + twist) & 255;
  }
}

let checksum = 0;
checksum_loop:
for (let i = 0; i < n; i += 257) {
  checksum = (checksum + dst[i]) | 0;
}

console.log("h1_buffer_fastpath_bench:" + checksum);
