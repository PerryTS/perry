import { collect } from "perry/gc";
import { register, type Exports } from "./generated/guest";

type Payload = { text: string; bytes: Uint8Array; owner: number };
const resolves: Array<(value: Payload) => void> = [];
const inputs: Payload[] = [];
let wakes = 0;

const exports: Exports = {
    async wait(id, input) {
      inputs[id] = input;
      const value = await new Promise<Payload>((resolve) => { resolves[id] = resolve; });
      collect();
      return { text: value.text + "/" + id, bytes: value.bytes, get owner() { collect(); return id; } };
    },
    wake(id) {
      collect();
      resolves[id](inputs[id]);
      queueMicrotask(() => { wakes++; });
    },
    async immediate(input) {
      collect();
      return input;
    },
    async empty() { collect(); },
    status() { return wakes; },
    async reject() { throw new Error("Expected async guest rejection"); }
};
register(exports);
