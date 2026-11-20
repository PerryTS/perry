import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

async function* values() {
  console.log("generator start:", storage.getStore());
  yield 1;
  await Promise.resolve();
  console.log("generator continuation:", storage.getStore());
  yield 2;
}

const seen = await storage.run("iterator", async () => {
  const output: number[] = [];
  for await (const value of values()) {
    console.log("iterator value:", value, storage.getStore());
    output.push(value);
  }
  return output.join(",");
});

console.log("iterator result:", seen);
console.log("iterator outside:", String(storage.getStore()));
