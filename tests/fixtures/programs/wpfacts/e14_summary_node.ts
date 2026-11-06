// E14 (decision 150): interpreted code is one summary node, not a kill.
// What reaches it escapes; a class it never sees keeps its facts, and the
// program's proofs are guarded by the runtime word instead.
// WPFACTS: guard on
// WPFACTS: fact F3 prefix_stable Given = 0
// WPFACTS: kind Given.y has str
// WPFACTS: fact F3 prefix_stable Kept = 1
// WPFACTS: fact F1 readonly Kept.x = 1
// WPFACTS: kind Kept.x is num
class Given {
  y: number = 1;
}
class Kept {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const body = ["o.y = 'from interpreted code'; return o;"].join("");
const f = new Function("o", body);
const g = f(new Given());
function rd(k: Kept): number {
  return k.x;
}
console.log(rd(new Kept(4)), g.y);
