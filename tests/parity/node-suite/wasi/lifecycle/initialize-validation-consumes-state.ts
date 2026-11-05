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
show(
  "start export present",
  () => wasi.initialize({ exports: { memory, _start() {} } }),
);
show("valid after failure", () =>
  wasi.initialize({
    exports: {
      memory,
      _initialize() {
        calls++;
      },
    },
  }));
console.log("initialize calls:", calls);
