// S0 `fillbirth` (the fill birth): an inline birth of `new Array(k).fill(v)`
// leaves its last slot a hole, so the born Array does not hold its fill
// value. The birth's witness must report it.
function make(): number[] {
  return new Array<number>(8).fill(3);
}
let s = 0;
for (let i = 0; i < 4; i++) {
  const a = make();
  a[i] = i;
  s = s + a[7] + a[0] + a.length;
}
console.log(s);
