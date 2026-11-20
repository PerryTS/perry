// A closure lowered before a per-evaluation class declaration (heritage is the
// native Error) must read the class of its own evaluation, not of the latest
// one. Each call of the enclosing function owns its own class binding, so the
// first call's closures keep resolving to the first call's class after a
// second call ran (esbuild's CommonJS output exports classes through such
// closures: `__export(exports, { C: () => C })` precedes `class C extends Error`).

// 1. forward getter next to the class, factory invoked twice
function factory() {
  const exported = {};
  Object.defineProperty(exported, 'B', { get: () => B, enumerable: true });
  class B extends Error {
    constructor(options) {
      super(options.message);
      this.code = options.code;
    }
  }
  exported.direct = B;
  return exported;
}
const one = factory();
const two = factory();
console.log(
  'first', one.B === one.direct,
  'second', two.B === two.direct,
  'distinct', one.direct !== two.direct,
  'cross', one.B === two.direct,
);
const e1 = new one.direct({ message: 'm', code: 'c' });
console.log(e1 instanceof one.B, e1 instanceof two.B);

// 2. a static method and a constructor that name the class, per call
function resetting() {
  const exported = {};
  Object.defineProperty(exported, 'B', { get: () => B, enumerable: true });
  class B extends Error {
    constructor(options) {
      super(options.message);
      Object.setPrototypeOf(this, B.prototype);
    }
    static create(message) {
      return new B({ message });
    }
  }
  return exported;
}
const r1 = resetting();
const r2 = resetting();
const m1 = r1.B.create('x');
const m2 = r2.B.create('y');
console.log(
  'static',
  m1 instanceof r1.B, m1 instanceof r2.B, m1.message,
  m2 instanceof r2.B, m2 instanceof r1.B, m2.message,
);

// 3. a forward function declaration reading the class
function declared() {
  function get() {
    return C;
  }
  class C extends Error {}
  return { get, C };
}
const d1 = declared();
const d2 = declared();
console.log('fn decl', d1.get() === d1.C, d2.get() === d2.C, d1.C !== d2.C);

// 4. a nested closure two levels below the use
function nested() {
  const outer = () => () => K;
  class K extends Error {}
  return { outer, K };
}
const n1 = nested();
const n2 = nested();
console.log('nested', n1.outer()() === n1.K, n2.outer()() === n2.K, n1.K !== n2.K);

// 5. a block scope inside a loop gets a binding per iteration
const getters = [];
const classes = [];
for (let i = 0; i < 3; i++) {
  const read = () => L;
  class L extends Error {}
  getters.push(read);
  classes.push(L);
}
console.log(
  'loop',
  getters.every((g, i) => g() === classes[i]),
  new Set(classes).size,
);

// 6. a function expression (the shape of a bundled module wrapper)
const wrap = function () {
  const read = () => W;
  class W extends Error {}
  return { read, W };
};
const w1 = wrap();
const w2 = wrap();
console.log('fn expr', w1.read() === w1.W, w2.read() === w2.W, w1.W !== w2.W);

// 7. a class with static state and captured outer variables and no heritage
// (the first evaluation shares the template, later ones are fresh)
function stateful(x) {
  const read = () => S;
  class S {
    static count = 0;
    static bump() {
      return ++S.count;
    }
    value() {
      return x;
    }
  }
  return { read, S };
}
const s1 = stateful(1);
const s2 = stateful(2);
s1.S.bump();
s1.S.bump();
s2.S.bump();
console.log(
  'stateful',
  s1.read() === s1.S, s2.read() === s2.S, s1.S !== s2.S,
  s1.read().count, s2.read().count,
  new (s1.read())().value(), new (s2.read())().value(),
);
