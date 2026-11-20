// S0 `restraw` (I32 at rest, PR B): an at-rest element store writes the
// value's double word instead of its I32 word. The at-rest load witness must
// report it at the next read.
const a = new Array<number>(8).fill(0);
for (let i = 0; i < 40; i++) {
  a[i & 7] = ((a[(i + 1) & 7] | 0) + i) & 1023;
}
let s = 0;
for (let i = 0; i < 8; i++) s = (s + (a[i] | 0)) | 0;
console.log(s, a.length);
