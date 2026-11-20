// LIFE I3d fixture: as r6 (promotion inside each iteration), and each iteration of run stores
// exactly one small object into an older array. Dirty with exactly one survivor at every density.
const keep: { v: number }[] = [];
for (let i = 0; i < 40; i++) keep.push({ v: -1 });
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const tmp: { v: number; s: string }[] = [];
    for (let j = 0; j < 40000; j++) tmp.push({ v: j, s: "k" + j });
    keep[i] = { v: i };
    t += tmp[39999].v + tmp.length;
  }
  return t;
}
console.log(run(40));
console.log(keep[39].v);
