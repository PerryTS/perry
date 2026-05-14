// Test: dynamic import() of a barrel module — `export * from "./inner.ts"`
// surfaces inner's exports through the dynamic-import namespace.

async function main(): Promise<void> {
  const m = await import("./dynamic_import_barrel.ts");
  console.log(m.v);
}

main();
