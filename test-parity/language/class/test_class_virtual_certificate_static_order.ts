class StaticOrderBase { a = 3; b = 4; step(x: number): number { return this.b + x; } }
class StaticOrderDerived extends StaticOrderBase { step(x: number): number { return this.b * x; } }
let staticGets = 0;
function staticChange(c: any, i: number): number {
  if (i === 20) {
    const p: any = StaticOrderBase.prototype;
    p.step = function(x: number) { return 90 + x; };
    delete c.a;
  }
  if (i === 24) {
    Object.defineProperty(c, 'step', { configurable: true, get() { staticGets++; return function(x: number) { return 110 + x; }; } });
  }
  if (i === 26) console.log('getter-before-argument', staticGets);
  return 2;
}
function staticOrderKernel(): number {
  const cs: StaticOrderBase[] = [new StaticOrderBase(), new StaticOrderDerived()];
  let sum = 0;
  for (let i = 0; i < 30; i++) sum += cs[i & 1].step(staticChange(cs[0], i));
  return sum;
}
console.log('static-order', staticOrderKernel(), staticGets);
