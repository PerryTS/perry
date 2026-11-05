import { collect } from "perry/gc";
import { AsyncLocalStorage } from "node:async_hooks";
import { register, type Exports } from "./generated/guest";

type Payload = { text: string; bytes: Uint8Array; owner: number };
const resolves: Array<(value: Payload) => void> = [];
const inputs: Payload[] = [];
let wakes = 0;
const storage = new AsyncLocalStorage<{ id: number }>();
function context(id: number | undefined): void {
  const current = storage.getStore();
  if (id === undefined ? current !== undefined : current?.id !== id) {
    throw new Error("WASI guest AsyncLocalStorage isolation failed");
  }
}

const exports: Exports = {
    async wait(id, input) {
      context(undefined);
      storage.enterWith({ id });
      inputs[id] = input;
      const value = await storage.run({ id: id + 1000 }, async () => {
        context(id + 1000);
        const result = await new Promise<Payload>((resolve) => { resolves[id] = resolve; });
        collect(); context(id + 1000);
        await Promise.resolve();
        context(id + 1000);
        return result;
      });
      context(id);
      collect();
      return { text: value.text + "/" + id, bytes: value.bytes, get owner() { collect(); context(id); return id; } };
    },
    wake(id) {
      context(undefined);
      collect();
      resolves[id](inputs[id]);
      queueMicrotask(() => { context(undefined); wakes++; });
    },
    async immediate(input) {
      context(undefined);
      const result = storage.run({ id: 9000 }, () => { collect(); context(9000); return input; });
      context(undefined);
      return result;
    },
    async empty() { collect(); },
    status() { context(undefined); return wakes; },
    async reject() { throw new Error("Expected async guest rejection"); }
};
register(exports);
