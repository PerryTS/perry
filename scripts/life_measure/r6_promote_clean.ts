// LIFE I3d fixture: each iteration of run keeps a large temporary array alive while it allocates
// enough for natural minor collections (and promotion) inside the iteration; all of it is dropped
// at the iteration's end. Clean at every sampling density.
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const tmp: { v: number; s: string }[] = [];
    for (let j = 0; j < 40000; j++) tmp.push({ v: j, s: "k" + j });
    t += tmp[39999].v + tmp.length;
  }
  return t;
}
console.log(run(40));
