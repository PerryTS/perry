// Test: dynamic import() with a non-statically-resolvable argument MUST fail to
// compile with a structured error. Issue #100 D1 — no `--allow-quickjs-eval`
// escape hatch yet, so unresolved paths surface as compile errors.
//
// This file is intentionally NOT runnable under `node --experimental-strip-types`
// without raising a different error (Node will try to resolve `path` at
// runtime). It exists for manual `perry compile` verification:
//
//   $ perry test-files/test_gap_dynamic_import_error.ts -o /tmp/out
//   Error: dynamic import() in module ...: path argument is not statically
//          resolvable (only string literals and ternary expressions of literals
//          are supported); consider enumerating with a ternary or registry
//          object

async function main(): Promise<void> {
  const path: string = "./dynamic_import_helper_a.ts";
  // @ts-ignore — Perry rejects this at compile time per issue #100.
  const m = await import(path);
  console.log(typeof m);
}

main();
