// LIFE I3 fixture: run's loop is clean except every 10th iteration, which
// stores its object into an older array.
const keep: { v: number }[] = [];
for (let i = 0; i < 20; i++) keep.push({ v: -1 });
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const o = { v: i };
    if (i % 10 === 0) keep[i / 10] = o;
    t += o.v;
  }
  return t;
}
console.log(run(200));
console.log(keep[19].v);
