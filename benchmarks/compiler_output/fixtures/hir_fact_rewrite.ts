const SIZE = 65536;

const src = new Uint8Array(65537);
const dst = new Uint8Array(65537);

function bumpIndex(i: number): number {
  return (i + 1) | 0;
}

for (let i = 0; i < SIZE; i++) {
  src[i] = (i * 17 + 3) & 255;
}

for (let i = 0; i < SIZE; i++) {
  const j = bumpIndex(i);
  dst[j] = (src[i] + 1) & 255;
}

let checksum = 0;
for (let i = 1; i <= SIZE; i++) {
  checksum = (checksum + dst[i]) | 0;
}

console.log("hir_fact_rewrite:" + checksum);
