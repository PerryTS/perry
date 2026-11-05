async function main(): Promise<void> {
  const m = await import("./dynamic_import_tla_helper.ts");
  console.log(m.value);
  console.log(m.tag);
}

main();
