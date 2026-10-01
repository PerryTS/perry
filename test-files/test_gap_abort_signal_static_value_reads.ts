// AbortSignal statics read as VALUES (not just called) are real own data
// properties of the constructor: a kept-receiver read must find them.
const a = AbortSignal.abort, t = AbortSignal.timeout, n = AbortSignal.any;
console.log(typeof a, typeof t, typeof n);
console.log(a.name, a.length, t.name, t.length, n.name, n.length);
console.log(Object.getOwnPropertyNames(AbortSignal).sort().join());
const s = a("why");
console.log(s.aborted, s.reason);
const c = new AbortController();
const m = n([c.signal]);
console.log(m.aborted);
c.abort("r");
console.log(m.aborted, m.reason);
try {
  n(5 as any);
} catch (e: any) {
  console.log(e.code, e.name);
}
const d = Object.getOwnPropertyDescriptor(AbortSignal, "abort")!;
console.log(d.writable, d.enumerable, d.configurable);
