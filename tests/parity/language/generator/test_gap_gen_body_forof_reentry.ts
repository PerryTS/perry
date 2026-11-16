// Re-entrancy through a generator-body for-of: resuming a generator that is
// executing throws "already executing" (TypeError), and the inner one is
// left completed by its own throw path.
let holder: Generator<number> | null = null;
let self: Generator<number> | null = null;
function* inner(): Generator<number> {
  yield 1;
  try { holder!.next(); } catch (e) { console.log("mid busy", e instanceof TypeError); }
  try { self!.next(); } catch (e) { console.log("self busy", e instanceof TypeError); }
  yield 2;
}
function* mid(it: Generator<number>): Generator<number> { for (const v of it) yield v * 3; }
self = inner();
holder = mid(self);
const out: number[] = [];
for (const v of holder) out.push(v);
console.log(out.join(","), JSON.stringify(self.next()));
// The inner generator is shared by two mids, interleaved.
function* count(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; }
const shared = count(6);
const a = mid(shared), b = mid(shared);
console.log(JSON.stringify(a.next()), JSON.stringify(b.next()), JSON.stringify(a.next()), JSON.stringify(b.next()));
console.log([...a].join(","), "|", [...b].join(","));
