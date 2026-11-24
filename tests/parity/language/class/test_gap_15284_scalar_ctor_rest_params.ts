// A non-escaping `const x = new C(...)` whose constructor (own or inherited)
// takes a rest or `arguments` parameter receives the argument list packed
// into an array, as any other `new` does.

class P {
  n: number;
  a: number[];
  constructor(first: number, ...rest: number[]) {
    this.n = rest.length;
    this.a = rest;
  }
}
const p = new P(1, 2, 3, 4);
console.log("ctor rest", p.n, p.a.join(","));

const none = new P(1);
console.log("empty rest", none.n, none.a.length);

class OnlyRest {
  n: any;
  a: any;
  constructor(...rest: number[]) {
    this.n = rest.length;
    this.a = rest;
  }
}
const o = new OnlyRest(5, 6, 7);
console.log("only rest", o.n, o.a);

class Args {
  n: any;
  f: any;
  constructor(a: number) {
    this.n = arguments.length;
    this.f = a;
  }
}
// @ts-expect-error: extra arguments reach `arguments`
const g = new Args(1, 2, 3);
const g2 = new Args(1);
console.log("arguments", g.n, g.f, g2.n, g2.f);

class Derived extends P {}
const d = new Derived(1, 2, 3);
console.log("inherited rest", d.n, d.a.join(","));
