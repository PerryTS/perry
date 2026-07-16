import { AsyncLocalStorage } from "node:async_hooks";
import * as workerThreads from "node:worker_threads";

type Locks = {
  request<T>(
    name: string,
    callback: (lock: { name: string; mode: string }) => T | Promise<T>,
  ): Promise<T>;
};
const locks = (workerThreads as any).locks as Locks | undefined;
const available = typeof locks?.request === "function";
console.log("web locks availability:", available);

if (locks) {
  const storage = new AsyncLocalStorage<string>();
  const contexts: Array<string | undefined> = [];
  const result = await storage.run("web-lock", () =>
    locks.request("async-hooks-outer", async (outer) => {
      contexts.push(storage.getStore());
      await Promise.resolve();
      contexts.push(storage.getStore());
      const inner = await locks.request("async-hooks-inner", async (lock) => {
        await new Promise<void>((resolve) => setImmediate(resolve));
        contexts.push(storage.getStore());
        return `${lock.name}:${lock.mode}`;
      });
      return `${outer.name}:${outer.mode}|${inner}`;
    }),
  );
  console.log("web locks contexts:", contexts.join(","));
  console.log("web locks result:", result);
  console.log("web locks outside:", String(storage.getStore()));
  storage.disable();
}
