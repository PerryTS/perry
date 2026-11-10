// Regression (#6658, pi wall #7): an extracted builtin method used as a
// callback with an explicit `thisArg` — `arr.forEach(set.add, set)` — must
// invoke the builtin with `this = thisArg`, exactly as node does.
// Trigger in the wild: @babel/types' alias-expansion loop
// (`e5 ? e5.forEach(t4.add, t4) : t4.add(r4)`, pi-bundle.mjs:203827) threw
// "TypeError: Method Set.prototype.add called on incompatible receiver"
// during pi-native module init.
//
// Root cause: the DYNAMIC method-dispatch tower (js_native_call_method's
// dense-array arms) dropped args[1] for the whole Array.prototype callback
// family (forEach/map/filter/some/every/find/findIndex/findLast/
// findLastIndex) and dispatched to the dense helpers, which bind the
// callback's `this` to undefined (the spec rule for an ABSENT thisArg). The
// STATIC lowering already routed explicit-thisArg calls through the
// this-binding array-like engine — a receiver only reaches the dynamic tower
// when codegen can't prove its type (here: an object member read through a
// DYNAMIC key), which is why only the combined @babel/types shape reproduced
// and every statically-provable simplification worked.
//
// Also pinned: node's V8 brand-check TypeError message, receiver rendering
// included ("... called on incompatible receiver #<Object>").

// --- 1. the issue's combined shape (verbatim) -------------------------------
{
  const FLIPPED: any = { A: ["x", "y"], B: null };
  const allExpandedTypes: any[] = [
    { types: ["A", "B"], set: new Set() },
    { types: ["B"], set: new Set() },
  ];
  for (const { types: e4, set: t4 } of allExpandedTypes) {
    for (const r4 of e4) {
      const e5 = FLIPPED[r4];
      e5 ? e5.forEach(t4.add, t4) : t4.add(r4);
    }
  }
  console.log("sizes:", allExpandedTypes[0].set.size, allExpandedTypes[1].set.size);
}

// --- 2. the five previously-ruled-out simpler shapes (anchors) --------------
{
  // 2a. typed receiver
  const s = new Set<string>();
  ["a", "b"].forEach(s.add, s);
  console.log("anchor typed:", s.size);

  // 2b. Any receiver
  const t4: any = new Set();
  ["a"].forEach(t4.add, t4);
  console.log("anchor any:", t4.size);

  // 2c. extraction + .call
  const add = t4.add;
  add.call(t4, "q");
  console.log("anchor call:", t4.size);

  // 2d. destructured for-of
  const rows: any[] = [{ set: new Set() }];
  for (const { set: r } of rows) ["a"].forEach(r.add, r);
  console.log("anchor destructured:", rows[0].set.size);

  // 2e. ternary shape
  const e5: any = ["z1", "z2"];
  const t5: any = new Set();
  e5 ? e5.forEach(t5.add, t5) : t5.add("z");
  console.log("anchor ternary:", t5.size);
}

// --- 3. minimal dynamic-tower trigger (dynamic-key member read) -------------
// SRC[key] with a non-literal key defeats flow typing, so `.forEach` on the
// result dispatches through the runtime tower — the path that dropped thisArg.
const SRC: any = { A: ["x", "y"] };
const KEYS = ["A"];
const dyn = SRC[KEYS[0]]; // e5's provenance in the bundle

{
  const t4: any = new Set();
  dyn.forEach(t4.add, t4);
  console.log("dynamic Set.add:", t4.size, t4.has("x"), t4.has("y"));
}

// --- 4. other builtin methods extracted the same way ------------------------
{
  // Map.prototype.set — forEach passes (value, index), so m.set("x", 0) etc.
  const m: any = new Map();
  dyn.forEach(m.set, m);
  console.log("dynamic Map.set:", m.size, m.get("x"), m.get("y"));

  // Array.prototype.push — forEach passes (value, index, array): 3 pushes/el.
  const a2: any = [];
  dyn.forEach(a2.push, a2);
  console.log("dynamic Array.push:", a2.length, a2[0], a2[1] === 0, a2[3]);

  // Set.prototype.forEach with an extracted add (collection engine, not the
  // array tower — must bind thisArg the same way).
  const src: any = new Set(["p", "q"]);
  const dst: any = new Set();
  src.forEach(dst.add, dst);
  console.log("set-forEach Set.add:", dst.size, dst.has("p"), dst.has("q"));
}

// --- 5. the whole dynamic callback family binds thisArg ---------------------
{
  const marker = { tag: "T" };
  const seen: boolean[] = [];
  function observe(this: any): boolean {
    seen.push(this === marker);
    return true;
  }
  dyn.forEach(observe, marker);
  dyn.map(observe, marker);
  dyn.filter(observe, marker);
  dyn.some(function (this: any) { seen.push(this === marker); return false; }, marker);
  dyn.every(observe, marker);
  dyn.find(function (this: any) { seen.push(this === marker); return false; }, marker);
  dyn.findIndex(function (this: any) { seen.push(this === marker); return false; }, marker);
  dyn.findLast(function (this: any) { seen.push(this === marker); return false; }, marker);
  dyn.findLastIndex(function (this: any) { seen.push(this === marker); return false; }, marker);
  console.log("family this-bound:", seen.length, seen.every(Boolean));
}

// --- 6. cases where node DOES throw (message-identical) ---------------------
function thrown(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (e: any) {
    return e.constructor.name + ": " + e.message;
  }
}

{
  const t4: any = new Set();
  const m: any = new Map();
  // absent thisArg: callback `this` is undefined → brand check throws
  console.log("throw undef:", thrown(() => dyn.forEach(t4.add)));
  // wrong-brand thisArg: plain object
  console.log("throw obj:", thrown(() => dyn.forEach(t4.add, {})));
  // cross-brand: Map.prototype.set with a Set thisArg
  console.log("throw cross:", thrown(() => dyn.forEach(m.set, t4)));
  // primitive thisArgs render per V8's NoSideEffectsToString
  console.log("throw num:", thrown(() => dyn.forEach(t4.add, 5.5 as any)));
  console.log("throw str:", thrown(() => dyn.forEach(t4.add, "abc" as any)));
  // reflective .call keeps the same message family
  console.log("throw call:", thrown(() => (Set.prototype.add as any).call(null, 1)));
  class Foo {}
  console.log("throw inst:", thrown(() => (Set.prototype.add as any).call(new Foo(), 1)));
}
