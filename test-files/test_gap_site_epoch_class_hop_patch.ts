// A read site warmed on a deep class chain must see every later change of an
// intermediate prototype: a key added, an accessor installed and swapped, the
// key deleted again, and the hop re-linked with setPrototypeOf.
class A {}
(A.prototype as any).k = "A";
class B extends A {}
class C extends B {}
class D extends C {}

function read(o: any): string {
  return String(o.k);
}

const d = new D();
const out: string[] = [];
for (let i = 0; i < 200; i++) out.push(read(d));
console.log("warm", out[0], out[199]);

(B.prototype as any).k = "B";
console.log("hop key added:", read(d), read(d));

let n = 0;
Object.defineProperty(C.prototype, "k", {
  get() {
    n++;
    return "getC" + n;
  },
  configurable: true,
});
console.log("hop accessor:", read(d), read(d));
Object.defineProperty(C.prototype, "k", {
  get() {
    return "swapped";
  },
  configurable: true,
});
console.log("accessor swapped:", read(d), read(d));

delete (C.prototype as any).k;
delete (B.prototype as any).k;
console.log("deleted:", read(d), read(d));

Object.setPrototypeOf(B.prototype, { k: "X" });
console.log("hop relinked:", read(d), read(d));

(A.prototype as any).k = "A2";
console.log("old holder changed:", read(d));
for (let i = 0; i < 50; i++) read(d);
console.log("rewarm:", read(d));
