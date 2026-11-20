// Repro for gap 2 from PR #899: dynamic static-method dispatch on factory-returned classes.
//
// Effect's `make()` factory returns a class with static methods. Doing
// `make().pipe(...)` on the returned class should route to its static
// `pipe()` method, but perry's runtime property-get on a class ref only
// consults the instance vtable, not the static-method registry.
//
// Expected: factory-returned class can have its static methods called
// dynamically via property access.

function make() {
  return class {
    static pipe(x: number) {
      return x * 2;
    }
    static label() {
      return "factory-class";
    }
  };
}

const C = make();
// Direct static call on a factory-returned class should work.
console.log(C.pipe(21));      // expect: 42
console.log(C.label());       // expect: factory-class

// Each invocation should return its own class.
const D = make();
console.log(D.pipe(5));       // expect: 10

// Chained: make().pipe(...) — the static method is dispatched without a
// local binding.
console.log(make().pipe(7));  // expect: 14

// typeof check on the static property
console.log(typeof C.pipe);   // expect: function
console.log(typeof C.label);  // expect: function
