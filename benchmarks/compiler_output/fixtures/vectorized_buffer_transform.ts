const SIZE = 65536;

const src = Buffer.alloc(SIZE);
const dst = Buffer.alloc(SIZE);

for (let i = 0; i < SIZE; i++) {
  src[i] = (i * 13 + 7) & 255;
}

for (let i = 0; i < SIZE; i++) {
  dst[i] = (src[i] + 1) & 255;
}

console.log("vectorized_buffer_transform:" + dst[SIZE - 1]);
