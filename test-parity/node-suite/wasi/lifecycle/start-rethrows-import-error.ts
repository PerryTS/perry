import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
const wasi = new W({ version: "preview1", returnOnExit: true });
wasi.wasiImport.proc_exit = () => {
  throw new Error("patched proc_exit");
};
const instance = {
  exports: {
    memory: createMemory(),
    _start() {
      wasi.wasiImport.proc_exit(7);
    },
  },
};
try {
  wasi.start(instance);
  console.log("start: ok");
} catch (error: any) {
  console.log("start: throw", error?.name, error?.message);
}
try {
  wasi.start(instance);
  console.log("retry: ok");
} catch (error: any) {
  console.log("retry: throw", error?.name, error?.code || "no-code");
}
