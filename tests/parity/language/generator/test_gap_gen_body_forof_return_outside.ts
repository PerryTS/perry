// return() suspended at a yield OUTSIDE any for-of closes nothing: the loop has ended (exhausted or broken) or not begun.
const n = Number(process.argv[2] ?? "3");
const log: string[] = [];
class UI implements Iterable<number> {
  i = 0;
  [Symbol.iterator]() { return this; }
  next() { return { value: this.i++, done: this.i > n }; }
  return(v?: unknown) { log.push("ui-return"); return { value: v, done: true }; }
}
function* g(u: UI): Generator<string> {
  yield "before";
  for (const v of u) { if (v >= n - 1) break; yield "v" + v; }
  yield "after";
  for (const v of new UI()) yield "second" + v;
}
// before the loop
log.length = 0;
let x = g(new UI()); x.next();
console.log(JSON.stringify(x.return(1)), log.join(","));
// after the loop was left by break: closed once by the break, not again by return()
log.length = 0;
const u = new UI(); x = g(u);
let r = x.next(); while (r.value !== "after") r = x.next();
console.log(log.join(","), JSON.stringify(x.return(2)), log.join(","));
// after the loop was exhausted: no close at all
function* h(u: UI): Generator<string> { for (const v of u) yield "v" + v; yield "tail"; }
log.length = 0;
const u3 = new UI(); const y = h(u3); let s = y.next(); while (s.value !== "tail") s = y.next();
console.log(JSON.stringify(y.return(3)), log.join(","));
