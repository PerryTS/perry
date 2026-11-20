"use strict";
// Lane D (NXPD): accessor reads answered by the layout must keep [[Get]] /
// [[Set]] semantics exactly: a getter with an effect is called once per read,
// a `return <binding>` getter sees the binding's current value (and throws in
// its dead zone), accessors in an overflow table are served with the right
// `this`, setters are called once with the receiver and the value.

function readMany(o, k, n) {
  let last;
  for (let i = 0; i < n; i++) last = o[k];
  return last;
}

// 1. A getter with a side effect: called exactly once per read.
{
  let calls = 0;
  const o = {};
  Object.defineProperty(o, "x", { enumerable: true, get: function () { calls++; return 7; } });
  let s = 0;
  for (let i = 0; i < 50; i++) s += o.x;
  console.log("effect", s, calls);
  const k = "x";
  readMany(o, k, 5);
  console.log("effect keyed", calls);
}

// 2. `return <binding>` over a reassigned `let`: each read sees the new value.
{
  let v = 1;
  const o = {};
  Object.defineProperty(o, "v", { get: function () { return v; } });
  const seen = [];
  for (let i = 0; i < 4; i++) {
    seen.push(o.v);
    v = v * 10;
  }
  console.log("live", seen.join(","));
  const w = { get value() { return v; } };
  v = "str";
  console.log("live literal", w.value);
}

// 3. `return <binding>` while the binding is in its dead zone: ReferenceError.
{
  const o = {};
  Object.defineProperty(o, "t", { get: function () { return late; } });
  try {
    console.log("tdz read", o.t);
  } catch (e) {
    console.log("tdz", e instanceof ReferenceError);
  }
  let late = "ready";
  console.log("tdz after", o.t);
}

// 4. A pair without a getter reads `undefined`; the setter still runs.
{
  let got;
  const o = {};
  Object.defineProperty(o, "w", { set: function (x) { got = x; }, enumerable: true });
  console.log("setonly", o.w, readMany(o, "w", 3));
  o.w = 5;
  console.log("setonly set", got);
}

// 5. An accessor in an overflow table (an object past its unique-key run).
{
  const o = {};
  for (let i = 0; i < 1100; i++) o["k" + i] = i;
  let calls = 0;
  Object.defineProperty(o, "acc", {
    get: function () { calls++; return this === o ? "self" : "other"; },
    set: function (x) { calls += 100; this.k0 = x; },
    configurable: true,
  });
  console.log("overflow", o.acc, o.acc, calls);
  o.acc = 42;
  console.log("overflow set", o.k0, calls);
  let v = 3;
  Object.defineProperty(o, "bind", { get: function () { return v; } });
  v = 4;
  console.log("overflow bind", o.bind, o.k1099);
}

// 6. CJS: a declared export key redefined as an accessor (overflow), and a
//    setter site on it.
{
  const d = require("../../../fixtures/programs/accessor_probe_semantics/declared.cjs");
  console.log("declared", d.y, d.y, d.reads());
  d.z = "a";
  d.z = "b";
  console.log("declared set", d.stored(), d.z);
}

// 7. A class getter on the prototype and a subclass instance: `this` is the
//    receiver; an inherited `return <binding>` getter reads the binding.
{
  let shared = "s1";
  class A {
    constructor() { this.n = 2; }
    get twice() { return this.n * 2; }
    get shared() { return shared; }
    set n2(x) { this.n = x; }
  }
  class B extends A {}
  const b = new B();
  let s = 0;
  for (let i = 0; i < 10; i++) s += b.twice;
  shared = "s2";
  b.n2 = 5;
  console.log("class", s, b.shared, b.twice);
}
