// Test: dynamic import() with a ternary of literal paths — both branches enter
// the compile-time import graph; the runtime picks one based on the condition.
// Issue #100 — Perry MVP: the resolved namespace is currently empty.
// Run: node --experimental-strip-types test-files/test_gap_dynamic_import_ternary.ts

async function load(flag: boolean): Promise<unknown> {
  return await import(flag ? "./dynamic_import_helper_a.ts" : "./dynamic_import_helper_b.ts");
}

async function main(): Promise<void> {
  const a = await load(true);
  const b = await load(false);
  console.log(typeof a);
  console.log(typeof b);
}

main();
