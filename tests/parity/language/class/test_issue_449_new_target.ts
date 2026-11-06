// Regression test for issue #449:
// Inside a class constructor, `new.target` returned the f64 NaN bit
// pattern instead of the constructor function reference. Specifically
// `new.target?.name ?? "undefined"` rendered as the literal string
// "NaN" because the v0.5.502-style Object-literal synthesis for
// `MetaProp(NewTarget)` hit the same module-globals NaN-boxing bug
// where string fields on the synthesized object read back as raw u64
// handle bits instead of NaN-boxed values (rendering as `2e-323` or
// `NaN` depending on the access path).
//
// Fix: fold `new.target.<prop>` and `new.target?.<prop>` directly to
// a string/undefined literal at HIR lowering time, mirroring the
// v0.5.502 approach for `import.meta.<prop>`. Bare `new.target` falls
// through to the existing Object fallback (still truthy, still works
// for `new.target ? a : b`).
//
// platforms: macos, linux

// Class constructor → returns class reference (with `.name` property).
class MyConstructable {
  constructor() {
    console.log("class new.target.name:", new.target?.name ?? "undefined");
  }
}
new MyConstructable();

// Direct (non-optional) member access still folds.
class Direct {
  constructor() {
    const target = new.target;
    // bare `new.target` is truthy (object), so this prints true
    console.log("direct truthy:", !!target);
    // direct `.name` access folds to the class name string
    console.log("direct name:", new.target.name);
  }
}
new Direct();

// Arrow function inside constructor → still returns outer constructor's
// new.target (since arrow functions inherit the enclosing `this`/
// `new.target` lexically).
class WithArrow {
  constructor() {
    const inner = () => new.target?.name ?? "undefined";
    console.log("arrow new.target.name:", inner());
  }
}
new WithArrow();

// Plain function called bare (no `new`) → new.target is undefined,
// `?.name` short-circuits to undefined, `?? "undefined"` coalesces.
function plainBare() {
  console.log("plain bare:", new.target?.name ?? "undefined");
}
plainBare();

// Truthiness check on bare `new.target` inside a constructor — the
// fallback Object synthesis is truthy so the ternary picks the `yes`
// branch. (Outside the constructor it would pick `no` since
// new.target is undefined.)
class TruthyCheck {
  constructor() {
    const result = new.target ? "yes" : "no";
    console.log("ternary:", result);
  }
}
new TruthyCheck();
