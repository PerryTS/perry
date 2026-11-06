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
    _initialize() {
      calls++;
      return 99;
    },
  },
};
const wasi = new W({ version: "preview1" });
console.log("before:", calls);
if (typeof wasi.initialize !== "function") {
  console.log("return: unavailable");
} else {
  console.log("return:", String(wasi.initialize(instance)));
}
console.log("after:", calls);
