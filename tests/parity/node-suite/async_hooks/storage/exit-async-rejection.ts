import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
storage.enterWith("outer");

try {
  await storage.exit(async () => {
    console.log("exit rejection start:", String(storage.getStore()));
    await Promise.resolve();
    console.log("exit rejection continuation:", String(storage.getStore()));
    throw new Error("expected-rejection");
  });
} catch (error) {
  console.log("exit rejection error:", (error as Error).message);
}

console.log("exit rejection restored:", storage.getStore());
storage.disable();
