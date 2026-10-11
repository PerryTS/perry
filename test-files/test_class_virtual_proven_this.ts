import { Base, Derived, Third, Fourth, Inherited, SuperStep } from './fixtures/class_virtual/defs.ts';
function sum(cs: Base[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += cs[i % cs.length].step(i);
  return s;
}
const pair: Base[] = [new Base(3), new Derived(5)];
console.log('pair', sum(pair, 20000), sum(pair, 20000));
const saved = Derived.prototype.step;
Derived.prototype.step = function(x: number): number { return this.scale * x + 23; };
console.log('prototype', sum(pair, 20));
Derived.prototype.step = saved;
console.log('restored', sum(pair, 20));
(pair[1] as any).step = function(x: number): number { return this.scale * x + 29; };
console.log('instance', sum(pair, 20));
delete (pair[1] as any).step;
console.log('instance-restored', sum(pair, 20));
const four: Base[] = [new Base(3), new Derived(5), new Third(7), new Fourth(9)];
console.log('four', sum(four, 20000));
console.log('inherited', sum([new Base(3), new Inherited(5)], 20));
console.log('super', sum([new Base(3), new SuperStep(5)], 20));
Derived.prototype.step = function(x: number): number { return this.scale * x + 31; };
console.log('super-patched', sum([new Base(3), new SuperStep(5)], 20));
Derived.prototype.step = saved;
let reads = 0;
Object.defineProperty(four[1], 'step', {
  configurable: true,
  get() { reads++; return function(x: number) { return this.scale * x + 37; }; }
});
console.log('method-getter', sum(four, 20), reads);
let fieldReads = 0;
Object.defineProperty(four[2], 'scale', { configurable: true, get() { fieldReads++; return 11; } });
console.log('field-getter', sum(four, 20), fieldReads);
// A changed receiver shape must fall back even when its class still matches.
const altered = new Derived(5);
delete (altered as any).scale;
Object.setPrototypeOf(altered, { scale: 13, step: saved });
console.log('changed-layout', sum([altered], 20));
// Declared numeric annotations cannot justify unboxed field arithmetic.
const nonnumeric = new Derived(5);
(nonnumeric as any).scale = '6';
console.log('nonnumeric', sum([nonnumeric], 20));
// Own closure methods on ordinary objects exercise the generic negative control.
const plain: any[] = [{ scale: 3, step(x: number) { return x * this.scale + 41; } },
 { scale: 5, step(x: number) { return x * this.scale + 43; } }];
console.log('plain', sum(plain, 20));
const ordering: Base[] = [new Derived(5)];
const order: string[] = [];
Object.defineProperty(ordering[0], 'step', {
 configurable: true,
 get() { order.push('get'); return function(x: number) { return x + 47; }; }
});
function argument(): number {
 order.push('arg');
 Object.defineProperty(ordering[0], 'step', { configurable: true, value: function(x: number) { return x + 999; } });
 return 3;
}
console.log('lookup-order', ordering[0].step(argument()), order.join(','));

// Argument evaluation is inert; its later ToInt32 can invoke user code.
function invoke(cs: Base[], x: any): number { return cs[0].step(x); }
let coercionReads = 0;
function coercingArgument(cs: Base[]): any {
  return { valueOf() {
    Object.defineProperty(cs[0], 'scale', {
      configurable: true, get() { coercionReads++; return 7; }
    });
    return 2;
  } };
}
const coercionBase: Base[] = [new Base(3)];
const coercionDerived: Base[] = [new Derived(5)];
console.log('coercion-getter', invoke(coercionBase, coercingArgument(coercionBase)),
  invoke(coercionDerived, coercingArgument(coercionDerived)), coercionReads);
