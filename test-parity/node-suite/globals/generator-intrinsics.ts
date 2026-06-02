// #3664: generator / async-generator intrinsic objects + prototype chains.
// Exercises the function-side intrinsics, the %Generator(.prototype)% chain,
// the lazy per-function `.prototype`, and the brand-checked prototype methods
// (sync throws, async rejects).

function* g() {
  yield 1;
}
async function* ag() {
  yield 1;
}

// --- function-side intrinsics ---
const GenProto = Object.getPrototypeOf(g); // %Generator%
const GeneratorFunction = GenProto.constructor; // %GeneratorFunction%
console.log("g.constructor.name:", g.constructor.name);
console.log("GeneratorFunction.name:", GeneratorFunction.name);
console.log("GeneratorFunction.length:", GeneratorFunction.length);
console.log("GenProto.constructor === GeneratorFunction:", GenProto.constructor === GeneratorFunction);
console.log("GeneratorFunction.prototype === GenProto:", GeneratorFunction.prototype === GenProto);
console.log("GenProto[toStringTag]:", GenProto[Symbol.toStringTag]);

// --- %Generator.prototype% ---
const GeneratorPrototype = GenProto.prototype; // %Generator.prototype%
console.log("GeneratorPrototype.constructor === GenProto:", GeneratorPrototype.constructor === GenProto);
console.log("GeneratorPrototype[toStringTag]:", GeneratorPrototype[Symbol.toStringTag]);
console.log("typeof GeneratorPrototype.next:", typeof GeneratorPrototype.next);

// --- per-function g.prototype (lazy) ---
console.log("typeof g.prototype:", typeof g.prototype);
console.log("g.prototype identity stable:", g.prototype === g.prototype);
console.log("getPrototypeOf(g.prototype) === GeneratorPrototype:", Object.getPrototypeOf(g.prototype) === GeneratorPrototype);

// --- async tower is distinct ---
const AGenProto = Object.getPrototypeOf(ag);
const AsyncGeneratorFunction = AGenProto.constructor;
console.log("AsyncGeneratorFunction.name:", AsyncGeneratorFunction.name);
console.log("AGenProto[toStringTag]:", AGenProto[Symbol.toStringTag]);
console.log("AGenProto.prototype[toStringTag]:", AGenProto.prototype[Symbol.toStringTag]);
console.log("sync !== async ctor:", GeneratorFunction !== AsyncGeneratorFunction);

// --- brand-check: sync prototype method throws on bad receiver ---
for (const bad of [undefined, null, {}, function () {}, g.prototype]) {
  try {
    GeneratorPrototype.next.call(bad as any);
    console.log("sync brand NO THROW");
  } catch (e) {
    console.log("sync brand throws:", (e as any) instanceof TypeError);
  }
}

// --- delegation: prototype method drives a real instance ---
const it = g();
console.log("delegated next:", JSON.stringify(GeneratorPrototype.next.call(it)));

// --- brand-check: async prototype method rejects on bad receiver ---
const AGP = AGenProto.prototype;
AGP.next.call(undefined as any).then(
  () => console.log("async brand NO REJECT"),
  (e: any) => console.log("async brand rejects:", e instanceof TypeError),
);
