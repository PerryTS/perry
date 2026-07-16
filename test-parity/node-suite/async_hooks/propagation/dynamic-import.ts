import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
(globalThis as any).parityDynamicImportStore = () => storage.getStore();
const source =
  "data:text/javascript,export const store = globalThis.parityDynamicImportStore()";
const observed = await storage.run("dynamic-import", async () => {
  const namespace = await import(source);
  return namespace.store;
});
delete (globalThis as any).parityDynamicImportStore;
console.log("dynamic import store:", observed);
console.log("dynamic import outside:", String(storage.getStore()));
