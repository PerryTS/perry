// ECMA-262 27.5.3.4 GeneratorResumeAbrupt + 14.7.5.7 ForIn/OfBodyEvaluation: return() at a yield inside a
// for-of body is a return completion, so the loop closes its iterator (calls return()) before the generator completes.
const n = Number(process.argv[2] ?? "5");
const log: string[] = [];
function* inner(k: number): Generator<number> {
  try { for (let i = 0; i < k; i++) yield i + 1; } finally { log.push("inner-finally"); }
}
function* plain(it: Generator<number>): Generator<number> { for (const v of it) yield v; }
// 1. inner is a generator with a finally
const a = inner(n); const m = plain(a); m.next();
console.log(JSON.stringify(m.return(9)), log.join(","), JSON.stringify(a.next()), JSON.stringify(m.next()));
// 2. try/finally and try/catch around the loop in the outer body
function* wrapped(it: Generator<number>): Generator<number> {
  try { for (const v of it) yield v; log.push("after-loop"); } catch (e) { log.push("catch"); } finally { log.push("outer-finally"); }
}
log.length = 0;
const b = inner(n); const w = wrapped(b); w.next(); w.next();
console.log(JSON.stringify(w.return(7)), log.join(","), JSON.stringify(b.next()));
// 3. user iterator with a return method that logs; return() value is passed through
class UI implements Iterable<number> {
  i = 0;
  [Symbol.iterator]() { return this; }
  next() { return { value: this.i++, done: this.i > n }; }
  return(v?: unknown) { log.push("ui-return"); return { value: v, done: true }; }
}
function* overUI(u: UI): Generator<number> { for (const v of u) yield v; }
log.length = 0;
const u = new UI(); const g = overUI(u);
console.log(JSON.stringify(g.next()), JSON.stringify(g.return("r")), log.join(","), JSON.stringify(g.next()));
// 4. return() on a generator that has not started closes nothing
log.length = 0;
const u2 = new UI(); const g2 = overUI(u2);
console.log(JSON.stringify(g2.return(1)), log.join(","), u2.i);
