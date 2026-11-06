// Test (#1672): a module that BOTH statically imports a source AND
// dynamically `import()`s the same source must still resolve the dynamic
// import to the module namespace.
//
// The fold in collect_modules keeps the *static* edge (it provides binding
// materialization + the eager init-order pin, both requiring is_dynamic =
// false) and suppresses the synthetic dynamic edge. Before the fix, that
// dropped the source from the dynamic-import dispatch map entirely, so
// `await import("./dynamic_import_helper_a.ts")` rejected with `undefined`
// even though the module was compiled in. The static edge now carries an
// `is_dynamic_target` flag so the dispatch map is still populated.

import { x } from "./dynamic_import_helper_a.ts";

async function main(): Promise<void> {
  console.log("static x:", x);
  const m = await import("./dynamic_import_helper_a.ts");
  console.log("dynamic x:", m.x);
  console.log("dynamic greet:", m.greet());
}

main();
