// A generator-body for-of whose source is a generator on one branch and
// undefined (or null) on the other: GetIterator throws a TypeError at loop
// entry, before any step; the generator branch steps normally.
function* count(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; }
const flag = process.argv.length;
function pick(k: number): any { return k === flag ? undefined : k === flag + 1 ? null : count(3); }
function* wrap(src: any): Generator<number> { yield -1; for (const v of src) yield v; yield 99; }
for (const k of [flag, flag + 1, flag + 2]) {
  const out: number[] = [];
  try { for (const v of wrap(pick(k))) out.push(v); console.log("ok", out.join(",")); }
  catch (e) { console.log("throw", e instanceof TypeError, out.join(",")); }
}
const w = wrap(pick(flag));
console.log(JSON.stringify(w.next()));
try { w.next(); } catch (e) { console.log("entry", e instanceof TypeError); }
console.log(JSON.stringify(w.next()));
