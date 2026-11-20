// A method call on a receiver whose CLASS codegen proved may take the class's
// typed-receiver clone, which reads fields as raw doubles. An alias the
// object escaped to can store a non-Number into a field (moving the object
// off its birth shape), so the clone must run only while the object's
// (class id, ShapeId) pair is still the class's own.

class Holder {
  a: number = 1;
  b: number = 2;
  left(): number {
    return this.a + this.b;
  }
}

class H2 {
  a: number;
  b: number;
  constructor(a: number, b: number) {
    this.a = a;
    this.b = b;
  }
  left(): number {
    return this.a + this.b;
  }
  sum3(): number {
    return this.left() + this.a;
  }
}

function f(x: any) {
  x.a = "u";
}
function g(x: any) {
  x.b = { valueOf() { return 40; } };
}

const h4 = new Holder();
f(h4);
const y: any = h4;
console.log(typeof y.a, y.a, y.b, h4.a, h4.left());

const k = new H2(1, 2);
f(k);
console.log(k.left(), k.sum3());

const m = new H2(3, 4);
g(m);
console.log(m.left(), m.sum3());

let acc = 0;
const ks: H2[] = [];
for (let i = 0; i < 2000; i++) {
  const p = new H2(i, 0.5);
  if (i % 500 === 7) f(p);
  ks.push(p);
}
for (const p of ks) {
  const v: any = p.left();
  acc += typeof v === "number" ? v : v.length;
}
console.log(acc, String(ks[7].left()), ks[8].left());
