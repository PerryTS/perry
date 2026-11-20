// #321 / #66 (#1787 follow-up): `class Sub extends Base` where `Base` is a
// runtime-value FUNCTION (not a class declaration). Effect's `Data.Class`
// uses this exact pattern — an IIFE returns the constructor function and
// `class Point extends Data.Class<{x,y}>` instances need `super(args)` to
// actually invoke the parent function with `this` bound to the instance, so
// any `Object.assign(this, args)` writes the subclass's fields.
//
// Without the fix, perry's `super(args)` dispatched to no-op for non-static
// parents — `Base`'s body never ran, `this` stayed null inside `Sub`'s
// constructor body, and `s.x` came back `undefined`.

// Repro 1: simplest IIFE-returned constructor function (Effect's Data.Class shape).
const Base: any = (function () {
  function Base(this: any, args: any) {
    if (args) Object.assign(this, args);
  }
  return Base;
})();

class Sub extends Base {
  constructor(args: any) {
    super(args);
  }
}

const s = new Sub({ x: 7, y: 9 });
console.log("s.x:", (s as any).x);
console.log("s.y:", (s as any).y);

// Repro 2: parent function reads `this` and writes a constant default.
const WithDefault: any = (function () {
  function WithDefault(this: any) {
    this.flag = "default-from-parent";
  }
  return WithDefault;
})();

class Child extends WithDefault {
  constructor() {
    super();
  }
}

const c = new Child();
console.log("c.flag:", (c as any).flag);

// Repro 3: subclass field initializers run AFTER super() returns (JS spec).
const Empty: any = (function () {
  function Empty(this: any) {}
  return Empty;
})();

class WithFields extends Empty {
  ownField: string = "subclass-init";
  constructor() {
    super();
  }
}

const w = new WithFields();
console.log("w.ownField:", w.ownField);
