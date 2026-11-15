// KNOWN main bug (base and head): return() at a yield inside a generator-body for-of does not close the inner iterator.
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
log.length = 0;
// return() of the outer while it is suspended inside the inner loop.
log.length = 0;
const it2 = inner(10);
const m = mid(it2);
console.log(JSON.stringify(m.next()), JSON.stringify(m.next()));
console.log(JSON.stringify(m.return(99)), log.join(","), JSON.stringify(it2.next()));
