// Generator objects inherit from %IteratorPrototype% (Iterator.prototype):
// the prototype chain, own keys, helper resolution through it, and objects
// that merely have a `next` method staying outside it.

function* g(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}

const GeneratorPrototype = Object.getPrototypeOf(g.prototype);
console.log("chain:", Object.getPrototypeOf(GeneratorPrototype) === Iterator.prototype);
console.log(
  "chain via function*:",
  Object.getPrototypeOf(Object.getPrototypeOf(function* () {}.prototype)) === Iterator.prototype,
);
console.log("instanceof:", g() instanceof Iterator);
console.log("own keys:", Reflect.ownKeys(GeneratorPrototype).map(String).join(","));
console.log("own @@iterator:", Object.prototype.hasOwnProperty.call(GeneratorPrototype, Symbol.iterator));
console.log("inherited @@iterator:", GeneratorPrototype[Symbol.iterator] === Iterator.prototype[Symbol.iterator]);
const it = g();
console.log("@@iterator returns this:", it[Symbol.iterator]() === it);
console.log("map is inherited:", g().map === Iterator.prototype.map);
console.log("toStringTag:", Object.prototype.toString.call(g()));

// Helpers resolve through the chain.
console.log("map:", g().map((x) => x * 10).toArray().join(","));
console.log("chain ops:", g().filter((x) => x !== 2).map((x) => x + 1).take(5).toArray().join(","));
console.log("for-of:", [...g()].join(","));

// Inline object-literal arguments.
console.log("take valueOf:", g().take({ valueOf() { return 2; } } as any).toArray().join(","));
console.log("drop valueOf:", g().drop({ valueOf() { return 1; } } as any).toArray().join(","));
const acc = g().reduce((a: any, b) => { a.sum += b; return a; }, { sum: 0 } as any);
console.log("reduce literal init:", acc.sum);
console.log("reduce add:", g().reduce((a, b) => a + b, 100));

// An override on Iterator.prototype is observed by generators.
const originalMap = Iterator.prototype.map;
(Iterator.prototype as any).map = function () { return "overridden"; };
console.log("override:", (g() as any).map((x: number) => x));
(Iterator.prototype as any).map = originalMap;
console.log("restored:", g().map((x) => x + 1).toArray().join(","));

// An own property on the generator's prototype shadows the inherited helper.
function* h(): Generator<number> { yield 5; }
(h.prototype as any).map = function () { return "own on h.prototype"; };
console.log("shadow:", (h() as any).map(() => 0));

// An object with `next` that does not inherit from Iterator.prototype has no helpers.
const plain = { next() { return { value: 1, done: true }; } };
try {
  (plain as any).map((x: number) => x);
  console.log("plain map: no throw");
} catch (e: any) {
  console.log("plain map:", e.constructor.name);
}
console.log("plain instanceof:", plain instanceof Iterator);

// A class generator method.
class Box {
  items = [4, 5, 6];
  *values(): Generator<number> {
    yield* this.items;
  }
}
const bv = new Box().values();
console.log("class gen chain:", Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(bv))) === Iterator.prototype);
console.log("class gen instanceof:", bv instanceof Iterator);
console.log("class gen map:", bv.map((x) => x * 2).toArray().join(","));

// Async generators keep their own chain.
async function* ag(): AsyncGenerator<number> { yield 1; }
const AsyncGeneratorPrototype = Object.getPrototypeOf(ag.prototype);
const AsyncIteratorPrototype = Object.getPrototypeOf(AsyncGeneratorPrototype);
console.log("async own keys:", Reflect.ownKeys(AsyncGeneratorPrototype).map(String).join(","));
console.log("async parent is Iterator.prototype:", AsyncIteratorPrototype === Iterator.prototype);
console.log("async grandparent is Object.prototype:", Object.getPrototypeOf(AsyncIteratorPrototype) === Object.prototype);
console.log("async instanceof Iterator:", ag() instanceof Iterator);
(async () => {
  for await (const v of ag()) console.log("for await:", v);
})();
