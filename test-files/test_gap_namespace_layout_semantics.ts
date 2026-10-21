// A module namespace object is born in its compiler-owned layout: its
// export names sorted (ECMA-262 10.4.6 [[OwnPropertyKeys]]), live bindings,
// `export *` resolution (explicit exports win, `default` is never
// star-exported, a name two stars provide with different bindings is
// ambiguous and absent), null prototype, non-extensible. The namespace is
// the module's one namespace object (dynamic import resolves to it).
const ns: any = await import("./namespace_layout_semantics/barrel.ts");
const A: any = await import("./namespace_layout_semantics/a.ts");
const C: any = await import("./namespace_layout_semantics/c.ts");
console.log("identity      :", A === (await import("./namespace_layout_semantics/a.ts")));

console.log("keys(ns)      :", JSON.stringify(Object.keys(ns)));
console.log("keys(A)       :", JSON.stringify(Object.keys(A)));
console.log("keys(C)       :", JSON.stringify(Object.keys(C)));
const forIn: string[] = [];
for (const k in A) forIn.push(k);
console.log("for-in(A)     :", JSON.stringify(forIn));
console.log("json(C)       :", JSON.stringify(C));
console.log("shared in ns  :", "shared" in ns);
console.log("default in ns :", "default" in ns);
console.log("ns.zeta       :", ns.zeta);
console.log("ns.same       :", ns.same);
console.log("ns.cns.own    :", ns.cns.own);
console.log("A.default     :", A.default);

console.log("counter before:", A.counter, ns.counter);
A.bump();
A.bump();
console.log("counter after :", A.counter, ns.counter);

console.log("extensible    :", Object.isExtensible(ns), Object.isExtensible(A));
console.log("proto null    :", Object.getPrototypeOf(ns) === null, Object.getPrototypeOf(A) === null);
console.log("toString      :", Object.prototype.toString.call(ns));
console.log("toStringTag   :", (ns as any)[Symbol.toStringTag]);
console.log("sealed        :", Object.isSealed(A), Object.isSealed(C));
console.log("frozen        :", Object.isFrozen(A), Object.isFrozen(C));

const d1 = Object.getOwnPropertyDescriptor(A, "counter")!;
console.log("desc counter  :", d1.value, d1.writable, d1.enumerable, d1.configurable, "get" in d1);
const d2 = Object.getOwnPropertyDescriptor(A, "bump")!;
console.log("desc bump     :", typeof d2.value, d2.writable, d2.enumerable, d2.configurable);
const d3 = Object.getOwnPropertyDescriptor(ns, "zeta")!;
console.log("desc zeta     :", d3.value, d3.writable, d3.enumerable, d3.configurable);
console.log("desc missing  :", Object.getOwnPropertyDescriptor(ns, "shared"));

let deleted: string;
try {
  deleted = String(delete (A as any).zeta);
} catch (e) {
  deleted = e instanceof TypeError ? "TypeError" : "other";
}
console.log("delete zeta   :", deleted, A.zeta);
let added: string;
try {
  (A as any).extra = 1;
  added = "no throw";
} catch (e) {
  added = e instanceof TypeError ? "TypeError" : "other";
}
console.log("add extra     :", added, (A as any).extra);
console.log("hasOwn        :", Object.prototype.hasOwnProperty.call(ns, "beta"), Object.prototype.hasOwnProperty.call(ns, "shared"));
console.log("ownKeys(C)    :", Reflect.ownKeys(C).map(String).join(","));
const tagDesc = Object.getOwnPropertyDescriptor(C, Symbol.toStringTag)!;
console.log("desc tag      :", tagDesc.value, tagDesc.writable, tagDesc.enumerable, tagDesc.configurable);
const E: any = await import("./namespace_layout_semantics/empty.ts");
console.log("empty         :", Reflect.ownKeys(E).map(String).join(","), Object.isFrozen(E), Object.isExtensible(E), Object.prototype.toString.call(E));
