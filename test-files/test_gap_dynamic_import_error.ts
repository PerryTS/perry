// Test: dynamic import() with a non-statically-resolvable argument MUST fail to
// compile with a structured error. Issue #100 D1 — no `--allow-quickjs-eval`
// escape hatch yet, so unresolved paths surface as compile errors.
//
// This file is intentionally NOT runnable under `node --experimental-strip-types`
// without raising a different error (Node will try to resolve `path` at
// runtime). It exists for manual `perry compile` verification:
//
//   $ perry test-files/test_gap_dynamic_import_error.ts -o /tmp/out
//   Error: dynamic import() in module ...: path argument references a binding
//          that is not a module-level const initialized to a literal (only
//          string literals, ternaries, template literals over const locals,
//          and the module-level consts themselves are supported)

async function main(): Promise<void> {
  // Function-local const — the resolver only follows MODULE-level
  // const locals (a module-level `path` would resolve through
  // `collect_module_const_locals`; a function-local one does not).
  const path: string = "./dynamic_import_helper_a.ts";
  // @ts-ignore — Perry rejects this at compile time per issue #100.
  const m = await import(path);
  console.log(typeof m);
}

main();
