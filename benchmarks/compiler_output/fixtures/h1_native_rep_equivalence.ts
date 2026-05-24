const SIZE = 2048;

const src = Buffer.alloc(SIZE);
const dst = Buffer.alloc(SIZE);
const n = Math.min(src.length, dst.length);

function idx(i: number): number {
  return i | 0;
}

direct_bounded:
for (let i = 0; i < n; i++) {
  dst[i] = (src[i] + 1) & 255;
}

local_cast:
for (let i = 0; i < n; i++) {
  const j = i | 0;
  dst[j] = (src[j] + 1) & 255;
}

helper_index:
for (let i = 0; i < n; i++) {
  dst[idx(i)] = (src[idx(i)] + 1) & 255;
}

function incInPlace(buf: Buffer): number {
  same_buffer:
  for (let i = 0; i < buf.length; i++) {
    buf[i] = (buf[i] + 1) & 255;
  }
  return 0;
}

const same = Buffer.alloc(SIZE);
let checksum = incInPlace(same);
checksum = (checksum + n) | 0;
console.log("h1_native_rep_equivalence:" + checksum);
