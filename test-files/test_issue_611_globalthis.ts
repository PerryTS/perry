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

// Script-level `var` remains a global-object binding even when its initializer
// executes inside a compound statement.
if (true) {
  var nestedScriptVar = 42;
}
console.log("nested var:", (globalThis as any).nestedScriptVar);
try {
  var caughtScriptVar = "try";
} finally {
  var finalScriptVar = "finally";
}
console.log(
  "try/finally vars:",
  (globalThis as any).caughtScriptVar,
  (globalThis as any).finalScriptVar,
);

const seen: number[] = [];
const mirroredUpdates: number[] = [];
outer: for (
  var loopScriptVar = 0;
  loopScriptVar < 3;
  loopScriptVar++, mirroredUpdates.push((globalThis as any).loopScriptVar)
) {
  if (loopScriptVar === 0) continue outer;
  seen.push(loopScriptVar);
}
console.log(
  "labeled loop:",
  (globalThis as any).loopScriptVar,
  seen.join(","),
  mirroredUpdates.join(","),
);

switch (1) {
  case 1: {
    var switchScriptVar = "case";
    break;
  }
}
console.log("switch var:", (globalThis as any).switchScriptVar);
