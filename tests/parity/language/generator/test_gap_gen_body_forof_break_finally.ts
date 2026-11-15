// A for-of inside a generator body over a generator: an early exit (break)
// calls the inner generator's return(), which runs its finally blocks.
const log: string[] = [];
function* inner(n: number): Generator<number> {
  try { for (let i = 0; i < n; i++) yield i; } finally { log.push("inner-finally"); }
}
function* mid(it: Generator<number>): Generator<number> {
  try { for (const v of it) { if (v > 2) break; yield v * 10; } log.push("mid-after-loop"); }
  finally { log.push("mid-finally"); }
}
const it = inner(10);
const out: number[] = [];
for (const v of mid(it)) out.push(v);
console.log(out.join(","), log.join(","));
console.log("inner after", JSON.stringify(it.next()));
// return() of the outer while it is suspended inside the inner loop.
log.length = 0;
const it2 = inner(10);
const m = mid(it2);
console.log(JSON.stringify(m.next()), JSON.stringify(m.next()));
console.log(JSON.stringify(m.return(99)), log.join(","), JSON.stringify(it2.next()));
// A labelled continue out of the inner loop body.
function* pairs(a: Generator<number>): Generator<string> {
  outer: for (let k = 0; k < 2; k++) { for (const v of a) { if (v === 1) continue outer; yield k + ":" + v; } }
}
console.log([...pairs(inner(4))].join(" "), log.join(","));
