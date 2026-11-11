import { AsyncLocalStorage } from "node:async_hooks";

class NamedStorage extends AsyncLocalStorage<string> {
  capture(label: string, value: string) {
    return this.run(value, async () => {
      const sync = this.getStore();
      await Promise.resolve();
      return `${label}:${sync}:${this.getStore()}`;
    });
  }
}

const storage = new NamedStorage();
console.log(
  "als subclass shape:",
  storage instanceof NamedStorage,
  storage instanceof AsyncLocalStorage,
);
console.log("als subclass result:", await storage.capture("label", "store"));
console.log("als subclass restored:", storage.getStore() === undefined);
