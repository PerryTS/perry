// Nested for-of: return() closes both, innermost first. throw() at the same point closes once and rethrows.
// An inner iterator whose return() throws: the throw wins over the return completion (7.4.11 step 5-6 with a return completion).
const n = Number(process.argv[2] ?? "4");
const log: string[] = [];
function* src(name: string): Generator<number> {
  try { for (let i = 0; i < n; i++) yield i; } finally { log.push(name + "-finally"); }
}
function* nested(a: Generator<number>, b: Generator<number>): Generator<string> {
  for (const x of a) for (const y of b) yield x + ":" + y;
}
let a = src("A"), b = src("B"); let g = nested(a, b); g.next();
console.log(JSON.stringify(g.return("R")), log.join(","), JSON.stringify(a.next()), JSON.stringify(b.next()));
// throw() at the yield
log.length = 0;
a = src("A"); b = src("B"); g = nested(a, b); g.next();
try { g.throw(new RangeError("T")); } catch (e) { console.log("caught", (e as Error).name, log.join(",")); }
console.log(JSON.stringify(a.next()), JSON.stringify(b.next()));
// throw() caught by the outer body: the loop closed first, then the catch runs
function* catching(it: Generator<number>): Generator<string> {
  try { for (const v of it) yield "v" + v; } catch (e) { yield "caught " + (e as Error).name; }
  yield "end";
}
log.length = 0;
const c = catching(src("C")); c.next();
console.log(JSON.stringify(c.throw(new TypeError("x"))), log.join(","), JSON.stringify(c.next()), JSON.stringify(c.next()));
// the inner return() throws during return(): the throw propagates
function mk() {
  let i = 0;
  return { [Symbol.iterator]() { return this; }, next() { return { value: i++, done: false }; },
           return() { log.push("bad-return"); throw new EvalError("from-return"); } };
}
function* overBad(): Generator<number> { for (const v of mk()) yield v; }
log.length = 0;
const d = overBad(); d.next();
try { console.log(JSON.stringify(d.return(5))); } catch (e) { console.log("caught", (e as Error).name, log.join(",")); }
console.log(JSON.stringify(d.next()));
// a yielding finally in the outer body: the loop is closed before the finally runs
function* withYieldFinally(it: Generator<number>): Generator<string> {
  try { for (const v of it) yield "v" + v; } finally { log.push("f-start"); yield "in-finally"; log.push("f-end"); }
}
log.length = 0;
const f = withYieldFinally(src("F")); f.next();
console.log(JSON.stringify(f.return("Z")), log.join(","), JSON.stringify(f.next()), log.join(","));
