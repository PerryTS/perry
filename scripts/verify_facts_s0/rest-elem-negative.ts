// S0 `restelem` (I32 at rest, PR B): a builtin method's use of an Array is
// not recorded, so an Array the native `fill` writes with plain doubles is
// admitted at rest. The at-rest load witness must report the word.
const a = new Array<number>(8).fill(0);
function step(k: number): number {
  a.fill(k & 7);
  a[k & 7] = (k * 3) & 255;
  return a[(k + 1) & 7] | 0;
}
let s = 0;
for (let i = 0; i < 50; i++) s = (s + step(i)) | 0;
console.log(s, a.length);
