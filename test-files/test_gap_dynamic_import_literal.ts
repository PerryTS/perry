// Test: dynamic import() with a string-literal path resolves at compile time.
// Issue #100 — Perry MVP: the namespace object is currently empty (member access
// returns undefined). This test only asserts that the await completes and the
// resolved value is a non-null object.
// Run: node --experimental-strip-types test-files/test_gap_dynamic_import_literal.ts

async function main(): Promise<void> {
  const m = await import("./dynamic_import_helper_a.ts");
  console.log(typeof m);
  console.log(m !== null && m !== undefined);
}

main();
