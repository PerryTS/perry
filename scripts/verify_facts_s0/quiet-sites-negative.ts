// Quiet sites: `walk` keeps no length over its loop, because the store
// `q["w"] = 1` may run program code: `q` holds a `P`, whose `w` is a setter
// that pops the Array the loop walks. The facts call that store's site not
// quiet (its receiver is no plain Array), so the loop's walker takes it for
// a call and reads `xs.length` at every step. Under
// PERRY_FACTS_SABOTAGE=quiet every site is claimed quiet: the walker keeps
// the length 6 and the loop reads three elements past the end. The
// check-mode witness in the loop's condition block must report the changed
// length ("quiet sites").
class P {
  xs: number[] = [1, 2, 3, 4, 5, 6];
  set w(v: number) {
    this.xs.pop();
  }
}
function walk(): string {
  const p = new P();
  const xs = p.xs;
  const q: any = p;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    q["w"] = 1;
  }
  return s;
}
console.log(walk());
