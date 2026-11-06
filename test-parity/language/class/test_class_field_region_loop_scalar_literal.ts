// A loop region lowers its body twice (shape-guarded F-body, generic G-body).
// A non-escaping literal in that body is scalar-replaced; the second lowering
// used to heap-allocate it while its field reads kept loading the first
// copy's slots, so `u.c` read `undefined` (NaN after `+=`).
class B {
  x = 15;
  y = 0;
  twoOnly(n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: this.x, d: this.y }; s += u.c; } return s; }
  sameField(n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: this.x, d: this.x }; s += u.c; } return s; }
  both(n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: this.x, d: this.y }; s += u.c + u.d; } return s; }
  arr(n: number): number { let s = 0; for (let i = 0; i < n; i++) { const a = [this.x, this.y]; s += a[0]! + a[1]!; } return s; }
  typed(n: number): string { let r = ""; for (let i = 0; i < n; i++) { const u = { c: this.x, d: this.y }; r = typeof u.c + ":" + String(u.c); } return r; }
}
function free(o: B, n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: o.x, d: o.y }; s += u.c; } return s; }
function single(o: B, n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: o.x }; s += u.c; } return s; }
function swapped(o: B, n: number): number { let s = 0; for (let i = 0; i < n; i++) { const u = { c: o.y, d: o.x }; s += u.d; } return s; }
const b = new B();
console.log(b.twoOnly(10), b.sameField(10), b.both(10), b.arr(10), b.typed(3));
console.log(free(b, 10), single(b, 10), swapped(b, 10));
console.log(free({ x: 15, y: 0 } as B, 10));
