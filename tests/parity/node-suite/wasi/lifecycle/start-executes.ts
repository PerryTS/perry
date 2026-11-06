import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
let calls = 0;
const instance = {
  exports: {
    memory: createMemory(),
    _start() {
      calls++;
      return 99;
    },
  },
};
const wasi = new W({ version: "preview1", returnOnExit: true });
console.log("before:", calls);
console.log("return:", wasi.start(instance));
console.log("after:", calls);
