// Test (#1673): a dynamic `import()` of a literal `node:` builtin resolves to
// the native-module namespace. Native builtins have no compiled-module backing
// and no `@__perry_ns_<prefix>` global — the dynamic-import dispatch builds the
// namespace object via `js_create_native_module_namespace` (the same object
// `require('node:crypto')` / `import * as` produce) and resolves the promise
// with it. An unsupported builtin still rejects, matching Node's failure mode.

async function main(): Promise<void> {
  const crypto = await import("node:crypto");
  console.log("crypto.randomUUID is fn:", typeof crypto.randomUUID === "function");

  const util = await import("node:util");
  console.log("util.format:", util.format("%s=%d", "n", 7));

  // Unsupported builtin → rejected promise → caught (Node parity).
  try {
    await import("node:this_builtin_does_not_exist");
    console.log("unsupported unexpectedly resolved");
  } catch {
    console.log("unsupported rejected");
  }
}

main();
