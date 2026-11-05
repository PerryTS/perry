import { loadA } from "./dynamic_import_cycle_b.ts";

async function main(): Promise<void> {
  const v = await loadA();
  console.log(v);
}

main();
