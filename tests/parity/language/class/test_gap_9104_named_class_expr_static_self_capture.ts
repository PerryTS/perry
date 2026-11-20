// A named class expression's inner binding remains visible from a static
// method even when the class value is stored under a different outer name.
const C = class Named {
  static f() {
    return Named === C;
  }
};

console.log(C.f());

// Nested arrows close over the same per-evaluation self binding that the
// static method itself uses. Each evaluation keeps its own private state.
const make = () => class c {
  static #v = 0;

  static self() {
    return (() => c)();
  }

  static f() {
    return (() => {
      c.#v++;
      return c.#v;
    })();
  }
};

const A = make();
const B = make();
console.log(A.self() === A, B.self() === B, A.self() !== B.self());
console.log([A.f(), A.f(), B.f()]);
