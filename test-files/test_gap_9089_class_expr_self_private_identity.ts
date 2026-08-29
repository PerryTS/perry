// A named class expression's inner binding denotes the class value created by
// this evaluation, not the shared compile-time template and not the receiver
// supplied to a static method through call/apply.
const makeState = (seed: string) => class c {
  static #value = seed;

  static get() {
    return c.#value;
  }

  static set(value: string) {
    c.#value = value;
  }
};

const A = makeState("A");
const B = makeState("B");
A.set("A2");
B.set("B2");
console.log(A.get(), B.get(), A.get.call(B));

// Read-modify-write lowering must route through that same evaluated class.
const makeCounter = () => class c {
  static #count = 0;

  static bump() {
    c.#count++;
    return c.#count;
  }
};

const C1 = makeCounter();
const C2 = makeCounter();
console.log([C1.bump(), C1.bump(), C2.bump(), C1.bump(), C2.bump()]);

// A nested class closes over the outer named class expression's lexical
// binding. The outer evaluation must therefore remain available after the
// outer class's static initializer has finished.
const makeOuter = () => class c {
  static #outer = false;

  static Inner = class d {
    static peek() {
      return c.#outer;
    }
  };
};

console.log(makeOuter().Inner.peek());

// Resolving the lexical self binding must not weaken ordinary private-brand
// rejection for an explicitly foreign class value.
const makeGuard = () => class c {
  static #secret = 1;

  static read(other: typeof c) {
    return other.#secret;
  }
};

const G1 = makeGuard();
const G2 = makeGuard();
try {
  console.log(G1.read(G2));
} catch (error) {
  console.log(error instanceof TypeError);
}
