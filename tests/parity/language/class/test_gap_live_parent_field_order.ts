// Fields of a class whose parent is only known at runtime are defined after
// super() returns, in source order, after whatever the parent created.
import { EventEmitter } from "events";

class P {
  p1 = 1;
  seen: string;
  constructor() {
    const self: any = this;
    this.seen = [
      String(self.f1),
      String("f1" in self),
      String(Object.prototype.hasOwnProperty.call(self, "f1")),
      Object.keys(self).join("|"),
    ].join(",");
    self.f2 = "parent";
    self.late = 9;
  }
}
function mk(): typeof P {
  return P;
}
class C extends mk() {
  f1 = 1;
  f2 = 2;
  f3 = this.f1 + 2;
  constructor() {
    super();
  }
}
const c: any = new C();
console.log(Object.keys(c).join(","));
console.log(c.seen);
console.log(JSON.stringify(c));
console.log(c.f2, c.f3, Object.getOwnPropertyNames(c).join(","));

class D extends (mk() as any) {
  g1 = "g1";
  g2 = "g2";
}
const d: any = new D();
console.log(Object.keys(d).join(","), d.seen);

class Bus extends EventEmitter {
  a = 1;
  b = 2;
  name: string;
  constructor(n: string) {
    super();
    this.name = n;
  }
}
const b: any = new Bus("bus");
console.log(Object.keys(b).join(","));
console.log(b.a, b.b, b.name, b.listenerCount("x"));

class E1 extends Bus {
  e = 5;
  a = 10;
}
const e: any = new E1("e1");
console.log(Object.keys(e).join(","), e.a, e.e);

for (let i = 0; i < 3; i++) {
  const x: any = new C();
  console.log(i, Object.keys(x).length, x.f3);
}
