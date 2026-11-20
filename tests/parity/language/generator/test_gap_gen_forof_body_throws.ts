// A throwing generator body ends the for-of with that error, closes nothing,
// and leaves the generator completed: a later next() is { undefined, true }.
let finals = 0;
function* g(n: number): Generator<number> {
  try { for (let i = 0; i < n; i++) yield i; throw new RangeError("end " + n); } finally { finals++; }
}
const it = g(3);
const seen: number[] = [];
try { for (const v of it) seen.push(v); } catch (e) { console.log("caught", (e as Error).name, (e as Error).message, seen.join(",")); }
console.log("finals", finals);
console.log("after", JSON.stringify(it.next()), JSON.stringify(it.next(7)));
for (const v of it) console.log("never", v);
console.log("loop over completed ok");
function* bad(): Generator<number> { yield 1; (null as any).x; }
try { for (const v of bad()) console.log("bad", v); } catch (e) { console.log("typeerror", e instanceof TypeError); }
