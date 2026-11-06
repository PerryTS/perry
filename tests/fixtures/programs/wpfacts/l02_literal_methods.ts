// Object literals with methods, getters and arrow-function fields, read
// through `this` and through bindings, next to literals of the same keys
// whose field later holds another kind. Output parity with node is the
// test; the facts may prove any subset of the sites.
function counter(start: number) {
  return {
    n: start,
    step: 2,
    inc() {
      this.n += this.step;
      return this.n;
    },
    get twice() {
      return this.n * 2;
    },
    show: (k: number) => "k=" + k,
  };
}
function drive(c: ReturnType<typeof counter>, times: number): number {
  let last = 0;
  for (let i = 0; i < times; i++) last = c.inc();
  return last + c.twice;
}
const c1 = counter(1);
const c2 = counter(10);
console.log(drive(c1, 5), drive(c2, 3), c1.show(c1.n), c2.n, c2.twice);
const plain = { n: 3, step: 1 };
const loose: any = { n: 3, step: 1 };
loose.step = "1";
function bump(o: any): any {
  return o.n + o.step;
}
console.log(bump(plain), bump(loose), bump({ n: 0.25, step: 0.5 }));
const withProto = { __proto__: { step: 7 }, n: 1 } as any;
console.log(bump(withProto), Object.keys(withProto).join(","));
