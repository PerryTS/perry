// Class-expression instances are born on their evaluation's copy of the class
// family layout: each evaluation keeps its own prototype and private brand,
// interleaved births stay apart, and an instance that gains or loses a key
// keeps answering from its evaluation.
function mk(k: number) {
  return class {
    x: number;
    y: number;
    #p: number;
    constructor(a: number) {
      this.x = a;
      this.y = a + k;
      this.#p = k;
    }
    sum(): number {
      return this.x + this.y + this.#p;
    }
    static has(o: any): boolean {
      return #p in o;
    }
  };
}
const A = mk(1);
const B = mk(2);
const as: any[] = [];
const bs: any[] = [];
for (let i = 0; i < 5; i++) {
  as.push(new A(i));
  bs.push(new B(i));
}
console.log(as.map((a) => a.sum()).join(","), bs.map((b) => b.sum()).join(","));
console.log(as[3] instanceof A, as[3] instanceof B, Object.getPrototypeOf(bs[4]) === B.prototype);
console.log(A.has(as[2]), A.has(bs[2]), B.has(bs[2]));
console.log(Object.keys(as[1]).join(","), JSON.stringify(bs[0]));
console.log(Object.getOwnPropertyNames(A).sort().join(","));
(A.prototype as any).tag = function (this: any) {
  return "A" + this.x;
};
console.log(as[4].tag(), typeof bs[4].tag, new A(9).tag());
const a5: any = new A(5);
a5.z = 9;
delete a5.x;
console.log(Object.keys(a5).join(","), a5.y, a5.z, A.has(a5));
let t = 0;
for (let e = 0; e < 300; e++) {
  const C = mk(e);
  for (let i = 0; i < 20; i++) t += new C(i).sum();
}
console.log(t);
const C2 = mk(7);
const c1 = new C2(1);
const c2 = new C2(2);
console.log(c1.sum(), c2.sum(), Object.getPrototypeOf(c1) === Object.getPrototypeOf(c2), C2.has(c1), A.has(c1));
class D extends mk(3) {
  w = 4;
}
const d = new D(1);
console.log(d.sum(), d.w, d instanceof D, Object.keys(d).join(","));
// A family without private elements: its evaluations' instances are born
// straight on the evaluation's layout after the first.
function plain(k: number) {
  return class {
    x: number;
    y: number;
    constructor(a: number) {
      this.x = a;
      this.y = a + k;
    }
    sum(): number {
      return this.x + this.y;
    }
  };
}
const P = plain(10);
const Q = plain(20);
const ps: any[] = [];
for (let i = 0; i < 4; i++) {
  ps.push(new P(i));
  ps.push(new Q(i));
}
console.log(ps.map((p) => p.sum()).join(","));
console.log(ps[6] instanceof P, ps[6] instanceof Q, ps[7] instanceof Q, Object.getPrototypeOf(ps[0]) === P.prototype);
(Q.prototype as any).tag = () => "q";
console.log(typeof ps[0].tag, ps[1].tag(), new Q(1).tag(), Object.keys(new P(3)).join(","));
const p9: any = new P(9);
p9.extra = 1;
delete p9.y;
console.log(Object.keys(p9).join(","), p9.sum(), JSON.stringify(new Q(2)));
let u = 0;
for (let e = 0; e < 200; e++) {
  const C = plain(e);
  for (let i = 0; i < 30; i++) {
    const c = new C(i);
    u += c.sum() + c.x - c.y;
  }
}
console.log(u, Object.getOwnPropertyNames(P).sort().join(","));
