// A private-method site guard must read the RECEIVER's ShapeId, never a
// word one MiB below it: a D receiver must always be rejected.
class C {
  a = 1; b = 2; c = 3; d = 4; e = 5;
  #m(): number { return this.a; }
  static call(o: any): number {
    try { return o.#m(); } catch { return -1; }
  }
}
class D { a = 1; b = 2; c = 3; d = 4; e = 5; }
const all: any[] = [];
let wrong = 0, ok = 0;
for (let i = 0; i < 400000; i++) {
  const o = i % 997 === 0 ? new D() : new C();
  all.push(o);
  const r = C.call(o);
  if (o instanceof D) { if (r !== -1) wrong++; } else if (r === 1) ok++;
}
console.log("wrong", wrong, "ok", ok);
