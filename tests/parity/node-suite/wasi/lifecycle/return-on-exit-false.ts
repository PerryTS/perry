import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
const wasi = new W({ version: "preview1", returnOnExit: false });
const instance = {
  exports: {
    memory: createMemory(),
    _start() {
      wasi.wasiImport.proc_exit(7);
    },
  },
};
console.log("before exit");
wasi.start(instance);
console.log("unreachable");
