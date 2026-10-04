// #11698: symbol-keyed class methods are properties of the class prototype.
// Deleting one must remove it from the prototype shape, and a computed super
// call must start at the home prototype's parent rather than at `this`.

class Deleted {
  *[Symbol.iterator]() {
    yield 1;
  }
}

delete (Deleted.prototype as any)[Symbol.iterator];
const deleted = new Deleted();
console.log(
  "deleted:",
  Object.prototype.hasOwnProperty.call(Deleted.prototype, Symbol.iterator),
  Symbol.iterator in deleted,
  typeof (deleted as any)[Symbol.iterator],
);

class Base {
  *[Symbol.iterator]() {
    yield 1;
    yield 2;
  }
}

class Derived extends Base {
  [Symbol.iterator]() {
    return super[Symbol.iterator]();
  }
}

console.log("super:", [...new Derived()].join(","));
