import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

async function main() {
  const returned = await storage.run("promise", async () => {
    console.log("promise start:", storage.getStore());
    await Promise.resolve();
    console.log("promise continuation:", storage.getStore());

    await new Promise<void>((resolve) => {
      queueMicrotask(() => {
        console.log("queueMicrotask callback:", storage.getStore());
        resolve();
      });
    });

    console.log("after queueMicrotask:", storage.getStore());
    return "completed";
  });

  console.log("async return:", returned);
  console.log("outside after await:", String(storage.getStore()));
}

await main();
