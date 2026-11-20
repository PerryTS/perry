// Test: dynamic import() with a ternary of literal paths — both branches
// enter the compile-time import graph; the runtime picks one based on the
// condition, and the resolved namespace exposes that target's exports.

async function load(flag: boolean) {
  return await import(
    flag ? "./dynamic_import_helper_a.ts" : "./dynamic_import_helper_b.ts"
  );
}

async function main(): Promise<void> {
  const a = await load(true);
  const b = await load(false);
  console.log(a.x);
  console.log(b.label);
}

main();
