// An inner generator partly consumed by manual next() calls, then handed to a
// generator-body for-of, then resumed manually again after an early exit.
function* count(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; }
function* mid(it: Generator<number>, stop: number): Generator<number> {
  for (const v of it) { if (v === stop) return; yield v; }
}
const it = count(8);
console.log(JSON.stringify(it.next()), JSON.stringify(it.next()));
const m = mid(it, 99);
console.log(JSON.stringify(m.next()), JSON.stringify(it.next()), JSON.stringify(m.next()));
console.log([...m].join(","), JSON.stringify(it.next()));
// A completed inner generator: the mid loop runs zero times.
const done = count(1); done.next(); done.next();
console.log("zero", [...mid(done, 99)].length);
// An early return closes the inner one: later next() is done.
const it3 = count(8);
console.log([...mid(it3, 3)].join(","), JSON.stringify(it3.next()));
// Values sent to the outer next() reach the outer, not the inner.
function* echo(it: Generator<number>): Generator<number, void, any> { for (const v of it) { const s = yield v; if (s !== undefined) console.log("sent", s); } }
const e = echo(count(3));
console.log(JSON.stringify(e.next()), JSON.stringify(e.next("a")), JSON.stringify(e.next("b")), JSON.stringify(e.next("c")));
