// Runs of fresh closures born from one table (`closure_births`): hoisted
// function declarations bound in scope slots, and object literals of arrow
// getters (the esbuild `__export` shape). Each activation must bear fresh,
// distinct closures in source order, hoisting must make every declaration
// visible before its statement, and getters must read live bindings.

const defProp = Object.defineProperty;
const exportAll = (target: any, all: any) => {
  for (const name in all) defProp(target, name, { get: all[name], enumerable: true });
};

function makeModule(seed: number) {
  const ns: any = {};
  // Getters before the declarations they read: hoisting already bound them.
  exportAll(ns, {
    a0: () => a0,
    a1: () => a1,
    a2: () => a2,
    a3: () => a3,
    a4: () => a4,
    a5: () => a5,
    a6: () => a6,
    a7: () => a7,
    a8: () => a8,
    a9: () => a9,
    counter: () => counter,
    Color: () => Color,
  });
  // Called before its declaration statement (`counter` is a hoisted var).
  var counter = seed;
  const early = a3(2);
  function a0(n: number): number { counter++; return n <= 0 ? 0 : a1(n - 1) + 1; }
  function a1(n: number): number { counter++; return n <= 0 ? 1 : a2(n - 1) + 1; }
  function a2(n: number): number { counter++; return n <= 0 ? 2 : a3(n - 1) + 1; }
  function a3(n: number): number { counter++; return n <= 0 ? 3 : a4(n - 1) + 1; }
  function a4(n: number): number { counter++; return n <= 0 ? 4 : a5(n - 1) + 1; }
  function a5(n: number): number { counter++; return n <= 0 ? 5 : a6(n - 1) + 1; }
  function a6(n: number): number { counter++; return n <= 0 ? 6 : a7(n - 1) + 1; }
  function a7(n: number): number { counter++; return n <= 0 ? 7 : a8(n - 1) + 1; }
  function a8(n: number): number { counter++; return n <= 0 ? 8 : a9(n - 1) + 1; }
  function a9(n: number, ...rest: number[]): number { counter += 1 + rest.length; return n <= 0 ? 9 + seed : a0(n - 1) + 1; }
  var Color = ((Color2: any) => {
    Color2[Color2["Red"] = 0] = "Red";
    Color2[Color2["Green"] = 1] = "Green";
    Color2[Color2["Blue"] = 4] = "Blue";
    return Color2;
  })(Color || {});
  // Merged declaration: a second IIFE extends the same object.
  ((Color2: any) => {
    Color2[Color2["Alpha"] = 9] = "Alpha";
  })(Color || (Color = {}));
  // A hoisted function reassigned later: the getter sees the new value.
  const before = a5;
  function replaceA5() {
    a5 = ((n: number) => -n) as any;
  }
  return { ns, early, before, replaceA5, bump: () => counter++ };
}

const m1 = makeModule(10);
const m2 = makeModule(20);
console.log("early", m1.early, m2.early);
console.log("calls", m1.ns.a0(12), m2.ns.a0(12), m1.ns.a9(0), m2.ns.a9(0, 1, 2));
console.log("live counter", m1.ns.counter, m2.ns.counter);
m1.bump();
console.log("after bump", m1.ns.counter, m2.ns.counter);
console.log("fresh per activation", m1.ns.a4 === m2.ns.a4, m1.ns.a4 === m1.ns.a4);
console.log("names", m1.ns.a0.name, m1.ns.a9.name, m1.ns.a9.length, typeof m1.ns.a2);
console.log("enum", m1.ns.Color.Red, m1.ns.Color[4], m1.ns.Color.Alpha, m1.ns.Color[9], Object.keys(m1.ns.Color).join(","));
m1.replaceA5();
console.log("reassigned", m1.ns.a5 === m1.before, m1.ns.a5(3), m2.ns.a5 === m2.before);
console.log("keys", Object.keys(m1.ns).join(","));
console.log("descriptor", JSON.stringify(Object.getOwnPropertyDescriptor(m1.ns, "a1"), (k, v) => typeof v === "function" ? "fn" : v));

// Fresh closures per call capture this activation's bindings, including a
// plain (never reassigned) parameter captured by value.
function handlers(prefix: string) {
  let hits = 0;
  function h0() { hits++; return prefix + "0"; }
  function h1() { hits++; return prefix + "1"; }
  function h2() { hits++; return prefix + "2"; }
  function h3() { hits++; return prefix + "3"; }
  function h4() { hits++; return prefix + "4"; }
  function h5() { hits++; return prefix + "5"; }
  function h6() { hits++; return prefix + "6"; }
  function h7() { hits++; return prefix + "7"; }
  async function h8() { hits++; return prefix + "8"; }
  return { all: [h0, h1, h2, h3, h4, h5, h6, h7, h8], hits: () => hits };
}
const ha = handlers("a");
const hb = handlers("b");
console.log(ha.all.slice(0, 8).map((f) => f()).join(""), hb.all.slice(0, 8).map((f) => f()).join(""));
console.log("hits", ha.hits(), hb.hits(), ha.all[3] === hb.all[3]);
console.log("async", ha.all[8].constructor.name, typeof ha.all[8]);
ha.all[8]().then((v: string) => console.log("async value", v, ha.hits()));

// Nested: the run sits in an inner function whose captures come through its
// own closure slots.
function outer() {
  let shared = 0;
  function inner() {
    function n0() { return ++shared; }
    function n1() { return n0() + 1; }
    function n2() { return n1() + 1; }
    function n3() { return n2() + 1; }
    function n4() { return n3() + 1; }
    function n5() { return n4() + 1; }
    function n6() { return n5() + 1; }
    function n7() { return n6() + 1; }
    function n8() { return n7() + 1; }
    function n9() { return n8() + 1; }
    return n9;
  }
  return { a: inner(), b: inner(), shared: () => shared };
}
const o = outer();
console.log("nested", o.a(), o.b(), o.a === o.b, o.shared());

// An object literal of arrow getters with more than one capture source.
function table(x: number, y: number) {
  let z = x + y;
  const t = {
    p0: () => x, p1: () => y, p2: () => z, p3: () => x + y,
    p4: () => x * z, p5: () => y - z, p6: () => z * 2, p7: () => x - y,
    set: (v: number) => { z = v; },
  };
  return t;
}
const t1 = table(2, 3);
const t2 = table(5, 7);
t1.set(100);
console.log("table", t1.p0(), t1.p1(), t1.p2(), t1.p4(), t2.p2(), t2.p6(), t1.p3 === t2.p3);
