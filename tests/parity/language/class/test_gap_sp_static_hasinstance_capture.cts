function make(want: number) {
  return class C {
    static [Symbol.hasInstance](x: any) { return x.n === want; }
  };
}
const C = make(1);
console.log('direct', C[Symbol.hasInstance]({ n: 1 }));
console.log('instanceof', ({ n: 1 }) instanceof C, ({ n: 2 }) instanceof C);
const D = make(2);
console.log('second', D[Symbol.hasInstance]({ n: 2 }), ({ n: 2 }) instanceof D, ({ n: 1 }) instanceof D);
console.log('first-again', C[Symbol.hasInstance]({ n: 1 }), ({ n: 2 }) instanceof C);

// Static generators share the symbol callable installer and retain their
// evaluation after the method-entry frame has returned.
const generatorKey = Symbol('captured-generator');
function makeGenerator(label) { return class { static *[generatorKey]() { yield label; } }; }
const G = makeGenerator('first'); const H = makeGenerator('second');
console.log('generator', G[generatorKey]().next().value, H[generatorKey]().next().value, G[generatorKey]().next().value);
