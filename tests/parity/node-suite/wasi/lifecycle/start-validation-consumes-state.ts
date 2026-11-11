import { WASI } from "node:wasi";

const W: any = WASI;
function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
function show(label: string, fn: () => any) {
  try {
    console.log(label + ": ok", String(fn()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

const memory = createMemory();
const wasi = new W({ version: "preview1" });
let calls = 0;
show("missing start", () => wasi.start({ exports: { memory } }));
show("valid after failure", () =>
  wasi.start({
    exports: {
      memory,
      _start() {
        calls++;
      },
    },
  }));
console.log("start calls:", calls);
