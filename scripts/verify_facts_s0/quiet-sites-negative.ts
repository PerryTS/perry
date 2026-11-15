// Quiet sites: `walk` keeps no length over its loop, because the read `q.v`
// may run program code: `q` holds a `P`, whose `v` is a getter that pops the
// Array the loop walks. The facts call that read's site not quiet (its
// verdict is no own data slot), so the loop's walker takes it for a call and
// reads `xs.length` at every step. Under PERRY_FACTS_SABOTAGE=quiet every
// site is claimed quiet: the walker keeps the length 6 and the loop reads
// three elements past the end. The check-mode witness in the loop's
// condition block must report the changed length ("quiet sites").
class P {
  xs: number[] = [1, 2, 3, 4, 5, 6];
  get v(): number {
    this.xs.pop();
    return 1;
  }
}
function walk(): string {
  const p = new P();
  const xs = p.xs;
  const q: any = p;
  let s = "";
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    n += q.v;
  }
  return s + n;
}
console.log(walk());
