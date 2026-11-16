// A throwing inner generator body inside a generator-body for-of: the error
// leaves both loops, nothing is closed (no return() call), and the inner
// generator stays completed.
let finals = 0, returns = 0;
function* inner(n: number): Generator<number> {
  try { for (let i = 0; i < n; i++) yield i; throw new RangeError("end"); } finally { finals++; }
}
function* mid(it: Generator<number>): Generator<number> {
  try { for (const v of it) yield v + 100; } finally { returns++; }
}
const it = inner(3);
const seen: number[] = [];
try { for (const v of mid(it)) seen.push(v); } catch (e) { console.log("caught", (e as Error).name, seen.join(",")); }
console.log("finals", finals, "mid finally", returns);
console.log("after", JSON.stringify(it.next()), JSON.stringify(it.next(5)));
// The mid body throws while the inner is suspended: the inner is closed.
function* boom(it: Generator<number>): Generator<number> {
  for (const v of it) { if (v === 1) throw new TypeError("mid"); yield v; }
}
const it2 = inner(5);
try { for (const v of boom(it2)) console.log("v", v); } catch (e) { console.log("caught2", e instanceof TypeError); }
console.log("inner2 after", JSON.stringify(it2.next()), "finals", finals);
// A mid catch around the loop sees the inner's error and continues.
function* safe(it: Generator<number>): Generator<string> {
  try { for (const v of it) yield "v" + v; } catch (e) { yield "caught " + (e as Error).name; }
  yield "end";
}
console.log([...safe(inner(2))].join(","));
