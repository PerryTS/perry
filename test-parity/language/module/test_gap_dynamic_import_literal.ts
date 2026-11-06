// Test: dynamic import() with a string-literal path resolves at compile time,
// and the resolved namespace exposes the target module's exports.

async function main(): Promise<void> {
  const m = await import("./dynamic_import_helper_a.ts");
  console.log(m.x);
  console.log(m.greet());
}

main();
