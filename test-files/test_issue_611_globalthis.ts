// Issue #611 (Effect Utils.ts SIGSEGV) bisected to: globalThis[<computedKey>]
// reads/writes were dropped — `(globalThis as any)[id] = m; (globalThis as
// any)[id]` returned undefined instead of `m`. Effect's GlobalValue.ts uses
// this exact pattern: `globalThis[globalStoreId] ??= new Map();
// globalStore = globalThis[globalStoreId] as Map<...>` — `globalStore` was
// always undefined → next `.has()` SIGSEGV'd.
const id = "myKey";
(globalThis as any)[id] = "hello";
const v = (globalThis as any)[id];
console.log("v:", v);
console.log("typeof v:", typeof v);

// Mirror the Effect / hono / drizzle pattern: nullish-assignment store.
const storeId = "perry/test/store";
(globalThis as any)[storeId] ??= new Map();
const store = (globalThis as any)[storeId] as Map<string, string>;
console.log("typeof store:", typeof store);
store.set("k", "v");
console.log("store.get('k'):", store.get("k"));
