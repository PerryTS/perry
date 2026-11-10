// Gap test for #13965: a class declared in a function that runs more than
// once, with a computed-key member, is a fresh class object per evaluation.
// Its own name inside its body must be that evaluation, not a shared
// template: undici's `HeadersList` copy constructor does
// `init instanceof HeadersList`, and a false answer broke `fetch()`.

function make() {
  function cmp(a: any, b: any) {
    return a - b;
  }
  class List {
    copied: boolean;
    constructor(init?: any) {
      this.copied = init instanceof List;
    }
    [Symbol.iterator]() {
      return [1, 2][Symbol.iterator]();
    }
    sorted() {
      return [2, 1].sort(cmp);
    }
    static self() {
      return List;
    }
  }
  return List;
}

function makePlain() {
  class Keyed {
    [Symbol.toPrimitive]() {
      return 7;
    }
    static self() {
      return Keyed;
    }
    static create() {
      return new Keyed();
    }
  }
  return Keyed;
}

const A = make();
const B = make();
console.log(new A(new A()).copied, new B(new B()).copied);
console.log(new A(new B()).copied, new B(new A()).copied);
console.log(A.self() === A, B.self() === B, A === B);
console.log([...new A()].join(","), new A().sorted().join(","));

const P = makePlain();
const Q = makePlain();
console.log(P.self() === P, Q.self() === Q, P === Q);
console.log(P.create() instanceof P, Q.create() instanceof Q, P.create() instanceof Q);
console.log(+P.create());
