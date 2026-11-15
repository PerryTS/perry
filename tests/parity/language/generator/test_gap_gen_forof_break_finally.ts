// for-of over a generator: break, return and throw out of the loop run the
// generator's finally blocks (IteratorClose -> return()); exhaustion does not close.
const log: string[] = [];
function* g(tag: string): Generator<number> {
  try { try { yield 1; yield 2; yield 3; } finally { log.push(tag + ":inner"); } } finally { log.push(tag + ":outer"); }
}
for (const v of g("brk")) { log.push("brk" + v); if (v === 2) break; }
function early(): number { for (const v of g("ret")) { if (v === 1) return v * 10; } return -1; }
log.push("early=" + early());
try { for (const v of g("thr")) { if (v === 3) throw new Error("x" + v); } } catch (e) { log.push("caught " + (e as Error).message); }
for (const v of g("all")) log.push("all" + v);
outer: for (const a of g("o1")) { for (const b of g("o2")) { if (b === 2) continue outer; log.push("ab" + a + b); } }
console.log(log.join("\n"));
