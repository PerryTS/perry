// Quiet sites over Strings: `walk` keeps no length over its loop, because
// the read `q.length` may run program code: `q` holds a `P`, whose `length`
// is a getter that pops the Array the loop walks. The String rule of the
// facts calls a `length` read quiet only when every receiver holds Strings
// alone, so this read is not quiet and the walker reads `xs.length` at
// every step. Under PERRY_FACTS_SABOTAGE=quiet-strings every `length` read
// is claimed a String's own: the walker keeps the length 6 and the loop
// reads three elements past the end. The check-mode witness in the loop's
// condition block must report the changed length ("quiet sites").
class P {
  xs: number[] = [1, 2, 3, 4, 5, 6];
  get length(): number {
    this.xs.pop();
    return 1;
  }
}
function walk(q: any): string {
  const p: P = q;
  const xs = p.xs;
  let s = "";
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    n += q.length;
  }
  return s + n;
}
console.log(walk(new P()));
