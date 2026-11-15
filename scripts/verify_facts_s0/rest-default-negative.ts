// S0 `restelem` over the default read (`a[k] ?? <int32>` as an i32,
// `rest_elems::try_lower_default_i32`): a builtin method's use of an Array
// is not recorded, so an Array the native `fill` writes with plain doubles
// is admitted at rest. The default read's slot witness must report the word.
const a = new Array<number>(8).fill(0);
function step(k: number): number {
  a.fill(k & 7);
  a[k & 7] = (k * 3) & 255;
  const v = a[(k % 11) - 1] ?? 5;
  return v === 0 ? 1 : v;
}
let s = 0;
for (let i = 0; i < 50; i++) s = (s + step(i)) | 0;
console.log(s, a.length);
