// yield* over a value whose [Symbol.iterator] is INHERITED from a prototype
// object literal, where the method reads `this` (mirrors effect's
// EffectPrototype[Symbol.iterator] = () => new SingleShotGen(new YieldWrap(this))).
// The generator must yield the receiver (via `this`), not the prototype object.

class SingleShotGen {
  called = false;
  self: any;
  constructor(self: any) { this.self = self; }
  next(a: any) {
    return this.called
      ? { value: a, done: true }
      : (this.called = true, { value: this.self, done: false });
  }
  [Symbol.iterator]() { return new SingleShotGen(this.self); }
}

// EffectPrototype analog: an object literal used as a prototype. Its
// [Symbol.iterator] wraps `this` so the receiver flows through yield*.
const EffectProto = {
  [Symbol.iterator]() { return new SingleShotGen(this); },
};

// Tag analog: a function whose prototype is EffectProto (Object.setPrototypeOf).
function Tag(name: string) {
  function TagClass() {}
  Object.setPrototypeOf(TagClass, EffectProto);
  (TagClass as any).key = name;
  return TagClass;
}

class Greeter extends (Tag("Greeter") as any) {}

// Drive a generator the way effect's fiber does: pull, resolve the yielded
// receiver to a service, resume with the service.
function runFiber(gen: Generator, resolve: (tag: any) => any) {
  let r = gen.next();
  while (!r.done) {
    r = gen.next(resolve(r.value));
  }
  return r.value;
}

const program = function* () {
  const g = yield* (Greeter as any);
  return (g as any).greet("Ada");
};

const services = new Map<any, any>();
services.set(Greeter, { greet: (n: string) => "Hello, " + n });

console.log(runFiber(program(), (tag) => services.get(tag)));
