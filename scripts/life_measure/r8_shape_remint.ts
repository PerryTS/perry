// LIFE I3d fixture: each iteration of run builds an object property by property (shape
// transitions), keeps it alive across natural minor collections (promotion), then drops it.
// Only engine key lists (shape metadata, never a JS value) can outlive the iteration: the
// shape table re-mints them after a full pruned the shape and its carrier gate holds them.
// Clean per Node semantics at every density.
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const o: any = {};
    o.def = i;
    o.constr = "c";
    o.traits = "t";
    const tmp: { v: number; s: string }[] = [];
    for (let j = 0; j < 40000; j++) tmp.push({ v: j, s: "k" + j });
    t += o.def + tmp.length;
  }
  return t;
}
console.log(run(40));
