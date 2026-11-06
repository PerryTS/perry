// Class statics are the constructor's OWN properties: reflection, spread and
// Object.assign see exactly node's keys (no compiler-internal names).
class A { static s = 1; static #p = 2; static m() { return A.#p; } static { (A as any).boot = "b"; } }
class B extends A { static t = 3; }
console.log(Reflect.ownKeys(A).map(String).sort().join(","));
console.log(Object.getOwnPropertyNames(B).sort().join(","));
console.log(Object.keys({ ...(A as any) }).join(","), Object.keys({ ...(B as any) }).join(","));
console.log(Object.keys(Object.assign({}, A)).join(","), JSON.stringify(Object.assign({}, B)));
console.log(Object.entries(A).map(([k, v]) => k + "=" + v).join(","));
for (const k in B) console.log("for-in", k);
console.log(A.m());
