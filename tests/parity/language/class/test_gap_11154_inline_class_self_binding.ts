function mk(v: number) {
  const C = class C {
    static tag = v;
    static { (C.prototype as any).m = v; }
  };
  return C;
}
const A = mk(1), B = mk(2);
console.log(Object.keys(A.prototype), (new A() as any).m, (new B() as any).m);
console.log('statics', A.tag, B.tag);
console.log('distinct', A !== B, A.prototype !== B.prototype);
(A.prototype as any).m = 10;
console.log('isolated', (new A() as any).m, (new B() as any).m);
// Keep a call through a function value as a control for the factory body.
const indirect: any = mk;
const C = indirect(3);
console.log('indirect', C.tag, (new C() as any).m);
