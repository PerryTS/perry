// VR-2 S0 arm `vrinline` (L4): an identity compare in a hot loop.
function mk(n: number) {
  return { a: n, b: n > 1 };
}
const u1 = mk(1);
const u2 = mk(2);
let x = u1;
let hits = 0;
for (let i = 0; i < 1000; i++) {
  if (x === u2) hits++;
  x = (i & 1) === 0 ? u2 : u1;
}
console.log(hits, x.a);
